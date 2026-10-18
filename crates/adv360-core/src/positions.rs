//! The physical position map of the Advantage360 SmartSet USB.
//!
//! Transcribed from **Appendix A — Position Token Map** of the *Advantage360 SmartSet Direct
//! Programming Guide* (version 8-8-25) and cross-checked against the *Advantage360 SmartSet
//! KB360 User's Manual* (v10-12-22) §2.2 "Keyboard Diagram", §5.5 "Four new Hotkeys" and
//! §5.6 "Default Layout Map".
//!
//! Three things this module is deliberately explicit about, because the documentation is:
//!
//! 1. The **SmartSet key** (gear icon, right module) is drawn in the official map *without a
//!    position token*. The guide says "each of the keys on the keyboard (other than the
//!    SmartSet Key which is not customizable) has been assigned a unique 'Position' token", and
//!    the manual confirms the SmartSet key is the "Gear Icon, right module". It therefore can
//!    never appear in a layout file; [`PositionDef::programmable`] is `false` and validation
//!    refuses to address it.
//! 2. The four **foot pedal** positions (`LPD1`, `LPD2`, `RPD1`, `RPD2`) appear in the official
//!    position map but not in the User's Manual default layout map. They are modelled as
//!    programmable positions in their own [`KeyBlock::Pedal`] block, and their factory action is
//!    reported as *uncertain* rather than guessed.
//! 3. Row 3 of the right module and the thumb clusters are **offset** relative to the rows above
//!    them; the coordinates below reproduce that offset so both interfaces draw the real shape.

use serde::{Deserialize, Serialize};

use crate::layer::LayerId;

/// Which physical module a key belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Half {
    /// Left key module.
    Left,
    /// Right key module.
    Right,
    /// FS360 foot pedal (a separate accessory, not part of either module).
    Pedal,
}

impl Half {
    pub fn label(self) -> &'static str {
        match self {
            Half::Left => "Left module",
            Half::Right => "Right module",
            Half::Pedal => "Foot pedals",
        }
    }
}

/// The sub-grid a key lives in. Both interfaces lay this out as three separate blocks, exactly
/// like the official map does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyBlock {
    /// The main keywell of a module.
    Main,
    /// The thumb cluster of a module.
    Thumb,
    /// The FS360 foot pedal row.
    Pedal,
}

/// One physical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PositionDef {
    /// The position token used in layout files. Matching is case-insensitive.
    pub token: &'static str,
    /// The label printed in the official Appendix A map, kept so the operator can cross-check
    /// this tool against the Kinesis document.
    pub map_label: &'static str,
    /// The legend actually printed on the keycap in profile 0, from the manual's §5.6 map.
    /// This is what the operator sees when looking at the keyboard.
    pub keycap: &'static str,
    pub half: Half,
    pub block: KeyBlock,
    /// Grid column inside the block, in key-width units.
    pub x: u8,
    /// Grid row inside the block.
    pub y: u8,
    /// Width in key-width units.
    pub w: u8,
    /// Height in key-height units.
    pub h: u8,
    /// `false` only for the SmartSet key, which has no position token in the official map.
    pub programmable: bool,
    /// Why this key is special, when it is.
    pub note: &'static str,
}

impl PositionDef {
    /// Builder for the static table. Keeps the table below readable.
    #[allow(clippy::too_many_arguments)]
    const fn new(
        token: &'static str,
        map_label: &'static str,
        keycap: &'static str,
        half: Half,
        block: KeyBlock,
        x: u8,
        y: u8,
        w: u8,
        h: u8,
        programmable: bool,
        note: &'static str,
    ) -> Self {
        PositionDef {
            token,
            map_label,
            keycap,
            half,
            block,
            x,
            y,
            w,
            h,
            programmable,
            note,
        }
    }

    /// True for keys that only exist when the FS360 foot pedal is attached.
    pub fn requires_foot_pedal(&self) -> bool {
        self.block == KeyBlock::Pedal
    }
}

use Half::{Left, Pedal, Right};
use KeyBlock::{Main, Pedal as PedalBlock, Thumb};

const NONE: &str = "";

