use crate::dropins::discover;
use crate::parser::parse_file;
use crate::paths::SearchPaths;
use crate::Error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Config {
    entries: Vec<Merged>,
    index: HashMap<(String, String), usize>,
    sources: Vec<PathBuf>,
}

struct Merged {
    section: String,
    key: String,
    value: String,
    source: PathBuf,
}

impl Config {
    /// Convenience: load `name` using [`SearchPaths::standard`] for `project`.
    /// Equivalent to `Config::load(name, &SearchPaths::standard(project))`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on file I/O failures and [`Error::Parse`] on
    /// malformed configuration.
    pub fn load_project(project: &str, name: &str) -> Result<Self, Error> {
        Self::load(name, &SearchPaths::standard(project))
    }

    /// Load `name` and its `name.d/*.conf` drop-ins from `paths`, merged.
    /// Later writes overwrite earlier ones.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on file I/O failures and [`Error::Parse`] on
    /// malformed configuration.
    pub fn load(name: &str, paths: &SearchPaths) -> Result<Self, Error> {
        let files = discover(name, paths);
        let mut entries: Vec<Merged> = Vec::new();
        let mut index: HashMap<(String, String), usize> = HashMap::new();
        let mut sources: Vec<PathBuf> = Vec::with_capacity(files.len());

        for path in files {
            let raw = parse_file(&path)?;
            sources.push(path.clone());
            for e in raw {
                let key = (e.section.clone(), e.key.clone());
                if let Some(&i) = index.get(&key) {
                    entries[i].value = e.value;
                    entries[i].source = path.clone();
                } else {
                    index.insert(key, entries.len());
                    entries.push(Merged {
                        section: e.section,
                        key: e.key,
                        value: e.value,
                        source: path.clone(),
                    });
                }
            }
        }

        Ok(Self {
            entries,
            index,
            sources,
        })
    }

    /// Final value for `(section, key)` after all overrides.
    #[must_use]
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.lookup(section, key).map(|m| m.value.as_str())
    }

    /// File that provided the current value. Useful for diagnostics.
    #[must_use]
    pub fn source_of(&self, section: &str, key: &str) -> Option<&Path> {
        self.lookup(section, key).map(|m| m.source.as_path())
    }

    /// Iterate `(key, value)` pairs in a section, in first-insertion order.
    pub fn section<'a>(&'a self, name: &str) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        let name = name.to_string();
        self.entries
            .iter()
            .filter(move |e| e.section == name)
            .map(|e| (e.key.as_str(), e.value.as_str()))
    }

    /// Every file that contributed, in apply order.
    #[must_use]
    pub fn sources(&self) -> &[PathBuf] {
        &self.sources
    }

    fn lookup(&self, section: &str, key: &str) -> Option<&Merged> {
        // HashMap requires an owned key; a small allocation is fine here.
        let k = (section.to_string(), key.to_string());
        self.index.get(&k).map(|&i| &self.entries[i])
    }
}
