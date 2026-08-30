use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Manifest {
    worktree: Worktree,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Worktree {
    copy: Vec<String>,
}

pub fn parse(text: &str) -> Result<Vec<PathBuf>> {
    let manifest: Manifest = toml::from_str(text)?;
    manifest
        .worktree
        .copy
        .iter()
        .map(String::as_str)
        .map(validate)
        .collect()
}

fn validate(entry: &str) -> Result<PathBuf> {
    if entry.is_empty() {
        return Err(anyhow!("a copy entry is empty"));
    }
    if entry.contains(['*', '?', '[']) {
        return Err(anyhow!(
            "copy entry '{entry}' looks like a pattern; only literal file paths are supported"
        ));
    }
    if entry.ends_with('/') {
        return Err(anyhow!(
            "copy entry '{entry}' is a directory; only files are supported"
        ));
    }

    let path = PathBuf::from(entry);
    if path.is_absolute() {
        return Err(anyhow!(
            "copy entry '{entry}' must be relative to the repository root"
        ));
    }
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(anyhow!(
            "copy entry '{entry}' must not leave the repository root"
        ));
    }
    if path.components().next() == Some(Component::Normal(OsStr::new(".git"))) {
        return Err(anyhow!(
            "copy entry '{entry}' must not reach into the git directory"
        ));
    }

    Ok(path)
}

/// A missing manifest is not an error: it is how most repositories run.
pub fn load(source_root: &Path) -> Result<Vec<PathBuf>> {
    let path = source_root.join(".fissa.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    };
    parse(&text).with_context(|| format!("invalid manifest in {}", path.display()))
}

fn resolve_in_root(root: &Path, rel: &Path) -> Result<PathBuf, &'static str> {
    let mut path = root.to_path_buf();
    let mut components = rel.components().peekable();

    while let Some(component) = components.next() {
        path.push(component);
        if components.peek().is_none() {
            break;
        }
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("skipped (symlink in the path)");
            }
            Ok(meta) if !meta.is_dir() => return Err("skipped (a file blocks the path)"),
            _ => {}
        }
    }

    Ok(path)
}

pub fn copy_file(source_root: &Path, dest_root: &Path, rel: &Path) -> Result<String> {
    let source = match resolve_in_root(source_root, rel) {
        Ok(source) => source,
        Err(detail) => return Ok(String::from(detail)),
    };
    let Ok(meta) = std::fs::symlink_metadata(&source) else {
        return Ok(String::from("skipped (not in the source)"));
    };
    if meta.file_type().is_symlink() {
        return Ok(String::from("skipped (symlink)"));
    }
    if !meta.is_file() {
        return Ok(String::from("skipped (not a regular file)"));
    }

    let dest = match resolve_in_root(dest_root, rel) {
        Ok(dest) => dest,
        Err(detail) => return Ok(String::from(detail)),
    };
    if std::fs::symlink_metadata(&dest).is_ok() {
        return Ok(String::from("skipped (already in the worktree)"));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }

    let bytes = std::fs::copy(&source, &dest)
        .with_context(|| format!("could not copy {}", rel.display()))?;

    Ok(format!("copied ({})", human_bytes(bytes)))
}