/// Every physical position, in the order the official map presents it.
pub static POSITIONS: &[PositionDef] = &[
    // ================= LEFT MODULE =================
    PositionDef::new("eql", "EQL", "=", Left, Main, 0, 0, 1, 1, true, NONE),
    PositionDef::new("1", "1", "1", Left, Main, 1, 0, 1, 1, true, NONE),
    PositionDef::new("2", "2", "2", Left, Main, 2, 0, 1, 1, true, NONE),
    PositionDef::new("3", "3", "3", Left, Main, 3, 0, 1, 1, true, NONE),
    PositionDef::new("4", "4", "4", Left, Main, 4, 0, 1, 1, true, NONE),
    PositionDef::new("5", "5", "5", Left, Main, 5, 0, 1, 1, true, NONE),
    PositionDef::new(
        "kp",
        "KP",
        "Kp Toggle",
        Left,
        Main,
        6,
        0,
        1,
        1,
        true,
        "Keypad layer access key. Toggles the keypad layer by default.",
    ),
    PositionDef::new("tab", "TAB", "Tab", Left, Main, 0, 1, 1, 1, true, NONE),
    PositionDef::new("q", "Q", "Q", Left, Main, 1, 1, 1, 1, true, NONE),
    PositionDef::new("w", "W", "W", Left, Main, 2, 1, 1, 1, true, NONE),
    PositionDef::new("e", "E", "E", Left, Main, 3, 1, 1, 1, true, NONE),
    PositionDef::new("r", "R", "R", Left, Main, 4, 1, 1, 1, true, NONE),
    PositionDef::new("t", "T", "T", Left, Main, 5, 1, 1, 1, true, NONE),
    PositionDef::new(
        "hk1",
        "HK1",
        "1 (circled)",
        Left,
        Main,
        6,
        1,
        1,
        1,
        true,
        HOTKEY_NOTE,
    ),
    PositionDef::new("esc", "ESC", "Esc", Left, Main, 0, 2, 1, 1, true, NONE),
    PositionDef::new("a", "A", "A", Left, Main, 1, 2, 1, 1, true, NONE),
    PositionDef::new("s", "S", "S", Left, Main, 2, 2, 1, 1, true, NONE),
    PositionDef::new("d", "D", "D", Left, Main, 3, 2, 1, 1, true, NONE),
    PositionDef::new("f", "F", "F", Left, Main, 4, 2, 1, 1, true, NONE),
    PositionDef::new("g", "G", "G", Left, Main, 5, 2, 1, 1, true, NONE),
    PositionDef::new(
        "hk2",
        "HK2",
        "2 (circled)",
        Left,
        Main,
        6,
        2,
        1,
        1,
        true,
        HOTKEY_NOTE,
    ),
    PositionDef::new("lshf", "LSHF", "Shift", Left, Main, 0, 3, 1, 1, true, NONE),
    PositionDef::new("z", "Z", "Z", Left, Main, 1, 3, 1, 1, true, NONE),
    PositionDef::new("x", "X", "X", Left, Main, 2, 3, 1, 1, true, NONE),
    PositionDef::new("c", "C", "C", Left, Main, 3, 3, 1, 1, true, NONE),
    PositionDef::new("v", "V", "V", Left, Main, 4, 3, 1, 1, true, NONE),
    PositionDef::new("b", "B", "B", Left, Main, 5, 3, 1, 1, true, NONE),
    PositionDef::new(
        "lfn",
        "LFN",
        "Fn Shift",
        Left,
        Main,
        0,
        4,
        1,
        1,
        true,
        FN_LAYER_NOTE,
    ),
    PositionDef::new("grav", "GRAV", "` ~", Left, Main, 1, 4, 1, 1, true, NONE),
    PositionDef::new(
        "caps",
        "CAPS",
        "Caps Lock",
        Left,
        Main,
        2,
        4,
        1,
        1,
        true,
        NONE,
    ),
    PositionDef::new(
        "left", "LEFT", "\u{2190}", Left, Main, 3, 4, 1, 1, true, NONE,
    ),
    PositionDef::new(
        "rght", "RGHT", "\u{2192}", Left, Main, 4, 4, 1, 1, true, NONE,
    ),
    // left thumb cluster
    PositionDef::new("lctr", "LCTR", "Ctrl", Left, Thumb, 1, 0, 1, 1, true, NONE),
    PositionDef::new("lalt", "LALT", "Alt", Left, Thumb, 2, 0, 1, 1, true, NONE),
    PositionDef::new(
        "bspc",
        "BSPC",
        "Backspace",
        Left,
        Thumb,
        0,
        1,
        1,
        2,
        true,
        NONE,
    ),
    PositionDef::new("del", "DEL", "Delete", Left, Thumb, 1, 1, 1, 2, true, NONE),
    PositionDef::new("home", "HOME", "Home", Left, Thumb, 2, 1, 1, 1, true, NONE),
    PositionDef::new("end", "END", "End", Left, Thumb, 2, 2, 1, 1, true, NONE),
    // ================= RIGHT MODULE =================
    PositionDef::new(
        "ss",
        "(unlabelled)",
        "(gear icon)",
        Right,
        Main,
        0,
        0,
        1,
        1,
        false,
        SMARTSET_NOTE,
    ),
    PositionDef::new("6", "6", "6", Right, Main, 1, 0, 1, 1, true, NONE),
    PositionDef::new("7", "7", "7", Right, Main, 2, 0, 1, 1, true, NONE),
    PositionDef::new("8", "8", "8", Right, Main, 3, 0, 1, 1, true, NONE),
    PositionDef::new("9", "9", "9", Right, Main, 4, 0, 1, 1, true, NONE),
    PositionDef::new("0", "0", "0", Right, Main, 5, 0, 1, 1, true, NONE),
    PositionDef::new("hyph", "HYPH", "- _", Right, Main, 6, 0, 1, 1, true, NONE),
    PositionDef::new(
        "hk3",
        "HK3",
        "3 (circled)",
        Right,
        Main,
        0,
        1,
        1,
        1,
        true,
        HOTKEY_NOTE,
    ),
    PositionDef::new("y", "Y", "Y", Right, Main, 1, 1, 1, 1, true, NONE),
    PositionDef::new("u", "U", "U", Right, Main, 2, 1, 1, 1, true, NONE),
    PositionDef::new("i", "I", "I", Right, Main, 3, 1, 1, 1, true, NONE),
    PositionDef::new("o", "O", "O", Right, Main, 4, 1, 1, 1, true, NONE),
    PositionDef::new("p", "P", "P", Right, Main, 5, 1, 1, 1, true, NONE),
    PositionDef::new("bsls", "BSLS", "\\ |", Right, Main, 6, 1, 1, 1, true, NONE),
    PositionDef::new(
        "hk4",
        "HK4",
        "4 (circled)",
        Right,
        Main,
        0,
        2,
        1,
        1,
        true,
        HOTKEY_NOTE,
    ),
    PositionDef::new("h", "H", "H", Right, Main, 1, 2, 1, 1, true, NONE),
    PositionDef::new("j", "J", "J", Right, Main, 2, 2, 1, 1, true, NONE),
    PositionDef::new("k", "K", "K", Right, Main, 3, 2, 1, 1, true, NONE),
    PositionDef::new("l", "L", "L", Right, Main, 4, 2, 1, 1, true, NONE),
    PositionDef::new("scol", "SCOL", "; :", Right, Main, 5, 2, 1, 1, true, NONE),
    PositionDef::new("apos", "APOS", "' \"", Right, Main, 6, 2, 1, 1, true, NONE),
    // Row 3 of the right module is offset one column to the right in the official map.
    PositionDef::new("n", "N", "N", Right, Main, 1, 3, 1, 1, true, NONE),
    PositionDef::new("m", "M", "M", Right, Main, 2, 3, 1, 1, true, NONE),
    PositionDef::new("comm", "COMM", ", <", Right, Main, 3, 3, 1, 1, true, NONE),
    PositionDef::new("perd", "PERD", ". >", Right, Main, 4, 3, 1, 1, true, NONE),
    PositionDef::new("fsls", "FSLS", "/ ?", Right, Main, 5, 3, 1, 1, true, NONE),
    PositionDef::new("rshf", "RSHF", "Shift", Right, Main, 6, 3, 1, 1, true, NONE),
    // Row 4 of the right module starts at column 2.
    PositionDef::new("up", "UP", "\u{2191}", Right, Main, 2, 4, 1, 1, true, NONE),
    PositionDef::new(
        "down", "DOWN", "\u{2193}", Right, Main, 3, 4, 1, 1, true, NONE,
    ),
    PositionDef::new("obrk", "OBRK", "[ {", Right, Main, 4, 4, 1, 1, true, NONE),
    PositionDef::new("cbrk", "CBRK", "] }", Right, Main, 5, 4, 1, 1, true, NONE),
    PositionDef::new(
        "rfn",
        "RFN",
        "Fn Shift",
        Right,
        Main,
        6,
        4,
        1,
        1,
        true,
        FN_LAYER_NOTE,
    ),
    // right thumb cluster
    PositionDef::new("rwin", "RWIN", "Win", Right, Thumb, 0, 0, 1, 1, true, NONE),
    PositionDef::new("rctr", "RCTR", "Ctrl", Right, Thumb, 1, 0, 1, 1, true, NONE),
    PositionDef::new(
        "pgup", "PGUP", "Page Up", Right, Thumb, 0, 1, 1, 1, true, NONE,
    ),
    PositionDef::new("ent", "ENT", "Enter", Right, Thumb, 1, 1, 1, 2, true, NONE),
    PositionDef::new("spc", "SPC", "Space", Right, Thumb, 2, 1, 1, 2, true, NONE),
    PositionDef::new(
        "pgdn",
        "PGDN",
        "Page Down",
        Right,
        Thumb,
        0,
        2,
        1,
        1,
        true,
        NONE,
    ),
    // ================= FS360 FOOT PEDALS =================
    PositionDef::new(
        "lpd1", "LPD1", "LPD1", Pedal, PedalBlock, 0, 0, 1, 1, true, PEDAL_NOTE,
    ),
    PositionDef::new(
        "lpd2", "LPD2", "LPD2", Pedal, PedalBlock, 1, 0, 1, 1, true, PEDAL_NOTE,
    ),
    PositionDef::new(
        "rpd1", "RPD1", "RPD1", Pedal, PedalBlock, 2, 0, 1, 1, true, PEDAL_NOTE,
    ),
    PositionDef::new(
        "rpd2", "RPD2", "RPD2", Pedal, PedalBlock, 3, 0, 1, 1, true, PEDAL_NOTE,
    ),
];

