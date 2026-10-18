//! Keyboard layout detection and the token -> character preview.
//!
//! The operator's real problem is a two-step translation that is easy to get wrong:
//!
//! 1. the keyboard sends a **USB HID usage** (the action token, e.g. `obrk` = usage 0x2F);
//! 2. the Linux XKB layout turns that usage into a **character** (`[` on `us`, something else on
//!    a Latin American layout).
//!
//! This module always shows both, and it refuses to guess the second step: only layouts whose
//! mapping is actually encoded here are reported as confident. Everything else is marked
//! uncertain, with the reason, instead of showing a plausible-looking wrong symbol.
//!
//! The Direct Programming Guide also has a hard requirement in §3.3: "Your computer must be
//! configured for the English (US) keyboard layout. Other language drivers use different
//! codes/positions for certain keys which are critical for programming characters such as [],
//! {} and >." That warning is surfaced to the operator verbatim.

use std::collections::BTreeMap;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::tokens;

/// Where the layout information came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutSource {
    /// Read from the running Hyprland instance with `hyprctl`.
    Hyprland,
    /// Supplied by the operator on the command line or in the interface.
    Manual,
    /// Read from an environment variable.
    Environment,
    /// Nothing was available; the tool will not claim to know the layout.
    Unknown,
}

impl LayoutSource {
    pub fn label(self) -> &'static str {
        match self {
            LayoutSource::Hyprland => "detected from Hyprland",
            LayoutSource::Manual => "set manually",
            LayoutSource::Environment => "from the environment",
            LayoutSource::Unknown => "unknown",
        }
    }
}

/// The keyboard layout in effect for the machine, as far as it can be determined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardLayout {
    /// XKB layout name, e.g. `us`.
    pub layout: String,
    /// XKB variant, when one is set.
    pub variant: Option<String>,
    pub source: LayoutSource,
    /// How it was obtained, or why it could not be.
    pub detail: String,
    /// The device it was read from, when Hyprland reported per-device layouts.
    pub device: Option<String>,
}

impl KeyboardLayout {
    /// A layout the operator set by hand.
    pub fn manual(layout: impl Into<String>, variant: Option<String>) -> Self {
        KeyboardLayout {
            layout: layout.into(),
            variant,
            source: LayoutSource::Manual,
            detail: "supplied by the operator".to_string(),
            device: None,
        }
    }

