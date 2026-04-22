//! Line-oriented INI parser matching systemd's `config_parse()` semantics.
//!
//! Reference: systemd src/shared/conf-parser.c:282 (`config_parse`).

use crate::Error;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

pub(crate) struct Entry {
    pub section: String,
    pub key: String,
    pub value: String,
}

pub(crate) fn parse_file(path: &Path) -> Result<Vec<Entry>, Error> {
    let f = std::fs::File::open(path)?;
    parse_reader(BufReader::new(f), path)
}

pub(crate) fn parse_reader<R: BufRead>(reader: R, path: &Path) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    let mut section = String::new();
    let mut continuation: Option<String> = None;
    let mut continuation_start: u32 = 0;
    let mut bom_seen = false;

    for (idx, line_result) in reader.lines().enumerate() {
        let line_no: u32 = u32::try_from(idx)
            .ok()
            .and_then(|n| n.checked_add(1))
            .expect("config file has more than u32::MAX lines");
        let mut line = match line_result {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                return Err(Error::Parse {
                    path: path.to_path_buf(),
                    line: line_no,
                    reason: "invalid UTF-8",
                })
            }
            Err(e) => return Err(e.into()),
        };

        if line.contains('\0') {
            return Err(Error::Parse {
                path: path.to_path_buf(),
                line: line_no,
                reason: "NUL byte in input",
            });
        }

        if !bom_seen {
            bom_seen = true;
            if let Some(rest) = line.strip_prefix('\u{FEFF}') {
                line = rest.to_string();
            }
        }

        // Comments: leading whitespace + `#` or `;`. Comment lines are dropped
        // even while a continuation is in progress (matches systemd).
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        let mut logical = if let Some(buf) = continuation.take() {
            let mut buf = buf;
            buf.push_str(&line);
            buf
        } else {
            continuation_start = line_no;
            line
        };

        // Count trailing backslashes. Odd = last one is an unescaped
        // continuation marker.
        let mut escaped = false;
        for c in logical.chars() {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            }
        }

        if escaped {
            logical.pop();
            logical.push(' ');
            continuation = Some(logical);
            continue;
        }

        process_logical(
            &logical,
            &mut section,
            &mut entries,
            continuation_start,
            path,
        )?;
    }

    // File ended mid-continuation. Parse what we have.
    if let Some(logical) = continuation {
        process_logical(
            &logical,
            &mut section,
            &mut entries,
            continuation_start,
            path,
        )?;
    }

    Ok(entries)
}