const HOTKEY_NOTE: &str = "Hotkey. The manual states that by default these four keys are blank, so their factory action is [null]. They can be programmed in every layer.";
const FN_LAYER_NOTE: &str = "Fn layer access key. Shifts to the Fn1 layer while held (fn1s).";
const SMARTSET_NOTE: &str = "The dedicated SmartSet key (gear icon, right module). The official position map draws it without a position token and the guide excludes it from programmable keys, so it can never be used in a layout file.";
const PEDAL_NOTE: &str = "FS360 foot pedal position, listed in Appendix A of the Direct Programming Guide. Only present when the foot pedal is attached.";

/// Number of grid columns in the main keywell of each module.
pub const MAIN_COLS: u8 = 7;
/// Number of grid rows in the main keywell of each module.
pub const MAIN_ROWS: u8 = 5;

/// Case-insensitive lookup of a physical position.
///
/// Returns the entry for the SmartSet key too (`ss`), so that user interfaces can render it; use
/// [`PositionDef::programmable`] to decide whether it may be addressed from a layout file.
pub fn lookup(token: &str) -> Option<&'static PositionDef> {
    let needle = token.trim();
    POSITIONS
        .iter()
        .find(|p| p.token.eq_ignore_ascii_case(needle))
}

/// True when `token` is a position token that a layout file is allowed to address.
pub fn is_programmable_position(token: &str) -> bool {
    lookup(token).is_some_and(|p| p.programmable)
}