    /// The value Hyprland shows for `input:kb_layout`, e.g. `us` or `us,es`.
    pub fn layouts(&self) -> Vec<String> {
        self.layout
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// True when the first (active) layout is the one the guide requires.
    pub fn is_us(&self) -> bool {
        self.layouts().first().is_some_and(|l| l == "us")
    }

    /// True when a variant is set that changes the produced characters.
    pub fn has_variant(&self) -> bool {
        self.variant
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
    }

    /// True when this module can translate tokens to characters with confidence.
    pub fn preview_is_reliable(&self) -> bool {
        self.is_us() && !self.has_variant()
    }

    /// A warning to show next to any character preview, or `None` when the preview is reliable.
    pub fn reliability_warning(&self) -> Option<String> {
        if self.preview_is_reliable() {
            return None;
        }
        if self.source == LayoutSource::Unknown {
            return Some(
                "The keyboard layout could not be detected, so characters are not shown. Tokens \
                 are still exact: they are what the keyboard sends."
                    .to_string(),
            );
        }
        if !self.is_us() {
            return Some(format!(
                "The active layout is '{}' and this tool only encodes the character mapping for \
                 'us'. Characters are marked uncertain. The Direct Programming Guide §3.3 states \
                 the computer must be configured for the English (US) layout because other \
                 language drivers use different codes for keys such as [], {{}} and >.",
                self.layout
            ));
        }
        Some(format!(
            "The layout 'us' has the variant '{}' set, which changes some characters, so the \
             preview is marked uncertain.",
            self.variant.as_deref().unwrap_or("")
        ))
    }
}

/// Detect the active keyboard layout.
///
/// Tries, in order: `hyprctl devices -j` (per-device layout), `hyprctl getoption input:kb_layout
/// -j`, then the environment. Nothing is inferred when all of them fail.
pub fn detect() -> KeyboardLayout {
    if let Some(l) = from_hyprctl_devices() {
        return l;
    }
    if let Some(l) = from_hyprctl_getoption() {
        return l;
    }
    if let Some(l) = from_environment() {
        return l;
    }
    KeyboardLayout {
        layout: "unknown".to_string(),
        variant: None,
        source: LayoutSource::Unknown,
        detail: "hyprctl was unavailable or reported no keyboard, and no XKB environment variable \
                 is set"
            .to_string(),
        device: None,
    }
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("hyprctl");
    cmd.args(args);
    // Hyprland exports HYPRLAND_INSTANCE_SIGNATURE only inside its own session, so a tool started
    // from a tmux pane, a systemd unit or an SSH connection will not inherit it. The socket
    // directory is readable, so the newest instance is used instead of giving up.
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        if let Some(signature) = discover_hyprland_instance() {
            cmd.env("HYPRLAND_INSTANCE_SIGNATURE", signature);
        }
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Find the most recently created Hyprland instance directory.
fn discover_hyprland_instance() -> Option<String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let dir = std::path::PathBuf::from(runtime).join("hypr");
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_dir() {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if best.as_ref().is_none_or(|(t, _)| modified > *t) {
            best = Some((modified, name));
        }
    }
    best.map(|(_, name)| name)
}

/// Node names Hyprland reports for things that are not real hardware keyboards.
const VIRTUAL_NODE_HINTS: &[&str] = &[
    "virtual",
    "passthrough",
    "consumer-control",
    "system-control",
    "power-button",
    "video-bus",
];

fn from_hyprctl_devices() -> Option<KeyboardLayout> {
    let text = hyprctl(&["devices", "-j"])?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let keyboards = value.get("keyboards")?.as_array()?;
    let mut best: Option<(u8, KeyboardLayout)> = None;
    for kb in keyboards {
        let layout = kb
            .get("layout")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if layout.trim().is_empty() {
            continue;
        }
        let variant = kb
            .get("variant")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());
        let name = kb
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("keyboard")
            .to_string();
        // Prefer a real hardware keyboard: Hyprland also lists consumer-control, system-control,
        // passthrough and power-button nodes, which all report the same layout.
        let lowered = name.to_ascii_lowercase();
        let score = if VIRTUAL_NODE_HINTS.iter().any(|h| lowered.contains(h)) {
            1
        } else {
            2
        };
        let detail = format!(
            "hyprctl devices reported layout '{layout}' for '{name}'{}",
            kb.get("active_keymap")
                .and_then(|v| v.as_str())
                .map(|k| format!(" (active keymap: {k})"))
                .unwrap_or_default()
        );
        let candidate = KeyboardLayout {
            layout,
            variant,
            source: LayoutSource::Hyprland,
            detail,
            device: Some(name),
        };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, candidate));
        }
        if score == 2 {
            // A real hardware keyboard was found; stop looking.
            break;
        }
    }
    best.map(|(_, l)| l)
}

fn from_hyprctl_getoption() -> Option<KeyboardLayout> {
    let text = hyprctl(&["getoption", "input:kb_layout", "-j"])?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let layout = value
        .get("str")
        .and_then(|v| v.as_str())?
        .trim()
        .to_string();
    if layout.is_empty() {
        return None;
    }
    let variant = hyprctl(&["getoption", "input:kb_variant", "-j"])
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("str").and_then(|s| s.as_str()).map(|s| s.to_string()))
        .filter(|s| !s.trim().is_empty());
    Some(KeyboardLayout {
        layout,
        variant,
        source: LayoutSource::Hyprland,
        detail: "read from the Hyprland option input:kb_layout".to_string(),
        device: None,
    })
}

fn from_environment() -> Option<KeyboardLayout> {
    for key in ["XKB_DEFAULT_LAYOUT", "XKBLAYOUT", "HYPRLAND_KBD_LAYOUT"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                let variant = std::env::var("XKB_DEFAULT_VARIANT")
                    .ok()
                    .filter(|s| !s.trim().is_empty());
                return Some(KeyboardLayout {
                    layout: v.trim().to_ascii_lowercase(),
                    variant,
                    source: LayoutSource::Environment,
                    detail: format!("read from ${key}"),
                    device: None,
                });
            }
        }
    }
    None
}

/// Layouts whose character mapping this build actually encodes.
pub fn supported_layouts() -> &'static [&'static str] {
    &["us"]
}

/// A preview of what an action token does, split into the token (what the keyboard sends) and the
/// character (what Linux produces).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharPreview {
    /// The action token, exactly as it appears in the configuration file.
    pub token: String,
    /// The layout-independent USB HID description from the official token list.
    pub usb_label: String,
    /// The character Linux produces for this token on the active layout, when known.
    pub character: Option<char>,
    /// The character produced with Shift held, when known.
    pub shifted_character: Option<char>,
    /// `false` whenever [`character`] was not computed from an encoded mapping.
    pub confident: bool,
    /// Why the character is unknown or uncertain.
    pub note: Option<String>,
}

