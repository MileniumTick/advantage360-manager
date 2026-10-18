//! Reading configuration files without losing anything.
//!
//! The reader never rewrites a line it did not have to. Each line keeps its raw text and its own
//! line terminator, so a file that is read and written back without edits is byte-identical.

use std::fs;
use std::path::Path;

use crate::error::{CoreError, CoreResult};
use crate::layer::{LayerId, ProfileId};
use crate::model::{
    CommentStyle, ConfigFile, Eol, FileRole, LedFunction, LedLine, Line, LineKind, MacroAtom,
    MacroLine, Origin, RemapAtom, RemapLine, Rgb, Stroke,
};
use crate::parse_error::{ParseError, SyntaxCode};
use crate::tokens;

/// The UTF-8 byte-order mark. Preserved so a BOM-prefixed file is not silently changed.
const BOM: char = '\u{feff}';

/// Read and parse a configuration file from disk.
///
/// `relative_path` is the path as it should be reported to the user, relative to the v-Drive root.
pub fn read_file(path: &Path, relative_path: &str, role: FileRole) -> CoreResult<ConfigFile> {
    let bytes = fs::read(path).map_err(|source| CoreError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_bytes(&bytes, relative_path, role).map_err(|e| match e {
        CoreError::NotUtf8 { .. } => CoreError::NotUtf8 {
            path: path.to_path_buf(),
        },
        other => other,
    })
}

/// Parse raw bytes into a [`ConfigFile`].
pub fn parse_bytes(bytes: &[u8], relative_path: &str, role: FileRole) -> CoreResult<ConfigFile> {
    let text = std::str::from_utf8(bytes).map_err(|_| CoreError::NotUtf8 {
        path: Path::new(relative_path).to_path_buf(),
    })?;

    let (bom, body) = match text.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, text),
    };

    let profile = match role {
        FileRole::ProfileLayout | FileRole::BackupLayout => {
            ProfileId::parse_layout_file_name(file_name(relative_path))
        }
        FileRole::ProfileLighting => ProfileId::parse_lighting_file_name(file_name(relative_path)),
        _ => None,
    };

    let lines = split_lines(body)
        .into_iter()
        .scan(None::<LayerId>, |current, (raw, eol)| {
            let line = parse_line(raw, eol, *current);
            if let LineKind::LayerHeader(l) = line.kind {
                *current = Some(l);
            }
            Some(line)
        })
        .collect();

    Ok(ConfigFile {
        relative_path: relative_path.to_string(),
        role,
        profile,
        bom,
        lines,
        dirty: false,
    })
}

fn file_name(relative_path: &str) -> &str {
    relative_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(relative_path)
}

/// Split text into `(raw line without terminator, terminator)` pairs, preserving CRLF, LF and a
/// missing final terminator exactly.
pub fn split_lines(text: &str) -> Vec<(String, Eol)> {
    let mut out = Vec::new();
    if text.is_empty() {
        return out;
    }
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    out.push((text[start..i].to_string(), Eol::CrLf));
                    i += 2;
                } else {
                    // A lone CR: treat it as a terminator so nothing is lost.
                    out.push((text[start..i].to_string(), Eol::CrLf));
                    i += 1;
                }
                start = i;
            }
            b'\n' => {
                out.push((text[start..i].to_string(), Eol::Lf));
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        out.push((text[start..].to_string(), Eol::None));
    }
    out
}

/// Parse one line, given the layer that is currently active.
pub fn parse_line(raw: String, eol: Eol, current_layer: Option<LayerId>) -> Line {
    let trimmed = raw.trim();
    let disabled = trimmed.starts_with('*');
    let content = if disabled {
        trimmed.strip_prefix('*').unwrap_or(trimmed).trim()
    } else {
        trimmed
    };

    let kind = classify(content);
    let layer = match kind {
        LineKind::LayerHeader(l) => Some(l),
        _ => current_layer,
    };

    Line {
        raw,
        eol,
        kind,
        enabled: !disabled,
        layer,
        origin: Origin::Original,
    }
}

