//! Discover the fragment file and its drop-ins across a [`SearchPaths`].
//!
//! Mirrors the semantics of systemd's
//! `config_parse_standard_file_with_dropins_full` (src/shared/conf-parser.c)
//! and `conf_files_list_dropins` (src/basic/conf-files.c).

use crate::paths::SearchPaths;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Return files to load in apply order: fragment first, then drop-ins
/// sorted by basename. Drop-ins sharing a basename across tiers are
/// deduplicated with the higher-priority tier winning.
pub(crate) fn discover(name: &str, paths: &SearchPaths) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = Vec::new();

    // Fragment: highest-priority tier that has it wins.
    for dir in &paths.dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            result.push(candidate);
            break;
        }
    }

    // Drop-ins: for each basename, keep the highest-priority tier's copy.
    // Walk lowest -> highest so later inserts overwrite.
    // BTreeMap so iteration yields drop-ins in basename order (required for
    // deterministic apply order: 10-foo.conf before 20-bar.conf).
    let dropin_dir_name = format!("{name}.d");
    let mut dropins: BTreeMap<String, PathBuf> = BTreeMap::new();
    for dir in paths.dirs.iter().rev() {
        let dropin_path = dir.join(&dropin_dir_name);
        let Ok(rd) = fs::read_dir(&dropin_path) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("conf") {
                continue;
            }
            let Some(basename) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            dropins.insert(basename.to_string(), p.clone());
        }
    }

    result.extend(dropins.into_values());
    result
}
