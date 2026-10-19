//! Reusable core for configuring a **Kinesis Advantage360 SmartSet USB** keyboard from Linux.
//!
//! This crate is the only place that reads, interprets, validates, backs up or writes the
//! keyboard's configuration files. The GTK4 GUI and the Ratatui TUI are thin clients: they render
//! [`session::Session`] state and call its methods, so both interfaces cannot drift apart.
//!
//! # Where every rule comes from
//!
//! Nothing in this crate is invented. The behaviour comes from three official documents, fetched
//! from <https://kinesis-ergo.com/support/kb360/> and encoded here with the source noted at each
//! decision:
//!
//! * **Advantage360 SmartSet Direct Programming Guide**, version 8-8-25 — the file format, the
//!   five layer headers, remap/macro/tap-and-hold syntax, the `*` disable marker, macro limits,
//!   the LED syntax, the layer/profile access tokens and the eject rules. See
//!   [`model`], [`parser`], [`writer`] and [`validate`].
//! * **Advantage360 SmartSet Direct Programming Action Tokens**, version 3-31-23 — every action
//!   token. See [`tokens`].
//! * **Advantage360 SmartSet KB360 User's Manual**, version 10-12-22 — the physical layout,
//!   profile 0 being non-programmable, the nine programmable profiles, the five layers, the four
//!   hotkeys being blank by default, and the factory actions per layer. See [`positions`].
//!
//! Where the documentation is silent or ambiguous, this crate says so explicitly instead of
//! guessing: [`positions::DefaultSource`] distinguishes published values from inferred ones,
//! [`tokens::TapHoldSupport`] marks the guide's unqualified tap-and-hold caveat, and
//! [`kbd_layout`] refuses to show a character it cannot compute.
//!
//! # The protection rules this crate enforces
//!
//! * `settings.txt` is read-only, always.
//! * Profile 0 is never editable and has no layout file.
//! * The SmartSet key cannot be programmed (the official position map gives it no token).
//! * A file is only written after: a dated verified backup, clean validation, and an explicit
//!   confirmation that includes how many lines will change.
//! * Lines are never deleted. Removing a binding switches the line off with the documented `*`
//!   marker, so the operator's original text survives and the change is undoable.
//! * Unknown and comment lines are preserved byte for byte, together with their line endings, a
//!   BOM, and a missing final newline.
//! * The writer verifies its own output by reading it back, and the rename that replaces the file
//!   is atomic, so a failure leaves the original in place.
//!
//! # A minimal example
//!
//! ```no_run
//! use adv360_core::prelude::*;
//!
//! # fn main() -> CoreResult<()> {
//! // Open the keyboard's v-Drive (or a copy of it).
//! let mut session = Session::open(SessionOptions::default())?;
//!
//! // Assign Escape the Caps Lock action, in profile 1, Base layer.
//! session.set_binding(
//!     ProfileId(1),
//!     LayerId::Base,
//!     "esc",
//!     Some(Assignment::Remap { position: "esc".into(), action: "caps".into() }),
//! )?;
//!
//! // Look at what would change, and whether it is safe.
//! let changed = session.file_change_count(session.profile_file_index(ProfileId(1)).unwrap());
//! assert!(!session.validation_for_write(ProfileId(1)).blocks_write());
//!
//! // Back up, then write with an explicit confirmation.
//! session.backup(&[ProfileId(1)])?;
//! let report = session.write_profile(WriteConfirmation {
//!     profile: ProfileId(1),
//!     confirmed_by_operator: true,
//!     acknowledged_changed_lines: changed,
//! })?;
//! assert!(report.verified);
//! # Ok(())
//! # }
//! ```

pub mod backup;
pub mod device;
pub mod diff;
pub mod error;
pub mod glyph;
pub mod kbd_layout;
pub mod layer;
pub mod model;
pub mod parse_error;
pub mod parser;
pub mod positions;
pub mod presets;
pub mod session;
pub mod tokens;
pub mod validate;
pub mod writer;

/// The names the two interfaces use most.
pub mod prelude {
    pub use crate::device::{self, VDrive};
    pub use crate::diff::{Binding, DiffSummary, KeyChange, KeyChangeKind};
    pub use crate::error::{CoreError, CoreResult};
    pub use crate::glyph;
    pub use crate::kbd_layout::{self, KeyboardLayout};
    pub use crate::layer::{LayerId, ProfileId};
    pub use crate::model::{
        ConfigFile, Eol, FileRole, Indicator, LedFunction, LedLine, Line, LineKind, MacroAtom,
        MacroLine, RemapAtom, RemapLine, Rgb, Stroke,
    };
    pub use crate::positions::{self, DefaultSource, Half, KeyBlock, LayerDefault, PositionDef};
    pub use crate::presets::{self, AccessKeyOption, Assignment, Draft, LayerPreset};
    pub use crate::session::{
        KeyState, PendingChange, ProfileSummary, Session, SessionOptions, SessionSummary,
        WriteConfirmation, WriteReport,
    };
    pub use crate::tokens::{self, Token, TokenCategory};
    pub use crate::validate::{self, Finding, Severity, ValidationReport};
    pub use crate::writer::{self, WritePlan};
}

/// The application name, used for the backup directory, the desktop entry and the CLI.
pub const APP_NAME: &str = "adv360-config";

