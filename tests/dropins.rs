//! Integration test ported from systemd
//! src/test/test-conf-parser.c:407 (`test_config_parse_standard_file_with_dropins_full`).
//!
//! Layout under a temp root, using project name "kernel" and config
//! "install.conf":
//!
//!   /usr/lib/kernel/install.conf              -> "A=!!!"   (shadowed)
//!   /usr/local/lib/kernel/install.conf        -> "A=aaa"
//!   /usr/local/lib/kernel/install.conf.d/drop1.conf -> "B=bbb"
//!   /usr/local/lib/kernel/install.conf.d/drop2.conf -> "C=c1"
//!   /usr/lib/kernel/install.conf.d/drop2.conf -> "C=c2"    (shadowed by c1)
//!   /run/kernel/install.conf.d/drop3.conf     -> "D=ddd"
//!   /etc/kernel/install.conf.d/drop4.conf     -> "E=eee"
//!
//! Expected: A=aaa, B=bbb, C=c1, D=ddd, E=eee, F=None.

use sd_conf::{Config, SearchPaths};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sd-conf-{}-{}-{}-{}",
            tag,
            std::process::id(),
            nanos,
            n
        ));
        fs::create_dir_all(&root).expect("create tmp root");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, contents: &str) {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).expect("mkdir");
        fs::write(&p, contents).expect("write file");
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dropins_merge_across_tiers() {
    let root = TempRoot::new("dropins");

    root.write("usr/lib/kernel/install.conf", "A=!!!\n");
    root.write("usr/local/lib/kernel/install.conf", "A=aaa\n");
    root.write("usr/local/lib/kernel/install.conf.d/drop1.conf", "B=bbb\n");
    root.write("usr/local/lib/kernel/install.conf.d/drop2.conf", "C=c1\n");
    root.write("usr/lib/kernel/install.conf.d/drop2.conf", "C=c2\n");
    root.write("run/kernel/install.conf.d/drop3.conf", "D=ddd\n");
    root.write("etc/kernel/install.conf.d/drop4.conf", "E=eee\n");

    let paths = SearchPaths::standard_with_root("kernel", root.path());
    let cfg = Config::load("install.conf", &paths).expect("load");

    // Entries in these files have no [Section] header, so section == "".
    assert_eq!(cfg.get("", "A"), Some("aaa"));
    assert_eq!(cfg.get("", "B"), Some("bbb"));
    assert_eq!(cfg.get("", "C"), Some("c1"));
    assert_eq!(cfg.get("", "D"), Some("ddd"));
    assert_eq!(cfg.get("", "E"), Some("eee"));
    assert_eq!(cfg.get("", "F"), None);

    // source_of should point at the winning tier.
    let c_src = cfg.source_of("", "C").expect("C has a source");
    assert!(
        c_src.ends_with("usr/local/lib/kernel/install.conf.d/drop2.conf"),
        "expected /usr/local/lib to shadow /usr/lib for drop2.conf, got {}",
        c_src.display()
    );

    // sources() lists one fragment + four distinct drop-ins (drop2.conf deduped).
    assert_eq!(cfg.sources().len(), 5);
}

#[test]
fn missing_everything_yields_empty_config() {
    let root = TempRoot::new("empty");
    let paths = SearchPaths::standard_with_root("nothing", root.path());
    let cfg = Config::load("nothing.conf", &paths).expect("load");
    assert_eq!(cfg.get("", "anything"), None);
    assert!(cfg.sources().is_empty());
}

#[test]
fn etc_overrides_usr_for_fragment() {
    let root = TempRoot::new("frag-override");
    root.write("usr/lib/foo/main.conf", "[S]\nkey=from-usr\n");
    root.write("etc/foo/main.conf", "[S]\nkey=from-etc\n");

    let paths = SearchPaths::standard_with_root("foo", root.path());
    let cfg = Config::load("main.conf", &paths).expect("load");
    assert_eq!(cfg.get("S", "key"), Some("from-etc"));
    assert_eq!(cfg.sources().len(), 1);
}

