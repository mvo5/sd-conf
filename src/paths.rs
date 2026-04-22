use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SearchPaths {
    pub dirs: Vec<PathBuf>,
}

impl SearchPaths {
    /// Build from explicit directories. First entry has highest priority.
    #[must_use]
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self { dirs }
    }

    /// Standard `/etc`, `/run`, `/usr/local/lib`, `/usr/lib` hierarchy for a
    /// named project. For `foobar` you get `/etc/foobar`, `/run/foobar`,
    /// `/usr/local/lib/foobar`, `/usr/lib/foobar` in that priority order.
    #[must_use]
    pub fn standard(project: &str) -> Self {
        Self::standard_with_root(project, Path::new("/"))
    }

    /// Same as [`SearchPaths::standard`], rooted at `root`. Useful for
    /// chroots, containers, and tests.
    #[must_use]
    pub fn standard_with_root(project: &str, root: &Path) -> Self {
        let tiers = ["etc", "run", "usr/local/lib", "usr/lib"];
        let dirs = tiers.iter().map(|t| root.join(t).join(project)).collect();
        Self { dirs }
    }
}