/// Every programmable position inside a half and block, sorted by row then column.
pub fn positions_in(half: Half, block: KeyBlock) -> Vec<&'static PositionDef> {
    let mut v: Vec<&'static PositionDef> = POSITIONS
        .iter()
        .filter(|p| p.half == half && p.block == block)
        .collect();
    v.sort_by_key(|p| (p.y, p.x));
    v
}

/// All programmable positions, in catalog order.
pub fn programmable() -> impl Iterator<Item = &'static PositionDef> {
    POSITIONS.iter().filter(|p| p.programmable)
}

// ---------------------------------------------------------------------------------------------
// Documented factory actions per layer
// ---------------------------------------------------------------------------------------------

/// Where a factory (default) action comes from in the official documentation. Modelled explicitly
/// so that neither interface presents an inferred value as if Kinesis had published it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefaultSource {
    /// Printed for exactly this key and layer in the manual's §5.6 default layout map.
    Documented,
    /// The manual states these keys are blank by default, i.e. the factory action is `[null]`.
    DocumentedNoAction,
    /// The manual titles the figure "Function Layers (Fn1, Fn2, Fn3)", so the same factory
    /// actions apply to all three function layers.
    DocumentedSharedFunctionMap,
    /// Nothing is published for this position; the base-layer action is assumed to still apply.
    InferredFromBase,
    /// The guide documents no factory action for this position, so it is reported as unknown
    /// instead of being guessed.
    Uncertain,
}