fn process_logical(
    raw: &str,
    section: &mut String,
    entries: &mut Vec<Entry>,
    line_no: u32,
    path: &Path,
) -> Result<(), Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    if let Some(rest) = trimmed.strip_prefix('[') {
        let Some(name) = rest.strip_suffix(']') else {
            return Err(Error::Parse {
                path: path.to_path_buf(),
                line: line_no,
                reason: "section header missing ']'",
            });
        };
        *section = name.to_string();
        return Ok(());
    }

    let Some((k, v)) = trimmed.split_once('=') else {
        return Err(Error::Parse {
            path: path.to_path_buf(),
            line: line_no,
            reason: "expected 'key = value' or '[Section]'",
        });
    };
    let key = k.trim();
    if key.is_empty() {
        return Err(Error::Parse {
            path: path.to_path_buf(),
            line: line_no,
            reason: "empty key before '='",
        });
    }
    entries.push(Entry {
        section: section.clone(),
        key: key.to_string(),
        value: v.trim().to_string(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse(input: &str) -> Vec<Entry> {
        parse_reader(input.as_bytes(), &PathBuf::from("<test>")).expect("parse ok")
    }

    fn setting1(input: &str) -> Option<String> {
        parse(input)
            .into_iter()
            .rev()
            .find(|e| e.section == "Section" && e.key == "setting1")
            .map(|e| e.value)
    }

    // Test vectors ported from systemd src/test/test-conf-parser.c:231 (config_file[]).
    // Each case number below corresponds to the index in the systemd array.

    #[test]
    fn case_0_basic() {
        // src/test/test-conf-parser.c:232
        assert_eq!(setting1("[Section]\nsetting1=1\n").as_deref(), Some("1"));
    }

    #[test]
    fn case_1_no_terminating_newline() {
        // src/test/test-conf-parser.c:235
        assert_eq!(setting1("[Section]\nsetting1=1").as_deref(), Some("1"));
    }

    #[test]
    fn case_2_leading_whitespace_no_trailing_newline() {
        // src/test/test-conf-parser.c:238
        assert_eq!(
            setting1("\n\n\n\n[Section]\n\n\nsetting1=1").as_deref(),
            Some("1")
        );
    }

    #[test]
    fn case_3_repeated_settings_last_wins() {
        // src/test/test-conf-parser.c:241 (repeated sections and repeated keys)
        let input = "[Section]\n[Section]\nsetting1=1\nsetting1=    2 \t\nsetting1=    1\n";
        assert_eq!(setting1(input).as_deref(), Some("1"));
    }

    #[test]
    fn case_4_empty_line_breaks_continuation() {
        // src/test/test-conf-parser.c:247
        let input = "[Section]\n[Section]\nsetting1=1\nsetting1=2\\\n   \nsetting1=1\n";
        assert_eq!(setting1(input).as_deref(), Some("1"));
    }

    #[test]
    fn case_5_normal_continuation() {
        // src/test/test-conf-parser.c:254
        let input = "[Section]\nsetting1=1\\\n2\\\n3\n";
        assert_eq!(setting1(input).as_deref(), Some("1 2 3"));
    }

    #[test]
    fn case_6_continuation_marker_in_comment_ignored() {
        // src/test/test-conf-parser.c:259
        let input = "[Section]\n#hogehoge\\\nsetting1=1\\\n2\\\n3\n";
        assert_eq!(setting1(input).as_deref(), Some("1 2 3"));
    }

    #[test]
    fn case_7_comment_line_inside_continuation_ignored() {
        // src/test/test-conf-parser.c:265
        let input = "[Section]\nsetting1=1\\\n#hogehoge\\\n2\\\n3\n";
        assert_eq!(setting1(input).as_deref(), Some("1 2 3"));
    }

    #[test]
    fn case_8_whitespace_before_comments_and_keys() {
        // src/test/test-conf-parser.c:271
        let input = "[Section]\n   #hogehoge\\\n   setting1=1\\\n2\\\n3\n";
        assert_eq!(setting1(input).as_deref(), Some("1 2 3"));
    }

    #[test]
    fn case_9_indented_comment_inside_continuation() {
        // src/test/test-conf-parser.c:277
        let input = "[Section]\n   setting1=1\\\n   #hogehoge\\\n2\\\n3\n";
        assert_eq!(setting1(input).as_deref(), Some("1 2 3"));
    }

    #[test]
    fn case_10_extra_trailing_backslash_at_eof() {
        // src/test/test-conf-parser.c:283
        let input = "[Section]\nsetting1=1\\\n2\\\n3\\\n";
        assert_eq!(setting1(input).as_deref(), Some("1 2 3"));
    }

    #[test]
    fn case_11_escape_backslashes() {
        // src/test/test-conf-parser.c:288
        // Input bytes: `setting1=1\\\` + LF + `\\2` + LF
        // The final `\` before LF is an unescaped continuation marker
        // (odd count of trailing backslashes). Joined with a single space:
        // `1\\` + ` ` + `\\2` = `1\\ \\2`.
        let input = "[Section]\nsetting1=1\\\\\\\n\\\\2\n";
        assert_eq!(setting1(input).as_deref(), Some(r"1\\ \\2"));
    }

    #[test]
    fn multiple_sections_all_retained() {
        // Adapted from src/test/test-conf-parser.c:317 (case 17).
        // systemd's test filters by section list; we return all entries.
        let input = "\
[Section]
setting1=2
[NoWarnSection]
setting1=3
[WarnSection]
setting1=3
[X-Section]
setting1=3
";
        let es = parse(input);
        assert_eq!(es.len(), 4);
        assert_eq!(es[0].section, "Section");
        assert_eq!(es[0].value, "2");
        assert_eq!(es[1].section, "NoWarnSection");
        assert_eq!(es[2].section, "WarnSection");
        assert_eq!(es[3].section, "X-Section");
    }

    #[test]
    fn bom_is_stripped() {
        let input = "\u{FEFF}[Section]\nsetting1=1\n";
        assert_eq!(setting1(input).as_deref(), Some("1"));
    }

    #[test]
    fn nul_byte_rejected() {
        let input = "[Section]\nsetting1=\0foo\n";
        let r = parse_reader(input.as_bytes(), &PathBuf::from("<test>"));
        assert!(matches!(
            r,
            Err(Error::Parse {
                reason: "NUL byte in input",
                ..
            })
        ));
    }

    #[test]
    fn semicolon_comment() {
        let input = "[Section]\n; a semicolon comment\nsetting1=1\n";
        assert_eq!(setting1(input).as_deref(), Some("1"));
    }

    #[test]
    fn unterminated_section_header_errors() {
        let r = parse_reader(b"[Section\n".as_ref(), &PathBuf::from("<test>"));
        assert!(matches!(r, Err(Error::Parse { reason, .. }) if reason.contains("section header")));
    }

    #[test]
    fn missing_equals_errors() {
        let r = parse_reader(
            b"[Section]\njust_a_word\n".as_ref(),
            &PathBuf::from("<test>"),
        );
        assert!(matches!(r, Err(Error::Parse { .. })));
    }

    #[test]
    fn empty_key_errors() {
        let r = parse_reader(b"[Section]\n=value\n".as_ref(), &PathBuf::from("<test>"));
        assert!(matches!(
            r,
            Err(Error::Parse {
                reason: "empty key before '='",
                ..
            })
        ));
    }
}