#[test]
fn dropins_override_fragment() {
    let root = TempRoot::new("frag-vs-dropin");
    root.write("usr/lib/foo/main.conf", "[S]\nkey=from-fragment\n");
    root.write(
        "usr/lib/foo/main.conf.d/99-override.conf",
        "[S]\nkey=from-dropin\n",
    );

    let paths = SearchPaths::standard_with_root("foo", root.path());
    let cfg = Config::load("main.conf", &paths).expect("load");
    assert_eq!(cfg.get("S", "key"), Some("from-dropin"));
}

#[test]
fn dropins_apply_in_lexical_order() {
    let root = TempRoot::new("lex-order");
    root.write("etc/foo/main.conf.d/10-first.conf", "[S]\nkey=first\n");
    root.write("etc/foo/main.conf.d/20-second.conf", "[S]\nkey=second\n");
    root.write("etc/foo/main.conf.d/30-third.conf", "[S]\nkey=third\n");

    let paths = SearchPaths::standard_with_root("foo", root.path());
    let cfg = Config::load("main.conf", &paths).expect("load");
    assert_eq!(cfg.get("S", "key"), Some("third"));
}

#[test]
fn get_string_returns_owned() {
    let root = TempRoot::new("get-string");
    root.write("etc/foo/main.conf", "[S]\nk=hello\n");

    let paths = SearchPaths::standard_with_root("foo", root.path());
    let cfg = Config::load("main.conf", &paths).expect("load");

    let owned: Option<String> = cfg.get_string("S", "k");
    assert_eq!(owned.as_deref(), Some("hello"));
    assert_eq!(cfg.get_string("S", "missing"), None);
}

#[test]
fn get_bool_happy_path() {
    let root = TempRoot::new("bool-ok");
    root.write(
        "etc/foo/main.conf",
        "[Settings]\nenabled=yes\ndisabled=0\nmissing_in_other_files=on\n",
    );

    let paths = SearchPaths::standard_with_root("foo", root.path());
    let cfg = Config::load("main.conf", &paths).expect("load");

    assert_eq!(cfg.get_bool("Settings", "enabled").unwrap(), Some(true));
    assert_eq!(cfg.get_bool("Settings", "disabled").unwrap(), Some(false));
    assert_eq!(
        cfg.get_bool("Settings", "missing_in_other_files").unwrap(),
        Some(true)
    );
    assert_eq!(cfg.get_bool("Settings", "never_mentioned").unwrap(), None);
}

#[test]
fn get_bool_reports_source_and_line_on_error() {
    let root = TempRoot::new("bool-err");
    // Two lines, bad bool is on line 2.
    root.write("etc/foo/main.conf", "[Settings]\nenabled=maybe\n");

    let paths = SearchPaths::standard_with_root("foo", root.path());
    let cfg = Config::load("main.conf", &paths).expect("load");

    let err = cfg.get_bool("Settings", "enabled").unwrap_err();
    let expected = format!(
        "{}/etc/foo/main.conf:2: [Settings] enabled: invalid boolean (expected yes/no/true/false/on/off/y/n/t/f/1/0)",
        root.path().display()
    );
    assert_eq!(err.to_string(), expected);
}

#[test]
fn non_conf_extension_ignored_in_dropin_dir() {
    let root = TempRoot::new("bad-ext");
    root.write("etc/foo/main.conf.d/a.conf", "[S]\nkey=yes\n");
    root.write("etc/foo/main.conf.d/b.txt", "[S]\nkey=no\n");

    let paths = SearchPaths::standard_with_root("foo", root.path());
    let cfg = Config::load("main.conf", &paths).expect("load");
    assert_eq!(cfg.get("S", "key"), Some("yes"));
    assert_eq!(cfg.sources().len(), 1);
}
