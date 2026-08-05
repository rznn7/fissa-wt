#[derive(Debug, PartialEq, Eq)]
pub struct Names {
    pub branch: String,
    pub dir: String,
}

pub fn derive_names(slug: &str, prefix: &str, repo_dir: &str) -> Option<Names> {
    let slug = slug.trim();
    if slug.is_empty() {
        return None;
    }

    let branch = if slug.contains('/') {
        slug.to_string()
    } else {
        format!("{prefix}{slug}")
    };

    let last = branch.rsplit('/').next().unwrap_or(branch.as_str());
    let dir = format!("{repo_dir}-{}", sanitize(last));

    Some(Names { branch, dir })
}

pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_names_bare_slug_applies_prefix() {
        let n = derive_names("spe-11667", "feature/", "spectra").unwrap();
        assert_eq!(n.branch, "feature/spe-11667");
        assert_eq!(n.dir, "spectra-spe-11667");
    }

    #[test]
    fn test_derive_names_slug_with_slash_ignores_prefix() {
        let n = derive_names("fix/spe-11667", "feature/", "spectra").unwrap();
        assert_eq!(n.branch, "fix/spe-11667");
        assert_eq!(n.dir, "spectra-spe-11667");
    }

    #[test]
    fn test_derive_names_preserves_branch_case_but_lowercases_dir() {
        let n = derive_names("SPE-11721", "feature/", "spectra").unwrap();
        assert_eq!(n.branch, "feature/SPE-11721");
        assert_eq!(n.dir, "spectra-spe-11721");
    }

    #[test]
    fn test_derive_names_empty_slug_returns_none() {
        assert!(derive_names("   ", "feature/", "spectra").is_none());
    }

    #[test]
    fn test_derive_names_trims_surrounding_whitespace() {
        let n = derive_names("  spe-1  ", "feature/", "spectra").unwrap();
        assert_eq!(n.branch, "feature/spe-1");
    }

    #[test]
    fn test_derive_names_empty_prefix_yields_bare_branch() {
        let n = derive_names("spe-1", "", "spectra").unwrap();
        assert_eq!(n.branch, "spe-1");
    }

    #[test]
    fn test_sanitize_collapses_runs_of_invalid_chars() {
        assert_eq!(sanitize("Foo  Bar!!baz"), "foo-bar-baz");
    }

    #[test]
    fn test_sanitize_trims_leading_and_trailing_dashes() {
        assert_eq!(sanitize("--abc--"), "abc");
    }

    #[test]
    fn test_sanitize_keeps_dots_and_underscores() {
        assert_eq!(sanitize("v1.2_x"), "v1.2_x");
    }
}