fn classify(content: &str) -> LineKind {
    if content.is_empty() {
        return LineKind::Blank;
    }
    if let Some(l) = LayerId::parse(content) {
        // A bare word such as "base" is accepted as a header too, matching token case-insensitivity.
        return LineKind::LayerHeader(l);
    }
    if content.starts_with('#') {
        return LineKind::Comment {
            style: CommentStyle::Hash,
        };
    }
    if content.starts_with("//") {
        return LineKind::Comment {
            style: CommentStyle::SlashSlash,
        };
    }

    let first = content.chars().next().unwrap_or(' ');
    // A line that starts with '>' is documented syntax with a missing position, not an unknown
    // instruction, so it must be reported as malformed rather than quietly tolerated.
    if first != '[' && first != '{' && first != '>' {
        // Not documented syntax. Undocumented instructions must survive untouched, so this is a
        // warning, not an error.
        return LineKind::Unknown {
            reason: "does not start with a documented layer header, '[' or '{'".into(),
        };
    }

    match scan_groups(content) {
        Err(e) => LineKind::Invalid { error: e },
        Ok(items) => build_kind(content, items),
    }
}

/// A bracketed group produced by [`scan_groups`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Group {
    open: char,
    inner: String,
    column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Group(Group),
    Sep(usize),
}

/// Split a line into bracket groups and `>` separators, rejecting stray characters.
fn scan_groups(content: &str) -> Result<Vec<Item>, ParseError> {
    let chars: Vec<(usize, char)> = content.char_indices().collect();
    let mut items = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let (byte_idx, c) = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '>' => {
                items.push(Item::Sep(byte_idx));
                i += 1;
            }
            '[' | '{' => {
                let close = if c == '[' { ']' } else { '}' };
                let mut j = i + 1;
                let mut inner = String::new();
                let mut closed = false;
                while j < chars.len() {
                    let (_, cj) = chars[j];
                    if cj == close {
                        closed = true;
                        break;
                    }
                    if cj == '[' || cj == '{' || cj == ']' || cj == '}' {
                        return Err(ParseError::new(
                            SyntaxCode::MixedBrackets,
                            Some(chars[j].0),
                            format!("'{close}'"),
                            format!("'{cj}'"),
                        ));
                    }
                    inner.push(cj);
                    j += 1;
                }
                if !closed {
                    return Err(ParseError::new(
                        SyntaxCode::UnterminatedGroup,
                        Some(byte_idx),
                        format!("a closing '{close}'"),
                        "the end of the line",
                    ));
                }
                if inner.trim().is_empty() {
                    return Err(ParseError::new(
                        SyntaxCode::EmptyGroup,
                        Some(byte_idx),
                        "a token between the brackets",
                        "an empty group",
                    ));
                }
                items.push(Item::Group(Group {
                    open: c,
                    inner: inner.trim().to_string(),
                    column: byte_idx,
                }));
                i = j + 1;
            }
            other => {
                return Err(ParseError::new(
                    SyntaxCode::StrayCharacter,
                    Some(byte_idx),
                    "'[' or '{'",
                    format!("'{other}'"),
                ))
            }
        }
    }
    Ok(items)
}

