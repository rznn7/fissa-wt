use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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
    Ok(manifest.worktree.copy.iter().map(PathBuf::from).collect())
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