/// (unshifted, shifted) characters for the `us` layout, keyed by action token.
fn us_table() -> &'static [(&'static str, char, char)] {
    &[
        ("a", 'a', 'A'),
        ("b", 'b', 'B'),
        ("c", 'c', 'C'),
        ("d", 'd', 'D'),
        ("e", 'e', 'E'),
        ("f", 'f', 'F'),
        ("g", 'g', 'G'),
        ("h", 'h', 'H'),
        ("i", 'i', 'I'),
        ("j", 'j', 'J'),
        ("k", 'k', 'K'),
        ("l", 'l', 'L'),
        ("m", 'm', 'M'),
        ("n", 'n', 'N'),
        ("o", 'o', 'O'),
        ("p", 'p', 'P'),
        ("q", 'q', 'Q'),
        ("r", 'r', 'R'),
        ("s", 's', 'S'),
        ("t", 't', 'T'),
        ("u", 'u', 'U'),
        ("v", 'v', 'V'),
        ("w", 'w', 'W'),
        ("x", 'x', 'X'),
        ("y", 'y', 'Y'),
        ("z", 'z', 'Z'),
        ("1", '1', '!'),
        ("2", '2', '@'),
        ("3", '3', '#'),
        ("4", '4', '$'),
        ("5", '5', '%'),
        ("6", '6', '^'),
        ("7", '7', '&'),
        ("8", '8', '*'),
        ("9", '9', '('),
        ("0", '0', ')'),
        ("hyph", '-', '_'),
        ("eql", '=', '+'),
        ("obrk", '[', '{'),
        ("cbrk", ']', '}'),
        ("bsls", '\\', '|'),
        ("scol", ';', ':'),
        ("apos", '\'', '"'),
        ("grav", '`', '~'),
        ("comm", ',', '<'),
        ("perd", '.', '>'),
        ("fsls", '/', '?'),
        ("spc", ' ', ' '),
        ("tab", '\t', '\t'),
        ("ent", '\n', '\n'),
    ]
}

/// Look up the `us` characters for a token.
pub fn us_characters(token: &str) -> Option<(char, char)> {
    let lower = token.trim().to_ascii_lowercase();
    us_table()
        .iter()
        .find(|(t, _, _)| *t == lower)
        .map(|(_, a, b)| (*a, *b))
}

/// Build the preview for one action token under a layout.
pub fn preview(token: &str, layout: &KeyboardLayout) -> CharPreview {
    let usb_label = match tokens::lookup(token) {
        Some(t) => t.name.to_string(),
        None => format!("'{token}' is not in the official token list"),
    };
    let mut preview = CharPreview {
        token: token.to_string(),
        usb_label,
        character: None,
        shifted_character: None,
        confident: false,
        note: None,
    };

    let Some((plain, shifted)) = us_characters(token) else {
        preview.note = Some(
            "Not a printable key on a US layout (a control, modifier, media, mouse or special \
             action): it has no character of its own."
                .to_string(),
        );
        return preview;
    };

    if layout.preview_is_reliable() {
        preview.character = Some(plain);
        preview.shifted_character = Some(shifted);
        preview.confident = true;
        return preview;
    }

    // Never present a plausible-looking wrong character.
    preview.note = layout.reliability_warning();
    preview
}

/// Preview a whole configuration line's effect as characters (used for macros and remaps).
pub fn preview_line(tokens_in_order: &[String], layout: &KeyboardLayout) -> Vec<CharPreview> {
    tokens_in_order.iter().map(|t| preview(t, layout)).collect()
}

/// A compact, human-readable rendering of an action token for either interface: the token, its
/// USB description, and the produced character when it can be computed reliably.
pub fn describe_with_character(token: &str, layout: &KeyboardLayout) -> String {
    let p = preview(token, layout);
    match p.character {
        Some(c) if p.confident => {
            let shown = match c {
                '\n' => "\\n".to_string(),
                '\t' => "\\t".to_string(),
                ' ' => "(space)".to_string(),
                other => other.to_string(),
            };
            format!("{} \u{2014} produces '{shown}'", p.usb_label)
        }
        _ => p.usb_label,
    }
}

