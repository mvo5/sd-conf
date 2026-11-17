# sd-conf

Rust library for reading systemd-style INI configuration files with drop-in
override support.

## Example

```rust
use sd_conf::Config;

let cfg = Config::load_project("foobar", "main.conf")?;

if let Some(url) = cfg.get("Settings", "download-url") {
    println!("download-url = {url}");
}
```

For non-standard layouts (custom directories, a chroot root, etc.) use
`SearchPaths::new(...)` or `SearchPaths::standard_with_root(...)` and
pass the result to `Config::load`.

## Parsing a string

`parse_str` parses input that is not a file on disk, for example a unit
file embedded in another document. It returns every assignment in order,
so a key that is assigned more than once is returned more than once:

```rust
use sd_conf::parse_str;

let entries = parse_str("[Install]\nWantedBy=a.target\nWantedBy=b.target\n")?;
for e in entries.iter().filter(|e| e.section == "Install") {
    println!("line {}: {} = {}", e.line, e.key, e.value);
}
```

## Search order

For project `foobar`, `main.conf` is loaded from these directories in
override priority (higher beats lower):

```
/etc/foobar/              (plus /etc/foobar/main.conf.d/*.conf)
/run/foobar/
/usr/local/lib/foobar/
/usr/lib/foobar/
```

The fragment is read from the highest-priority directory that has it. Drop-in
files (`*.conf` under `<name>.d/`) are collected across every directory,
deduplicated by basename with the higher-priority tier winning, and applied in
lexicographic order after the fragment. Later writes overwrite earlier ones.

## Syntax

```
[Section]
key = value
long-value = part one \
             part two
# comment (or ;)
```

Embedded NUL bytes are rejected. UTF-8 BOM at the start of a file is skipped.