/// The factory action of one key in one layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LayerDefault {
    /// The action token, or `"null"` when the key produces nothing.
    pub action: &'static str,
    pub source: DefaultSource,
}

impl LayerDefault {
    /// The documented action token as a `String` (the catalog uses owned tokens because files may
    /// contain tokens this build does not know).
    pub fn action_token(&self) -> String {
        self.action.to_string()
    }

    /// True when the value is published by Kinesis for this exact key and layer.
    pub fn is_documented(&self) -> bool {
        matches!(
            self.source,
            DefaultSource::Documented
                | DefaultSource::DocumentedNoAction
                | DefaultSource::DocumentedSharedFunctionMap
        )
    }
}

/// Keys whose factory action is *not* simply their own position token in the base layer.
fn base_override(token: &str) -> Option<LayerDefault> {
    let documented = |action: &'static str| {
        Some(LayerDefault {
            action,
            source: DefaultSource::DocumentedNoAction,
        })
    };
    match token {
        // Manual §5.5: "By default these keys are blank".
        "hk1" | "hk2" | "hk3" | "hk4" => documented("null"),
        // Manual §5.3: the two Fn keys momentarily shift the keyboard to the Fn layer.
        "lfn" | "rfn" => Some(LayerDefault {
            action: "fn1s",
            source: DefaultSource::Documented,
        }),
        // Manual §5.4: the Keypad key toggles the keypad layer.
        "kp" => Some(LayerDefault {
            action: "keyt",
            source: DefaultSource::Documented,
        }),
        // Not published anywhere: reported as uncertain rather than guessed.
        "lpd1" | "lpd2" | "rpd1" | "rpd2" => Some(LayerDefault {
            action: "null",
            source: DefaultSource::Uncertain,
        }),
        // The SmartSet key is not programmable and never appears in a layout file.
        "ss" => Some(LayerDefault {
            action: "ss",
            source: DefaultSource::Documented,
        }),
        _ => None,
    }
}

/// The documented Fn1 layer override, from the manual §5.6 "Function Layers (Fn1, Fn2, Fn3)" map
/// and the §5.3 example "press and hold either of the Fn Layer Keys and then tap the 'equals'
/// keys" to output F1.
fn fn_override(token: &str, source: DefaultSource) -> Option<LayerDefault> {
    let action = match token {
        "eql" => "f1",
        "1" => "f2",
        "2" => "f3",
        "3" => "f4",
        "4" => "f5",
        "5" => "f6",
        "6" => "f7",
        "7" => "f8",
        "8" => "f9",
        "9" => "f10",
        "0" => "f11",
        "hyph" => "f12",
        _ => return None,
    };
    Some(LayerDefault { action, source })
}