fn human_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;

    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejection(entry: &str) -> String {
        let text = format!("[worktree]\ncopy = [\"{entry}\"]");
        parse(&text)
            .expect_err("expected the entry to be rejected")
            .to_string()
    }

    #[test]
    fn test_parse_rejects_an_absolute_path() {
        assert!(rejection("/etc/passwd").contains("/etc/passwd"));
    }

    #[test]
    fn test_parse_rejects_a_path_that_climbs_out_of_the_repository() {
        assert!(rejection("../../.ssh/id_rsa").contains("id_rsa"));
    }

    #[test]
    fn test_parse_rejects_a_single_dot_component() {
        assert!(rejection("./.env").contains(".env"));
    }

    #[test]
    fn test_parse_rejects_a_wildcard() {
        let message = rejection(".env*");
        assert!(message.contains("literal"), "{message}");
    }

    #[test]
    fn test_parse_rejects_a_directory() {
        let message = rejection("config/");
        assert!(message.contains("file"), "{message}");
    }

    #[test]
    fn test_parse_rejects_an_empty_entry() {
        assert!(parse("[worktree]\ncopy = [\"\"]").is_err());
    }

    #[test]
    fn test_parse_keeps_accepting_a_nested_literal_path() {
        assert_eq!(
            parse("[worktree]\ncopy = [\"config/secrets.local.yml\"]").unwrap(),
            vec![PathBuf::from("config/secrets.local.yml")]
        );
    }

    #[test]
    fn test_parse_empty_text_declares_nothing() {
        assert_eq!(parse("").unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn test_parse_reads_the_worktree_copy_list() {
        let text = "[worktree]\ncopy = [\".env\", \"config/local.yml\"]";
        assert_eq!(
            parse(text).unwrap(),
            vec![PathBuf::from(".env"), PathBuf::from("config/local.yml")]
        );
    }

    #[test]
    fn test_parse_ignores_tables_it_does_not_know() {
        let text = "[hooks]\npost_create = [\"x\"]";
        assert_eq!(parse(text).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn test_parse_rejects_malformed_toml() {
        assert!(parse("[worktree]\ncopy = ").is_err());
    }

    #[test]
    fn test_load_from_a_repository_without_a_manifest_declares_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn test_load_reads_the_manifest_at_the_repository_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".fissa.toml"),
            "[worktree]\ncopy = [\".env\"]",
        )
        .unwrap();
        assert_eq!(load(dir.path()).unwrap(), vec![PathBuf::from(".env")]);
    }

    #[test]
    fn test_load_error_names_the_offending_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".fissa.toml");
        std::fs::write(&path, "[worktree]\ncopy = ").unwrap();
        let message = load(dir.path()).unwrap_err().to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn test_copy_file_puts_the_declared_file_in_the_worktree() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join(".env"), "TOKEN=1").unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.path().join(".env")).unwrap(),
            "TOKEN=1"
        );
        assert!(detail.starts_with("copied"), "{detail}");
    }

    #[test]
    fn test_copy_file_skips_a_source_that_does_not_exist() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(detail, "skipped (not in the source)");
        assert!(!dest.path().join(".env").exists());
    }

    #[test]
    fn test_copy_file_leaves_a_destination_that_already_exists() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join(".env"), "from source").unwrap();
        std::fs::write(dest.path().join(".env"), "already here").unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(detail, "skipped (already in the worktree)");
        assert_eq!(
            std::fs::read_to_string(dest.path().join(".env")).unwrap(),
            "already here"
        );
    }

    #[test]
    fn test_copy_file_skips_a_symlink_without_following_it() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let secret = source.path().join("secret");
        std::fs::write(&secret, "not yours").unwrap();
        std::os::unix::fs::symlink(&secret, source.path().join(".env")).unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(detail, "skipped (symlink)");
        assert!(!dest.path().join(".env").exists());
    }

    #[test]
    fn test_copy_file_skips_a_directory_standing_where_a_file_was_declared() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join(".env")).unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(detail, "skipped (not a regular file)");
    }

    #[test]
    fn test_copy_file_creates_a_parent_the_worktree_does_not_have() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join("config")).unwrap();
        std::fs::write(source.path().join("config/local.yml"), "k: v").unwrap();

        copy_file(source.path(), dest.path(), Path::new("config/local.yml")).unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.path().join("config/local.yml")).unwrap(),
            "k: v"
        );
    }

    #[test]
    fn test_parse_rejects_an_entry_inside_the_git_directory() {
        let message = rejection(".git/hooks/pre-commit");
        assert!(message.contains(".git/hooks/pre-commit"), "{message}");
    }

    #[test]
    fn test_copy_file_skips_a_symlinked_directory_in_the_source_path() {
        let outside = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("token"), "not yours").unwrap();
        std::os::unix::fs::symlink(outside.path(), source.path().join("esc")).unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new("esc/token")).unwrap();

        assert_eq!(detail, "skipped (symlink in the path)");
        assert!(!dest.path().join("esc").exists());
        assert!(!outside.path().join("esc").exists());
        assert_eq!(
            std::fs::read_dir(outside.path()).unwrap().count(),
            1,
            "nothing may be written outside the roots"
        );
    }

    #[test]
    fn test_copy_file_skips_a_symlinked_directory_in_the_destination_path() {
        let outside = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join("esc")).unwrap();
        std::fs::write(source.path().join("esc/token"), "TOKEN=1").unwrap();
        std::os::unix::fs::symlink(outside.path(), dest.path().join("esc")).unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new("esc/token")).unwrap();

        assert_eq!(detail, "skipped (symlink in the path)");
        assert!(!outside.path().join("token").exists());
        assert_eq!(
            std::fs::read_dir(outside.path()).unwrap().count(),
            0,
            "nothing may be written outside the worktree root"
        );
    }

    #[test]
    fn test_copy_file_skips_a_file_standing_where_a_destination_parent_belongs() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join("config")).unwrap();
        std::fs::write(source.path().join("config/local.yml"), "k: v").unwrap();
        std::fs::write(dest.path().join("config"), "i am a file").unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new("config/local.yml")).unwrap();

        assert_eq!(detail, "skipped (a file blocks the path)");
        assert_eq!(
            std::fs::read_to_string(dest.path().join("config")).unwrap(),
            "i am a file"
        );
    }

    #[test]
    fn test_load_reports_a_manifest_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".fissa.toml");
        std::fs::create_dir(&path).unwrap();

        let message = load(dir.path()).unwrap_err().to_string();

        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn test_copy_file_preserves_the_permissions_of_a_private_file() {
        use std::os::unix::fs::PermissionsExt;

        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let path = source.path().join(".env");
        std::fs::write(&path, "TOKEN=1").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        let mode = std::fs::metadata(dest.path().join(".env"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