fn build_kind(content: &str, items: Vec<Item>) -> LineKind {
    let groups: Vec<&Group> = items
        .iter()
        .filter_map(|i| match i {
            Item::Group(g) => Some(g),
            _ => None,
        })
        .collect();
    let seps: Vec<usize> = items
        .iter()
        .filter_map(|i| match i {
            Item::Sep(c) => Some(*c),
            _ => None,
        })
        .collect();

    if seps.is_empty() {
        return LineKind::Invalid {
            error: ParseError::new(
                SyntaxCode::MissingSeparator,
                None,
                "the '>' separator between the position and its action",
                content,
            ),
        };
    }
    if seps.len() > 1 {
        return LineKind::Invalid {
            error: ParseError::new(
                SyntaxCode::RepeatedSeparator,
                Some(seps[1]),
                "exactly one '>' separator",
                format!("{} separators", seps.len()),
            ),
        };
    }

    let open = groups.first().map(|g| g.open).unwrap_or('[');
    if groups.iter().any(|g| g.open != open) {
        return LineKind::Invalid {
            error: ParseError::new(
                SyntaxCode::MixedBrackets,
                None,
                "either square brackets or curly braces, not both",
                "a line mixing '[' and '{'",
            ),
        };
    }

    // Exactly one group to the left of '>'.
    let sep_pos = items
        .iter()
        .position(|i| matches!(i, Item::Sep(_)))
        .unwrap();
    let left: Vec<&Group> = items[..sep_pos]
        .iter()
        .filter_map(|i| match i {
            Item::Group(g) => Some(g),
            _ => None,
        })
        .collect();
    let right: Vec<&Group> = items[sep_pos + 1..]
        .iter()
        .filter_map(|i| match i {
            Item::Group(g) => Some(g),
            _ => None,
        })
        .collect();

    if left.is_empty() {
        return LineKind::Invalid {
            error: ParseError::new(
                SyntaxCode::ExpectedToken,
                None,
                "a position or trigger token before '>'",
                "nothing",
            ),
        };
    }
    if right.is_empty() {
        return LineKind::Invalid {
            error: ParseError::new(
                SyntaxCode::ExpectedToken,
                None,
                "at least one action token after '>'",
                "nothing",
            ),
        };
    }

    match open {
        '[' => {
            if left.len() > 1 {
                return LineKind::Invalid {
                    error: ParseError::new(
                        SyntaxCode::ExpectedToken,
                        Some(left[1].column),
                        "exactly one position token before '>'",
                        format!("{} tokens", left.len()),
                    ),
                };
            }
            let position = left[0].inner.clone();
            if crate::model::Indicator::parse(&position).is_some() {
                build_led(position, right)
            } else {
                build_remap(position, right)
            }
        }
        '{' => build_macro(left, right),
        _ => LineKind::Unknown {
            reason: "unrecognised group kind".into(),
        },
    }
}

fn build_remap(position: String, right: Vec<&Group>) -> LineKind {
    let mut atoms = Vec::new();
    for g in right {
        let inner = g.inner.as_str();
        if let Some(rest) = strip_prefix_ci(inner, "t&h") {
            let ms: u16 = match rest.trim().parse() {
                Ok(v) if (1..=999).contains(&v) => v,
                _ => {
                    return LineKind::Invalid {
                        error: ParseError::new(
                            SyntaxCode::BadTapHold,
                            Some(g.column),
                            "a Tap-and-Hold delay between 1 and 999 milliseconds, e.g. t&h250",
                            format!("t&h{rest}"),
                        ),
                    }
                }
            };
            atoms.push(RemapAtom::TapHold(ms));
        } else {
            atoms.push(RemapAtom::Action(inner.to_string()));
        }
    }
    LineKind::Remap(RemapLine { position, atoms })
}

fn build_led(indicator_token: String, right: Vec<&Group>) -> LineKind {
    let first = right[0];
    let Some(function) = LedFunction::parse(&first.inner) else {
        return LineKind::Invalid {
            error: ParseError::new(
                SyntaxCode::ExpectedToken,
                Some(first.column),
                "a documented LED function (null, prof, caps, nmlk, sclk, layd, layk, lay1, lay2, lay)",
                format!("[{}]", first.inner),
            ),
        };
    };
    let rest = &right[1..];
    if !rest.len().is_multiple_of(3) {
        return LineKind::Invalid {
            error: ParseError::new(
                SyntaxCode::BadColorTriple,
                rest.first().map(|g| g.column),
                "colours as whole RRR/GGG/BBB triples",
                format!("{} colour groups", rest.len()),
            ),
        };
    }
    let mut colors = Vec::new();
    for triple in rest.chunks(3) {
        let mut chan = [0u8; 3];
        for (idx, g) in triple.iter().enumerate() {
            match g.inner.trim().parse::<u16>() {
                Ok(v) if v <= 255 => chan[idx] = v as u8,
                _ => {
                    return LineKind::Invalid {
                        error: ParseError::new(
                            SyntaxCode::BadNumber,
                            Some(g.column),
                            "a colour channel between 000 and 255",
                            format!("[{}]", g.inner),
                        ),
                    }
                }
            }
        }
        colors.push(Rgb {
            r: chan[0],
            g: chan[1],
            b: chan[2],
        });
    }
    let indicator = match crate::model::Indicator::parse(&indicator_token) {
        Some(i) => i,
        None => {
            return LineKind::Invalid {
                error: ParseError::new(
                    SyntaxCode::ExpectedToken,
                    None,
                    "an indicator between IND1 and IND6",
                    indicator_token,
                ),
            }
        }
    };
    LineKind::Led(LedLine {
        indicator,
        function,
        colors,
    })
}