/// The documented Keypad layer override, from the manual §5.6 "Keypad Layer" map. The manual
/// states the layer "features 18 unique key actions on the right module (traditional 10 key)";
/// the table below contains exactly 18 distinct actions.
fn keypad_override(token: &str) -> Option<LayerDefault> {
    let action = match token {
        "7" => "nmlk",
        "8" => "kp=",
        "9" => "kp/",
        "0" => "kp*",
        "p" => "kp-",
        "u" => "kp7",
        "i" => "kp8",
        "o" => "kp9",
        "j" => "kp4",
        "k" => "kp5",
        "l" => "kp6",
        "scol" => "kp+",
        "m" => "kp1",
        "comm" => "kp2",
        "perd" => "kp3",
        "fsls" => "kpen",
        "obrk" => "kp.",
        "spc" => "kp0",
        _ => return None,
    };
    Some(LayerDefault {
        action,
        source: DefaultSource::Documented,
    })
}

/// The factory action of a position in a layer, as published by Kinesis where it is published.
pub fn documented_default(position: &PositionDef, layer: LayerId) -> LayerDefault {
    let token = position.token;

    if let Some(ov) = base_override(token) {
        // Layer access keys keep their function on every layer by default.
        return ov;
    }

    match layer {
        LayerId::Base => LayerDefault {
            action: token,
            source: DefaultSource::Documented,
        },
        LayerId::Function1 => {
            fn_override(token, DefaultSource::Documented).unwrap_or(LayerDefault {
                action: token,
                source: DefaultSource::InferredFromBase,
            })
        }
        // The manual presents one figure for all three function layers.
        LayerId::Function2 | LayerId::Function3 => {
            fn_override(token, DefaultSource::DocumentedSharedFunctionMap).unwrap_or(LayerDefault {
                action: token,
                source: DefaultSource::DocumentedSharedFunctionMap,
            })
        }
        LayerId::Keypad => keypad_override(token).unwrap_or(LayerDefault {
            action: token,
            source: DefaultSource::InferredFromBase,
        }),
    }
}

/// Number of distinct actions the manual attributes to the keypad layer. Kept as a constant so a
/// test can assert the table above still produces exactly this many.
pub const DOCUMENTED_KEYPAD_UNIQUE_ACTIONS: usize = 18;

