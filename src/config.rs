use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    List,
    Create,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_mode: Mode,
}

pub fn parse(text: &str) -> Result<Config> {
    Ok(toml::from_str(text)?)
}

fn config_path(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let base = match xdg.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(home.filter(|dir| !dir.is_empty())?).join(".config"),
    };
    Some(base.join("fissa").join("config.toml"))
}

/// A missing file is not an error: it is how most repositories run.
fn load_from(path: Option<PathBuf>) -> Result<Config> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Config::default());
    };
    parse(&text).with_context(|| format!("invalid configuration in {}", path.display()))
}

pub fn load() -> Result<Config> {
    load_from(config_path(
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_empty_config_defaults_to_list() {
        assert_eq!(parse("").unwrap().default_mode, Mode::List);
    }

    #[test]
    fn test_parse_reads_create_mode() {
        assert_eq!(
            parse("default_mode = \"create\"").unwrap().default_mode,
            Mode::Create
        );
    }

    #[test]
    fn test_parse_rejects_an_unknown_mode() {
        assert!(parse("default_mode = \"editor\"").is_err());
    }

    #[test]
    fn test_parse_ignores_keys_it_does_not_know() {
        assert_eq!(parse("future_key = 3").unwrap().default_mode, Mode::List);
    }

    #[test]
    fn test_config_path_prefers_xdg_config_home() {
        assert_eq!(
            config_path(Some("/xdg"), Some("/home/u")),
            Some(PathBuf::from("/xdg/fissa/config.toml"))
        );
    }

    #[test]
    fn test_config_path_falls_back_to_dot_config_under_home() {
        assert_eq!(
            config_path(None, Some("/home/u")),
            Some(PathBuf::from("/home/u/.config/fissa/config.toml"))
        );
    }

    #[test]
    fn test_config_path_ignores_an_empty_xdg_config_home() {
        assert_eq!(
            config_path(Some(""), Some("/home/u")),
            Some(PathBuf::from("/home/u/.config/fissa/config.toml"))
        );
    }

    #[test]
    fn test_config_path_is_absent_without_a_home() {
        assert_eq!(config_path(None, None), None);
    }

    #[test]
    fn test_load_from_a_missing_file_is_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("fissa/config.toml");
        assert_eq!(load_from(Some(missing)).unwrap().default_mode, Mode::List);
    }

    #[test]
    fn test_load_without_a_config_path_is_the_default() {
        assert_eq!(load_from(None).unwrap().default_mode, Mode::List);
    }

    #[test]
    fn test_load_reads_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "default_mode = \"create\"").unwrap();
        assert_eq!(load_from(Some(path)).unwrap().default_mode, Mode::Create);
    }

    #[test]
    fn test_load_error_names_the_offending_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "default_mode = \"editor\"").unwrap();
        let message = load_from(Some(path.clone())).unwrap_err().to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
    }

    #[test]
    fn test_parse_rejects_malformed_toml() {
        assert!(parse("default_mode = ").is_err());
    }
}
