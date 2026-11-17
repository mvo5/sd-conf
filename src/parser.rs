//! Line-oriented INI parser matching systemd's `config_parse()` semantics.
//!
//! Reference: systemd src/shared/conf-parser.c:282 (`config_parse`).

use crate::Error;
use std::path::Path;

/// One `key = value` assignment of the parsed input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The section the assignment is in, empty before the first section
    /// header.
    pub section: String,
    pub key: String,
    pub value: String,
    /// The line the assignment starts on, counted from 1.
    pub line: u32,
}

/// Why [`parse_str`] rejected its input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {reason}")]
pub struct ParseError {
    /// The line the rejected input starts on, counted from 1.
    pub line: u32,
    pub reason: &'static str,
}

pub(crate) fn parse_file(path: &Path) -> Result<Vec<Entry>, Error> {
    let parse_error = |line, reason| Error::Parse {
        path: path.to_path_buf(),
        line,
        reason,
    };
    let text = String::from_utf8(std::fs::read(path)?).map_err(|e| {
        let valid = String::from_utf8_lossy(&e.as_bytes()[..e.utf8_error().valid_up_to()]);
        let newlines = valid.matches('\n').count();
        parse_error(line_number(newlines), "invalid UTF-8")
    })?;
    parse_str(&text).map_err(|e| parse_error(e.line, e.reason))
}

/// Parse `input` in the syntax of systemd configuration and unit files.
/// Returns every assignment in the order of the input, so a key that is
/// assigned more than once is returned more than once.
///
/// # Errors
///
/// Returns a [`ParseError`] for a NUL byte, a section header without `]`,
/// a line that is neither a section header nor an assignment, and an
/// assignment without a key.
pub fn parse_str(input: &str) -> Result<Vec<Entry>, ParseError> {
    let mut entries = Vec::new();
    let mut section = String::new();
    let mut continuation: Option<String> = None;
    let mut continuation_start: u32 = 0;

    let input = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    for (idx, line) in input.lines().enumerate() {
        let line_no = line_number(idx);
        let line = line.to_string();

        if line.contains('\0') {
            return Err(ParseError {
                line: line_no,
                reason: "NUL byte in input",
            });
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

        process_logical(&logical, &mut section, &mut entries, continuation_start)?;
    }

    // File ended mid-continuation. Parse what we have.
    if let Some(logical) = continuation {
        process_logical(&logical, &mut section, &mut entries, continuation_start)?;
    }

    Ok(entries)
}

/// The number of the line with the zero-based index `idx`.
fn line_number(idx: usize) -> u32 {
    u32::try_from(idx)
        .ok()
        .and_then(|n| n.checked_add(1))
        .expect("config file has more than u32::MAX lines")
}

fn process_logical(
    raw: &str,
    section: &mut String,
    entries: &mut Vec<Entry>,
    line_no: u32,
) -> Result<(), ParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    if let Some(rest) = trimmed.strip_prefix('[') {
        let Some(name) = rest.strip_suffix(']') else {
            return Err(ParseError {
                line: line_no,
                reason: "section header missing ']'",
            });
        };
        *section = name.to_string();
        return Ok(());
    }

    let Some((k, v)) = trimmed.split_once('=') else {
        return Err(ParseError {
            line: line_no,
            reason: "expected 'key = value' or '[Section]'",
        });
    };
    let key = k.trim();
    if key.is_empty() {
        return Err(ParseError {
            line: line_no,
            reason: "empty key before '='",
        });
    }
    entries.push(Entry {
        section: section.clone(),
        key: key.to_string(),
        value: v.trim().to_string(),
        line: line_no,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Vec<Entry> {
        parse_str(input).expect("parse ok")
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
        assert_eq!(
            parse_str(input),
            Err(ParseError {
                line: 2,
                reason: "NUL byte in input",
            })
        );
    }

    #[test]
    fn semicolon_comment() {
        let input = "[Section]\n; a semicolon comment\nsetting1=1\n";
        assert_eq!(setting1(input).as_deref(), Some("1"));
    }

    #[test]
    fn unterminated_section_header_errors() {
        let r = parse_str("[Section\n");
        assert!(matches!(r, Err(ParseError { reason, .. }) if reason.contains("section header")));
    }

    #[test]
    fn missing_equals_errors() {
        assert!(parse_str("[Section]\njust_a_word\n").is_err());
    }

    #[test]
    fn empty_key_errors() {
        assert_eq!(
            parse_str("[Section]\n=value\n"),
            Err(ParseError {
                line: 2,
                reason: "empty key before '='",
            })
        );
    }

    #[test]
    fn repeated_keys_are_all_returned_in_order() {
        let input = "before=x\n[Install]\nWantedBy=a.target \\\n  b.target\nWantedBy=\n";
        let entry = |section: &str, key: &str, value: &str, line| Entry {
            section: section.to_string(),
            key: key.to_string(),
            value: value.to_string(),
            line,
        };
        assert_eq!(
            parse(input),
            [
                entry("", "before", "x", 1),
                entry("Install", "WantedBy", "a.target    b.target", 3),
                entry("Install", "WantedBy", "", 5),
            ]
        );
    }

    #[test]
    fn parse_error_names_the_line() {
        let err = parse_str("[Section]\n\njust_a_word\n").unwrap_err();
        assert_eq!(
            err.to_string(),
            "line 3: expected 'key = value' or '[Section]'"
        );
    }

    #[test]
    fn parse_file_reports_path_and_line() {
        let dir = std::env::temp_dir().join(format!("sd-conf-parser-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.conf");

        std::fs::write(&path, b"[Section]\nkey=\xff\n").unwrap();
        let utf8 = parse_file(&path).unwrap_err().to_string();
        std::fs::write(&path, "[Section]\n=value\n").unwrap();
        let syntax = parse_file(&path).unwrap_err().to_string();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(utf8, format!("{}:2: invalid UTF-8", path.display()));
        assert_eq!(
            syntax,
            format!("{}:2: empty key before '='", path.display())
        );
    }
}
