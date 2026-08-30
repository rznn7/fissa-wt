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

    Ok(path)
}

/// A missing manifest is not an error: it is how most repositories run.
pub fn load(source_root: &Path) -> Result<Vec<PathBuf>> {
    let path = source_root.join(".fissa.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    parse(&text).with_context(|| format!("invalid manifest in {}", path.display()))
}

pub fn copy_file(source_root: &Path, dest_root: &Path, rel: &Path) -> Result<String> {
    let source = source_root.join(rel);
    let dest = dest_root.join(rel);

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
}