/// The version reported by both interfaces.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The exact documentation this build was written against. Printed by `--about` and shown in the
/// interfaces' help, so an operator can tell whether a newer Kinesis document has superseded it.
pub const SOURCES: &[(&str, &str)] = &[
    (
        "Advantage360 SmartSet Direct Programming Guide",
        "version 8-8-25 — https://kinesis-ergo.com/wp-content/uploads/Adv360-SmartSet-Direct-Programming-Guide-Version-8-8-25.pdf",
    ),
    (
        "Advantage360 SmartSet Direct Programming Action Tokens",
        "version 3-31-23 — https://kinesis-ergo.com/wp-content/uploads/Adv360-SmartSet-Direct-Programming-Action-Tokens-v3-31-23.pdf",
    ),
    (
        "Advantage360 SmartSet KB360 User's Manual",
        "version 10-12-22 — https://kinesis-ergo.com/wp-content/uploads/Advantage360-SmartSet-KB360-Users-Manual-v10-12-22.pdf",
    ),
];

#[cfg(test)]
mod docs_tests {
    use super::*;

    #[test]
    fn the_documentation_this_build_targets_is_declared() {
        assert_eq!(SOURCES.len(), 3);
        for (name, detail) in SOURCES {
            assert!(!name.is_empty());
            assert!(detail.contains("version"), "{name} must state the version");
            assert!(
                detail.contains("kinesis-ergo.com"),
                "{name} must cite the source"
            );
        }
    }

    #[test]
    fn the_version_is_reported() {
        assert!(!VERSION.is_empty());
        assert_eq!(APP_NAME, "adv360-config");
    }
}

#[cfg(test)]
mod message_hygiene {
    //! A guard against a defect that reached the screen once: a message built with a
    //! backslash-continued string literal whose backslash was lost keeps the following line's
    //! indentation as literal spaces, and the operator reads "convert it to UTF-8          yourself".
    //! Scanning the sources is the only way to cover every message without enumerating every branch
    //! that can produce one.

    /// Every module of this crate that builds text a person reads.
    const SOURCES: &[(&str, &str)] = &[
        ("backup.rs", include_str!("backup.rs")),
        ("device.rs", include_str!("device.rs")),
        ("diff.rs", include_str!("diff.rs")),
        ("error.rs", include_str!("error.rs")),
        ("glyph.rs", include_str!("glyph.rs")),
        ("kbd_layout.rs", include_str!("kbd_layout.rs")),
        ("layer.rs", include_str!("layer.rs")),
        ("model.rs", include_str!("model.rs")),
        ("parse_error.rs", include_str!("parse_error.rs")),
        ("parser.rs", include_str!("parser.rs")),
        ("positions.rs", include_str!("positions.rs")),
        ("presets.rs", include_str!("presets.rs")),
        ("session.rs", include_str!("session.rs")),
        ("tokens.rs", include_str!("tokens.rs")),
        ("validate.rs", include_str!("validate.rs")),
        ("writer.rs", include_str!("writer.rs")),
    ];

    /// True when a run of two or more spaces sits between two non-space characters, which is what a
    /// lost continuation backslash leaves behind.
    fn has_embedded_run(text: &str) -> Option<usize> {
        let chars: Vec<char> = text.chars().collect();
        let mut index = 0usize;
        while index < chars.len() {
            if chars[index] != ' ' {
                index += 1;
                continue;
            }
            let start = index;
            while index < chars.len() && chars[index] == ' ' {
                index += 1;
            }
            // A run at the very start or end of a literal is ordinary padding; only a run sitting
            // between two visible characters is the defect. Counting the whole run matters: an
            // earlier version of this detector only looked at four-character windows and missed
            // runs longer than two spaces, which is exactly what the defect produces.
            if index - start >= 2 && start > 0 {
                if let Some(after) = chars.get(index) {
                    if !after.is_whitespace() {
                        return Some(start);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn the_detector_finds_the_defect_it_looks_for() {
        // A guard that cannot fail is worthless, so the detector is tested against the three real
        // occurrences and against text that must not be flagged.
        assert!(has_embedded_run("convert it to UTF-8          yourself").is_some());
        assert!(has_embedded_run("the last line                      wins.").is_some());
        assert!(has_embedded_run("two  spaces").is_some());
        assert!(has_embedded_run("closed \\n                     after the session").is_some());
        assert!(has_embedded_run("an ordinary sentence, with punctuation.").is_none());
        assert!(has_embedded_run("02, 03, 04, and 05").is_none());
        assert!(has_embedded_run("padded        ").is_none());
        assert!(has_embedded_run("        indented").is_none());
    }

    #[test]
    fn no_message_contains_a_broken_string_continuation() {
        let mut offenders = Vec::new();
        for (name, source) in SOURCES {
            for (number, line) in source.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                // Test modules live at the end of each module by convention, and their literals are
                // expectations rather than messages: `assert_eq!(rendered, "*   [q]>[w]")` is
                // deliberately full of spaces. Only production text is the operator's to read.
                if trimmed.starts_with("#[cfg(test)]") {
                    break;
                }
                // Only the first literal on the line, which is where these messages live.
                let Some(open) = line.find('"') else { continue };
                let body = &line[open + 1..];
                let Some(close) = body.find('"') else {
                    continue;
                };
                let literal = &body[..close];
                if let Some(column) = has_embedded_run(literal) {
                    offenders.push(format!(
                        "{name}:{} column {} -> {}",
                        number + 1,
                        column + 1,
                        literal
                    ));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a message keeps indentation spaces, which means a continuation backslash was lost:\n{}",
            offenders.join("\n")
        );
    }
}