/// Distinct layouts seen in an XKB layout string, for the operator-facing summary.
pub fn layout_summary(layout: &KeyboardLayout) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("active layout".into(), layout.layout.clone());
    m.insert(
        "variant".into(),
        layout.variant.clone().unwrap_or_else(|| "(none)".into()),
    );
    m.insert("source".into(), layout.source.label().into());
    m.insert("detail".into(), layout.detail.clone());
    m.insert(
        "preview".into(),
        if layout.preview_is_reliable() {
            "characters are computed from the encoded us mapping".into()
        } else {
            "characters are marked uncertain".into()
        },
    );
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_us_table_covers_every_printable_programming_symbol() {
        // The symbols the guide calls out as critical.
        assert_eq!(us_characters("obrk"), Some(('[', '{')));
        assert_eq!(us_characters("cbrk"), Some((']', '}')));
        assert_eq!(us_characters("perd"), Some(('.', '>')));
        assert_eq!(us_characters("bsls"), Some(('\\', '|')));
        assert_eq!(us_characters("comm"), Some((',', '<')));
    }

    #[test]
    fn us_characters_are_case_insensitive() {
        assert_eq!(us_characters("OBRK"), Some(('[', '{')));
        assert_eq!(us_characters("Q"), Some(('q', 'Q')));
    }

    #[test]
    fn a_reliable_preview_shows_both_characters() {
        let layout = KeyboardLayout::manual("us", None);
        let p = preview("obrk", &layout);
        assert!(p.confident);
        assert_eq!(p.character, Some('['));
        assert_eq!(p.shifted_character, Some('{'));
        assert!(p.note.is_none());
        assert!(p.usb_label.contains("[ and {"));
    }

    #[test]
    fn an_uncertain_layout_never_invents_a_character() {
        let layout = KeyboardLayout::manual("latam", None);
        let p = preview("obrk", &layout);
        assert!(!p.confident);
        assert_eq!(p.character, None);
        let note = p.note.expect("a reason must be given");
        assert!(note.contains("latam"));
        assert!(note.contains("§3.3") || note.contains("3.3"));
    }

    #[test]
    fn an_unknown_layout_says_so() {
        let layout = KeyboardLayout {
            layout: "unknown".into(),
            variant: None,
            source: LayoutSource::Unknown,
            detail: String::new(),
            device: None,
        };
        let p = preview("q", &layout);
        assert!(!p.confident);
        assert!(p.note.unwrap().contains("could not be detected"));
        assert!(layout
            .reliability_warning()
            .unwrap()
            .contains("could not be detected"));
    }

    #[test]
    fn a_us_variant_is_treated_as_uncertain() {
        let layout = KeyboardLayout::manual("us", Some("intl".into()));
        assert!(!layout.preview_is_reliable());
        let p = preview("apos", &layout);
        assert!(!p.confident);
        assert!(p.note.unwrap().contains("intl"));
    }

    #[test]
    fn a_multi_layout_string_uses_the_first_entry_as_active() {
        let layout = KeyboardLayout::manual("us,es", None);
        assert!(layout.is_us());
        assert_eq!(layout.layouts(), vec!["us", "es"]);
    }

    #[test]
    fn non_printable_tokens_have_no_character_but_keep_their_usb_description() {
        let layout = KeyboardLayout::manual("us", None);
        let p = preview("lctr", &layout);
        assert_eq!(p.character, None);
        assert!(p.usb_label.contains("left control"));
        assert!(p.note.unwrap().contains("no character of its own"));
    }

    #[test]
    fn unknown_tokens_are_reported_as_such() {
        let layout = KeyboardLayout::manual("us", None);
        let p = preview("nonsense", &layout);
        assert!(p.usb_label.contains("not in the official token list"));
    }

    #[test]
    fn describe_with_character_mentions_the_produced_symbol() {
        let layout = KeyboardLayout::manual("us", None);
        let s = describe_with_character("obrk", &layout);
        assert!(s.contains("'['"), "{s}");
        let s = describe_with_character("lctr", &layout);
        assert!(!s.contains("produces"), "{s}");
    }

    #[test]
    fn the_layout_summary_is_complete() {
        let layout = KeyboardLayout::manual("us", None);
        let m = layout_summary(&layout);
        assert_eq!(m.get("active layout").unwrap(), "us");
        assert!(m.get("preview").unwrap().contains("computed"));
    }

    #[test]
    fn detection_never_panics_and_always_answers() {
        // On a machine without Hyprland this returns the Unknown variant rather than failing.
        let l = detect();
        assert!(!l.layout.is_empty());
        if l.layout != "unknown" {
            assert_ne!(l.source, LayoutSource::Unknown);
            assert!(
                !l.detail.is_empty(),
                "detection must say how it found the layout"
            );
        }
    }

    #[test]
    fn virtual_and_control_nodes_are_recognised_as_not_a_keyboard() {
        let lowered = "some-consumer-control-thing";
        assert!(VIRTUAL_NODE_HINTS.iter().any(|h| lowered.contains(h)));
        assert!(!VIRTUAL_NODE_HINTS
            .iter()
            .any(|h| "sonix-draconic".contains(h)));
    }

    #[test]
    fn instance_discovery_is_optional_and_never_panics() {
        // Whatever the environment, this must answer without panicking.
        let _ = discover_hyprland_instance();
    }
}
