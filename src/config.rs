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
    line: u32,
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
                    entries[i].source.clone_from(&path);
                    entries[i].line = e.line;
                } else {
                    index.insert(key, entries.len());
                    entries.push(Merged {
                        section: e.section,
                        key: e.key,
                        value: e.value,
                        source: path.clone(),
                        line: e.line,
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

    /// Final value as an owned `String`. Convenience for moving values into
    /// owned structs; prefer [`Config::get`] when a `&str` suffices.
    #[must_use]
    pub fn get_string(&self, section: &str, key: &str) -> Option<String> {
        self.get(section, key).map(str::to_owned)
    }

    /// Final value interpreted as a boolean. Accepts the same spellings as
    /// systemd (`parse_boolean` in src/basic/parse-util.c):
    /// `1`/`yes`/`y`/`true`/`t`/`on` and `0`/`no`/`n`/`false`/`f`/`off`
    /// (case-insensitive except for `0`/`1`).
    ///
    /// Returns `Ok(None)` when the key is unset, `Ok(Some(b))` on success,
    /// and `Err(Error::InvalidValue)` when the key is set but unparseable.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidValue`] when the key is set to a value that
    /// does not match any of the accepted boolean spellings.
    pub fn get_bool(&self, section: &str, key: &str) -> Result<Option<bool>, Error> {
        let Some(m) = self.lookup(section, key) else {
            return Ok(None);
        };
        if let Some(b) = parse_bool(&m.value) {
            return Ok(Some(b));
        }
        Err(Error::InvalidValue {
            path: m.source.clone(),
            line: m.line,
            section: m.section.clone(),
            key: m.key.clone(),
            reason: "invalid boolean (expected yes/no/true/false/on/off/y/n/t/f/1/0)",
        })
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

/// Matches systemd's `parse_boolean` (src/basic/parse-util.c).
fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "yes" | "y" | "true" | "t" | "on" => Some(true),
        "0" | "no" | "n" | "false" | "f" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_bool;

    #[test]
    fn parses_true_spellings() {
        for s in [
            "1", "yes", "Yes", "YES", "y", "Y", "true", "True", "t", "T", "on", "ON",
        ] {
            assert_eq!(parse_bool(s), Some(true), "expected true for {s:?}");
        }
    }

    #[test]
    fn parses_false_spellings() {
        for s in [
            "0", "no", "No", "NO", "n", "N", "false", "False", "f", "F", "off", "OFF",
        ] {
            assert_eq!(parse_bool(s), Some(false), "expected false for {s:?}");
        }
    }

    #[test]
    fn rejects_garbage() {
        for s in ["", "maybe", "2", "-1", "truthy", "nope", " yes", "yes "] {
            assert_eq!(parse_bool(s), None, "expected None for {s:?}");
        }
    }
}