/// Number of distinct actions the manual attributes to the Fn layer.
pub const DOCUMENTED_FN_UNIQUE_ACTIONS: usize = 12;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn all_75_programmable_keys_plus_smartset_and_pedals_are_present() {
        let programable = programmable().count();
        // 38 keys on the left module + 37 programmable keys on the right module + 4 pedals.
        // The right module has 38 physical keys, one of which is the SmartSet key.
        assert_eq!(
            programable,
            38 + 37 + 4,
            "unexpected number of programmable positions"
        );
        assert_eq!(POSITIONS.len(), 38 + 38 + 4);
    }

    #[test]
    fn smartset_key_is_the_only_non_programmable_position() {
        let non: Vec<&str> = POSITIONS
            .iter()
            .filter(|p| !p.programmable)
            .map(|p| p.token)
            .collect();
        assert_eq!(non, vec!["ss"]);
        let ss = lookup("ss").unwrap();
        assert_eq!(ss.half, Half::Right);
        assert_eq!(
            (ss.x, ss.y),
            (0, 0),
            "the gear key is the top-left key of the right module"
        );
        assert!(!is_programmable_position("ss"));
    }

    #[test]
    fn position_tokens_are_unique_case_insensitively() {
        let mut seen = BTreeSet::new();
        for p in POSITIONS {
            assert!(
                seen.insert(p.token.to_ascii_lowercase()),
                "duplicate {}",
                p.token
            );
        }
    }

    #[test]
    fn right_module_offsets_match_the_official_map() {
        // Row 3 starts at column 1 and row 4 at column 2 on the right module.
        assert_eq!(lookup("n").unwrap().x, 1);
        assert_eq!(lookup("m").unwrap().x, 2);
        assert_eq!(lookup("rshf").unwrap().x, 6);
        assert_eq!(lookup("up").unwrap().x, 2);
        assert_eq!(lookup("rfn").unwrap().x, 6);
    }

    #[test]
    fn tall_thumb_keys_are_two_units_high() {
        assert_eq!(lookup("bspc").unwrap().h, 2);
        assert_eq!(lookup("del").unwrap().h, 2);
        assert_eq!(lookup("ent").unwrap().h, 2);
        assert_eq!(lookup("spc").unwrap().h, 2);
    }

    #[test]
    fn hotkeys_default_to_no_action_as_the_manual_states() {
        for hk in ["hk1", "hk2", "hk3", "hk4"] {
            let d = documented_default(lookup(hk).unwrap(), LayerId::Base);
            assert_eq!(d.action, "null");
            assert_eq!(d.source, DefaultSource::DocumentedNoAction);
            assert!(d.is_documented());
        }
    }

    #[test]
    fn fn_layer_has_twelve_documented_actions_matching_the_manual_example() {
        let unique: BTreeSet<&str> = [
            "eql", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "hyph",
        ]
        .iter()
        .map(|t| documented_default(lookup(t).unwrap(), LayerId::Function1).action)
        .collect();
        assert_eq!(unique.len(), DOCUMENTED_FN_UNIQUE_ACTIONS);
        // Manual §5.3: Fn + = produces F1.
        assert_eq!(
            documented_default(lookup("eql").unwrap(), LayerId::Function1).action,
            "f1"
        );
        assert_eq!(
            documented_default(lookup("hyph").unwrap(), LayerId::Function1).action,
            "f12"
        );
    }

    #[test]
    fn keypad_layer_has_exactly_the_documented_number_of_unique_actions() {
        let unique: BTreeSet<&str> = programmable()
            .map(|p| documented_default(p, LayerId::Keypad).action)
            .collect();
        // Count only the actions the keypad layer *adds* to a key, so the layer access keys (which
        // keep their function on every layer) are not miscounted as keypad actions.
        let overridden: BTreeSet<&str> = programmable()
            .filter(|p| {
                documented_default(p, LayerId::Keypad).action
                    != documented_default(p, LayerId::Base).action
            })
            .map(|p| documented_default(p, LayerId::Keypad).action)
            .collect();
        assert_eq!(overridden.len(), DOCUMENTED_KEYPAD_UNIQUE_ACTIONS);
        assert!(unique.contains("nmlk"));
        assert_eq!(
            documented_default(lookup("spc").unwrap(), LayerId::Keypad).action,
            "kp0"
        );
        assert_eq!(
            documented_default(lookup("p").unwrap(), LayerId::Keypad).action,
            "kp-"
        );
        // Left-module keys are untouched by the keypad layer.
        assert_eq!(
            documented_default(lookup("q").unwrap(), LayerId::Keypad).source,
            DefaultSource::InferredFromBase
        );
    }

    #[test]
    fn layer_access_keys_keep_their_function_in_every_layer() {
        for layer in LayerId::all() {
            assert_eq!(
                documented_default(lookup("lfn").unwrap(), *layer).action,
                "fn1s"
            );
            assert_eq!(
                documented_default(lookup("rfn").unwrap(), *layer).action,
                "fn1s"
            );
            assert_eq!(
                documented_default(lookup("kp").unwrap(), *layer).action,
                "keyt"
            );
        }
    }

    #[test]
    fn foot_pedal_defaults_are_reported_as_uncertain() {
        let d = documented_default(lookup("lpd1").unwrap(), LayerId::Base);
        assert_eq!(d.source, DefaultSource::Uncertain);
        assert!(!d.is_documented());
        assert!(lookup("lpd1").unwrap().requires_foot_pedal());
    }

    #[test]
    fn default_actions_are_documented_or_flagged() {
        // Nothing may claim Documentation it does not have.
        for p in programmable() {
            for layer in LayerId::all() {
                let d = documented_default(p, *layer);
                assert!(!d.action.is_empty());
                if d.source == DefaultSource::Documented
                    || d.source == DefaultSource::DocumentedNoAction
                    || d.source == DefaultSource::DocumentedSharedFunctionMap
                {
                    assert!(
                        crate::tokens::is_known(d.action),
                        "documented default {} for {} is not a known action token",
                        d.action,
                        p.token
                    );
                }
            }
        }
    }
}