fn build_macro(left: Vec<&Group>, right: Vec<&Group>) -> LineKind {
    // The guide says a macro is triggered by a key *action* and that up to four modifiers may act
    // as co-triggers. Kinesis's own examples put the modifiers first (`{lctr}{hk4}>{s9}...`) and,
    // in example 3, use a position token for a key that is blank by default (`{hk1}>{x3}...`).
    // The left side is therefore read positionally: the first token that is not a known modifier
    // is the trigger, and every other token is a co-trigger. The documented limits are then
    // applied by validation, so nothing is rejected by the parser for being unfamiliar.
    let trigger_index = left
        .iter()
        .position(|g| !tokens::is_modifier(&g.inner))
        .unwrap_or_else(|| left.len().saturating_sub(1));
    let trigger = left[trigger_index].inner.clone();
    let modifiers: Vec<String> = left
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != trigger_index)
        .map(|(_, g)| g.inner.clone())
        .collect();
    let mut body = Vec::new();
    for g in right {
        match classify_macro_atom(&g.inner, g.column) {
            Ok(atom) => body.push(atom),
            Err(error) => return LineKind::Invalid { error },
        }
    }
    LineKind::Macro(MacroLine {
        trigger,
        modifiers,
        body,
    })
}

/// Classify one macro body group.
///
/// Numeric prefixes (`sN`, `xN`, `dNNN`) are recognised structurally: when the shape matches but
/// the value is out of the documented range this is reported as an error rather than silently
/// kept as an unknown token. Anything else that is not a documented prefix becomes
/// [`MacroAtom::Unknown`] and is preserved verbatim.
fn classify_macro_atom(inner: &str, column: usize) -> Result<MacroAtom, ParseError> {
    let trimmed = inner.trim();
    let lower = trimmed.to_ascii_lowercase();

    if lower == "dran" {
        return Ok(MacroAtom::RandomDelay);
    }

    for (prefix, code, what) in [
        (
            's',
            SyntaxCode::BadSpeed,
            "a macro speed prefix between s1 and s9",
        ),
        (
            'x',
            SyntaxCode::BadMultiplay,
            "a macro multiplay prefix between x1 and x9",
        ),
    ] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                let n: u8 = rest.parse().map_err(|_| {
                    ParseError::new(code, Some(column), what, format!("{{{trimmed}}}"))
                })?;
                if !(1..=9).contains(&n) {
                    return Err(ParseError::new(
                        code,
                        Some(column),
                        what,
                        format!("{{{trimmed}}}"),
                    ));
                }
                return Ok(if prefix == 's' {
                    MacroAtom::Speed(n)
                } else {
                    MacroAtom::Multiplay(n)
                });
            }
        }
    }

    if let Some(rest) = lower.strip_prefix('d') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            let ms: u16 = rest.parse().map_err(|_| {
                ParseError::new(
                    SyntaxCode::BadNumber,
                    Some(column),
                    "a delay between d001 and d999, or dran",
                    format!("{{{trimmed}}}"),
                )
            })?;
            if !(1..=999).contains(&ms) {
                return Err(ParseError::new(
                    SyntaxCode::BadNumber,
                    Some(column),
                    "a delay between d001 and d999, or dran",
                    format!("{{{trimmed}}}"),
                ));
            }
            return Ok(MacroAtom::Delay(ms));
        }
    }

    if let Some(rest) = trimmed.strip_prefix('-') {
        if !rest.is_empty() {
            return Ok(MacroAtom::Action {
                stroke: Stroke::Down,
                token: rest.to_string(),
            });
        }
    }
    if let Some(rest) = trimmed.strip_prefix('+') {
        if !rest.is_empty() {
            return Ok(MacroAtom::Action {
                stroke: Stroke::Up,
                token: rest.to_string(),
            });
        }
    }

    // A plain group is an action press. Whether the token is *documented* is decided by
    // validation, not here, so nothing is dropped for being unfamiliar.
    Ok(MacroAtom::Action {
        stroke: Stroke::Press,
        token: trimmed.to_string(),
    })
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_preserves_every_terminator() {
        assert_eq!(split_lines(""), Vec::<(String, Eol)>::new());
        assert_eq!(split_lines("a"), vec![("a".to_string(), Eol::None)]);
        assert_eq!(split_lines("a\n"), vec![("a".to_string(), Eol::Lf)]);
        assert_eq!(
            split_lines("a\r\nb\nc"),
            vec![
                ("a".to_string(), Eol::CrLf),
                ("b".to_string(), Eol::Lf),
                ("c".to_string(), Eol::None),
            ]
        );
        // A blank final line is preserved as an empty line with a terminator.
        assert_eq!(
            split_lines("a\n\n"),
            vec![("a".to_string(), Eol::Lf), ("".to_string(), Eol::Lf)]
        );
    }

    fn kind_of(text: &str) -> LineKind {
        parse_line(text.to_string(), Eol::Lf, Some(LayerId::Base)).kind
    }

    #[test]
    fn parses_the_documented_remap_examples() {
        assert_eq!(
            kind_of("[hk1]>[q]"),
            LineKind::Remap(RemapLine::single("hk1", "q"))
        );
        assert_eq!(
            kind_of("[esc]>[caps]"),
            LineKind::Remap(RemapLine::single("esc", "caps"))
        );
        assert_eq!(
            kind_of("[caps]>[caps][t&h500][esc]"),
            LineKind::Remap(RemapLine::tap_and_hold("caps", "caps", 500, "esc"))
        );
    }

    #[test]
    fn parses_the_documented_macro_examples() {
        let k = kind_of("{tab}>{-lshf}{h}{+lshf}{i}");
        let LineKind::Macro(m) = k else {
            panic!("expected macro, got {k:?}")
        };
        assert_eq!(m.trigger, "tab");
        assert!(m.modifiers.is_empty());
        assert_eq!(m.action_count(), 4);
        assert_eq!(
            m.body[0],
            MacroAtom::Action {
                stroke: Stroke::Down,
                token: "lshf".into()
            }
        );

        let LineKind::Macro(m) = kind_of("{lctr}{hk4}>{s9}{q}{w}{e}{r}{t}{y}") else {
            panic!("expected macro")
        };
        assert_eq!(m.trigger, "hk4");
        assert_eq!(m.modifiers, vec!["lctr"]);
        assert_eq!(m.body[0], MacroAtom::Speed(9));
        assert_eq!(m.action_count(), 6);

        let LineKind::Macro(m) = kind_of("{hk1}>{x3}{vol+}") else {
            panic!("expected macro")
        };
        assert_eq!(m.body[0], MacroAtom::Multiplay(3));
        assert_eq!(
            m.body[1],
            MacroAtom::Action {
                stroke: Stroke::Press,
                token: "vol+".into()
            }
        );
    }

    #[test]
    fn parses_delays_including_random() {
        let LineKind::Macro(m) = kind_of("{tab}>{d001}{d999}{dran}") else {
            panic!()
        };
        assert_eq!(m.body[0], MacroAtom::Delay(1));
        assert_eq!(m.body[1], MacroAtom::Delay(999));
        assert_eq!(m.body[2], MacroAtom::RandomDelay);
        assert_eq!(m.action_count(), 0);
    }

    #[test]
    fn parses_a_led_line() {
        let k = kind_of("[IND1]>[caps][255][000][000]");
        let LineKind::Led(l) = k else {
            panic!("expected led, got {k:?}")
        };
        assert_eq!(l.indicator, crate::model::Indicator(1));
        assert_eq!(l.function, LedFunction::CapsLock);
        assert_eq!(l.colors, vec![Rgb { r: 255, g: 0, b: 0 }]);
        assert_eq!(l.render(), "[IND1]>[caps][255][000][000]");
    }

    #[test]
    fn layer_headers_are_recognised_and_tracked() {
        let lines = [
            "<base>",
            "[esc]>[caps]",
            "<keypad>",
            "[spc]>[kp0]",
            "<function1>",
            "<function2>",
            "<function3>",
        ];
        let mut current = None;
        let mut parsed = Vec::new();
        for t in lines {
            let l = parse_line(t.to_string(), Eol::Lf, current);
            if let LineKind::LayerHeader(ly) = l.kind {
                current = Some(ly);
            }
            parsed.push(l);
        }
        assert_eq!(parsed[1].layer, Some(LayerId::Base));
        assert_eq!(parsed[3].layer, Some(LayerId::Keypad));
        assert_eq!(parsed[5].layer, Some(LayerId::Function2));
        assert_eq!(parsed[6].layer, Some(LayerId::Function3));
    }

    #[test]
    fn a_disabled_line_is_parsed_but_marked_off() {
        let l = parse_line("*[q]>[w]".to_string(), Eol::Lf, Some(LayerId::Base));
        assert!(!l.enabled);
        assert_eq!(l.kind, LineKind::Remap(RemapLine::single("q", "w")));
    }

    #[test]
    fn undocumented_lines_are_preserved_as_unknown_not_dropped() {
        let l = parse_line(
            "some vendor extension".to_string(),
            Eol::Lf,
            Some(LayerId::Base),
        );
        assert!(matches!(l.kind, LineKind::Unknown { .. }));
        assert_eq!(l.raw, "some vendor extension");
        assert_eq!(l.to_file_text(), "some vendor extension\n");
    }

    #[test]
    fn comments_are_recognised_but_kept_verbatim() {
        assert!(matches!(
            kind_of("# a note"),
            LineKind::Comment {
                style: CommentStyle::Hash
            }
        ));
        assert!(matches!(
            kind_of("// another"),
            LineKind::Comment {
                style: CommentStyle::SlashSlash
            }
        ));
    }

    #[test]
    fn malformed_documented_syntax_is_invalid_and_blocks() {
        for bad in [
            "[q]>[",                         // unterminated
            "[q][w]",                        // missing separator
            "[q]>[w]>[e]",                   // repeated separator
            "[q]>",                          // no action
            "> [q]",                         // no position
            "[q]>[t&h0]",                    // out-of-range tap and hold
            "[q]>[t&habc]",                  // malformed tap and hold
            "[q]>[w]:[e]",                   // stray character
            "[q}>{w}",                       // mixed brackets
            "[IND1]>[caps][1][2]",           // incomplete colour triple
            "[IND1]>[caps][999][000][000]",  // channel out of range
            "[IND1]>[bogus][000][000][000]", // unknown LED function
            "{q}>{s0}",                      // speed below range
            "{q}>{s10}",                     // speed above range
            "{q}>{x0}",                      // multiplay below range
            "{q}>{d0}",                      // delay below range
            "{q}>{d1000}",                   // delay above range
        ] {
            let k = kind_of(bad);
            assert!(matches!(k, LineKind::Invalid { .. }), "{bad} -> {k:?}");
        }
    }

    #[test]
    fn in_range_numeric_prefixes_are_accepted() {
        let LineKind::Macro(m) = kind_of("{q}>{s1}{x9}{d999}{dran}") else {
            panic!()
        };
        assert_eq!(m.body[0], MacroAtom::Speed(1));
        assert_eq!(m.body[1], MacroAtom::Multiplay(9));
        assert_eq!(m.body[2], MacroAtom::Delay(999));
        assert_eq!(m.body[3], MacroAtom::RandomDelay);
    }

    #[test]
    fn unrecognised_macro_tokens_are_preserved_verbatim() {
        let LineKind::Macro(m) = kind_of("{q}>{c1}{zzz}") else {
            panic!()
        };
        // They are kept as presses so rendering is exact; validation is what flags them.
        assert_eq!(
            m.body[0],
            MacroAtom::Action {
                stroke: Stroke::Press,
                token: "c1".into()
            }
        );
        assert_eq!(m.render(), "{q}>{c1}{zzz}");
        assert_eq!(m.action_count(), 2);
    }

    #[test]
    fn every_documented_example_round_trips_through_render() {
        for line in [
            "[hk1]>[q]",
            "[esc]>[caps]",
            "[caps]>[caps][t&h500][esc]",
            "{tab}>{-lshf}{h}{+lshf}{i}",
            "{lctr}{hk4}>{s9}{q}{w}{e}{r}{t}{y}",
            "{hk1}>{x3}{vol+}",
        ] {
            let parsed = parse_line(line.to_string(), Eol::Lf, Some(LayerId::Base));
            assert!(
                !matches!(parsed.kind, LineKind::Invalid { .. }),
                "the guide's own example failed to parse: {line}"
            );
            let rendered = match &parsed.kind {
                LineKind::Remap(r) => r.render(),
                LineKind::Macro(m) => m.render(),
                other => panic!("{line} parsed as {other:?}"),
            };
            assert_eq!(rendered, line);
        }
    }

    #[test]
    fn blank_and_whitespace_lines_are_blank() {
        assert_eq!(kind_of(""), LineKind::Blank);
        assert_eq!(kind_of("   "), LineKind::Blank);
        assert_eq!(kind_of("\t"), LineKind::Blank);
    }

    #[test]
    fn a_bom_crlf_file_round_trips_byte_for_byte() {
        let original = "\u{feff}<base>\r\n[q]>[w]\r\n# note\r\n\r\n<keypad>\r\n";
        let f = parse_bytes(
            original.as_bytes(),
            "layouts/layout1.txt",
            FileRole::ProfileLayout,
        )
        .unwrap();
        assert!(f.bom);
        assert!(f.has_crlf());
        assert!(!f.has_mixed_eol());
        assert!(!f.lacks_final_newline());
        assert_eq!(f.render(), original);
        assert_eq!(f.profile, Some(ProfileId(1)));
    }

    #[test]
    fn a_file_without_a_final_newline_round_trips() {
        let original = "<base>\n[q]>[w]";
        let f = parse_bytes(
            original.as_bytes(),
            "layouts/layout2.txt",
            FileRole::ProfileLayout,
        )
        .unwrap();
        assert!(f.lacks_final_newline());
        assert_eq!(f.render(), original);
    }

    #[test]
    fn mixed_endings_are_reported_and_preserved() {
        let original = "a\nb\r\nc";
        let f = parse_bytes(
            original.as_bytes(),
            "layouts/layout3.txt",
            FileRole::ProfileLayout,
        )
        .unwrap();
        assert!(f.has_mixed_eol());
        assert!(f.lacks_final_newline());
        assert_eq!(f.render(), original);
    }

    #[test]
    fn non_utf8_is_refused_rather_than_mangled() {
        let bytes = [0x3c, 0x62, 0x61, 0x73, 0x65, 0x3e, 0xff, 0xfe];
        let err = parse_bytes(&bytes, "layouts/layout1.txt", FileRole::ProfileLayout).unwrap_err();
        assert_eq!(err.code(), "not_utf8");
    }

    #[test]
    fn role_and_profile_are_derived_from_the_name() {
        let f = parse_bytes(b"", "lighting/led7.txt", FileRole::ProfileLighting).unwrap();
        assert_eq!(f.profile, Some(ProfileId(7)));
        assert_eq!(f.role, FileRole::ProfileLighting);
        assert!(f.lines.is_empty());

        let f = parse_bytes(b"", "layouts/mybackup.txt", FileRole::BackupLayout).unwrap();
        assert_eq!(f.profile, None);
        assert!(!f.role.is_writable());
    }

    #[test]
    fn empty_file_yields_no_lines_and_renders_empty() {
        let f = parse_bytes(b"", "layouts/layout1.txt", FileRole::ProfileLayout).unwrap();
        assert!(f.lines.is_empty());
        assert_eq!(f.render(), "");
    }

    #[test]
    fn the_expected_empty_layout_shape_is_preserved() {
        // Guide §4.0: an unmodified layout file is "empty aside from the layer headings".
        let original = "<base>\n<keypad>\n<function1>\n<function2>\n<function3>\n";
        let f = parse_bytes(
            original.as_bytes(),
            "layouts/layout1.txt",
            FileRole::ProfileLayout,
        )
        .unwrap();
        assert_eq!(f.lines.len(), 5);
        assert!(f
            .lines
            .iter()
            .all(|l| matches!(l.kind, LineKind::LayerHeader(_))));
        assert_eq!(f.render(), original);
    }
}
