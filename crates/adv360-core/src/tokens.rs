//! The complete SmartSet action token catalog.
//!
//! Transcribed from *Advantage360 SmartSet Direct Programming Action Tokens v3-31-23*
//! (Kinesis, 31 March 2023), the token list referenced by the Direct Programming Guide.
//!
//! Token matching is **case-insensitive**: the guide states "Tokens are not case-sensitive.
//! Capitalizing a token does not produce the 'shifted' action." The catalog keeps the token
//! exactly as printed in the official list so that generated files look like the documented
//! examples.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Which part of the documentation a token comes from. Kept because the guide warns that the
/// USB HID actions are not supported on all operating systems, and because the operator needs
/// to know how much to trust a given token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenGroup {
    /// USB Keyboard/Keypad Page (0x0C).
    Keyboard,
    /// USB Consumer Page actions (multimedia).
    Consumer,
    /// Custom Kinesis tokens (layers, profiles, mouse, eLED, chords).
    Kinesis,
}

/// A coarse bucket used by the searchable catalog UI. Purely presentational.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TokenCategory {
    Letters,
    Digits,
    Punctuation,
    Editing,
    Navigation,
    FunctionKeys,
    Keypad,
    Modifiers,
    Consumer,
    Mouse,
    Layer,
    Profile,
    Led,
    Special,
    International,
    Internal,
}

impl TokenCategory {
    /// Human readable name, used by both interfaces.
    pub fn label(self) -> &'static str {
        match self {
            TokenCategory::Letters => "Letters",
            TokenCategory::Digits => "Digits",
            TokenCategory::Punctuation => "Punctuation",
            TokenCategory::Editing => "Editing",
            TokenCategory::Navigation => "Navigation",
            TokenCategory::FunctionKeys => "Function keys",
            TokenCategory::Keypad => "Keypad",
            TokenCategory::Modifiers => "Modifiers",
            TokenCategory::Consumer => "Media & consumer",
            TokenCategory::Mouse => "Mouse",
            TokenCategory::Layer => "Layers",
            TokenCategory::Profile => "Profiles",
            TokenCategory::Led => "LEDs & backlight",
            TokenCategory::Special => "Kinesis special",
            TokenCategory::International => "International",
            TokenCategory::Internal => "Internal / reserved",
        }
    }

    /// Stable order for menus and the catalog listing.
    pub fn all() -> &'static [TokenCategory] {
        &[
            TokenCategory::Letters,
            TokenCategory::Digits,
            TokenCategory::Punctuation,
            TokenCategory::Editing,
            TokenCategory::Navigation,
            TokenCategory::FunctionKeys,
            TokenCategory::Keypad,
            TokenCategory::Modifiers,
            TokenCategory::Layer,
            TokenCategory::Profile,
            TokenCategory::Consumer,
            TokenCategory::Mouse,
            TokenCategory::Led,
            TokenCategory::Special,
            TokenCategory::International,
            TokenCategory::Internal,
        ]
    }
}

/// How well Tap-and-Hold is known to work with a token.
///
/// The Direct Programming Guide only says "Not all key actions support Tap-and-Hold" without
/// publishing which ones do. This type keeps that gap explicit instead of inventing an answer:
/// validation raises a warning for [`TapHoldSupport::Unknown`] and an error only for
/// [`TapHoldSupport::No`], where the incompatibility follows from the token's own semantics
/// (a layer/profile action cannot meaningfully be the tap half of a dual-role key on this
/// firmware).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TapHoldSupport {
    No,
    Unknown,
}

/// Capability flags used by validation, the macro editor and the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCaps {
    /// Can act as a macro co-trigger (modifier) and be emitted with `-token` / `+token`.
    pub modifier: bool,
    /// Useful as a macro trigger: not a modifier, not a layer action.
    pub trigger: bool,
    pub tap_hold: TapHoldSupport,
    /// The token moves the keyboard to another layer.
    pub layer_action: bool,
    /// The token loads a profile.
    pub profile_action: bool,
    /// The token changes LED state or backlight brightness.
    pub led_action: bool,
}

impl TokenCaps {
    const PLAIN: TokenCaps = TokenCaps {
        modifier: false,
        trigger: true,
        tap_hold: TapHoldSupport::Unknown,
        layer_action: false,
        profile_action: false,
        led_action: false,
    };
    const MODIFIER: TokenCaps = TokenCaps {
        modifier: true,
        trigger: false,
        tap_hold: TapHoldSupport::Unknown,
        layer_action: false,
        profile_action: false,
        led_action: false,
    };
    const LAYER: TokenCaps = TokenCaps {
        modifier: false,
        trigger: false,
        tap_hold: TapHoldSupport::No,
        layer_action: true,
        profile_action: false,
        led_action: false,
    };
    const PROFILE: TokenCaps = TokenCaps {
        modifier: false,
        trigger: false,
        tap_hold: TapHoldSupport::No,
        layer_action: false,
        profile_action: true,
        led_action: false,
    };
    const LED: TokenCaps = TokenCaps {
        modifier: false,
        trigger: false,
        tap_hold: TapHoldSupport::No,
        layer_action: false,
        profile_action: false,
        led_action: true,
    };
    const NO_ACTION: TokenCaps = TokenCaps {
        modifier: false,
        trigger: false,
        tap_hold: TapHoldSupport::No,
        layer_action: false,
        profile_action: false,
        led_action: false,
    };
}

/// One entry of the official action token list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Token {
    /// The token exactly as printed by Kinesis.
    pub id: &'static str,
    /// USB usage id as printed (hex, no `0x`), or `-` for custom Kinesis tokens.
    pub usage: &'static str,
    pub group: TokenGroup,
    pub category: TokenCategory,
    /// The official "Usage Name" description.
    pub name: &'static str,
    pub caps: TokenCaps,
}

impl Token {
    /// True when this token is a modifier that the guide says can be a macro co-trigger.
    pub fn is_modifier(&self) -> bool {
        self.caps.modifier
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.id)
    }
}

macro_rules! tok {
    ($id:literal, $usage:literal, $group:ident, $cat:ident, $name:literal, $caps:ident) => {
        Token {
            id: $id,
            usage: $usage,
            group: TokenGroup::$group,
            category: TokenCategory::$cat,
            name: $name,
            caps: TokenCaps::$caps,
        }
    };
}

/// Canonical spelling accepted as a "no action" marker in the Kinesis documentation.
pub const NULL_TOKEN: &str = "null";

/// Every action token published in the official list.
///
/// Reserved USB usage ids (`A5`-`AF`) are deliberately absent: the official list marks them
/// `reserved` without a token, so no token can be produced for them.
pub static TOKENS: &[Token] = &[
    // ---- USB Keyboard/Keypad Page (0x0C): letters -------------------------------------
    tok!("a", "4", Keyboard, Letters, "keyboard a and A", PLAIN),
    tok!("b", "5", Keyboard, Letters, "keyboard b and B", PLAIN),
    tok!("c", "6", Keyboard, Letters, "keyboard c and C", PLAIN),
    tok!("d", "7", Keyboard, Letters, "keyboard d and D", PLAIN),
    tok!("e", "8", Keyboard, Letters, "keyboard e and E", PLAIN),
    tok!("f", "9", Keyboard, Letters, "keyboard f and F", PLAIN),
    tok!("g", "0A", Keyboard, Letters, "keyboard g and G", PLAIN),
    tok!("h", "0B", Keyboard, Letters, "keyboard h and H", PLAIN),
    tok!("i", "0C", Keyboard, Letters, "keyboard i and I", PLAIN),
    tok!("j", "0D", Keyboard, Letters, "keyboard j and J", PLAIN),
    tok!("k", "0E", Keyboard, Letters, "keyboard k and K", PLAIN),
    tok!("l", "0F", Keyboard, Letters, "keyboard l and L", PLAIN),
    tok!("m", "10", Keyboard, Letters, "keyboard m and M", PLAIN),
    tok!("n", "11", Keyboard, Letters, "keyboard n and N", PLAIN),
    tok!("o", "12", Keyboard, Letters, "keyboard o and O", PLAIN),
    tok!("p", "13", Keyboard, Letters, "keyboard p and P", PLAIN),
    tok!("q", "14", Keyboard, Letters, "keyboard q and Q", PLAIN),
    tok!("r", "15", Keyboard, Letters, "keyboard r and R", PLAIN),
    tok!("s", "16", Keyboard, Letters, "keyboard s and S", PLAIN),
    tok!("t", "17", Keyboard, Letters, "keyboard t and T", PLAIN),
    tok!("u", "18", Keyboard, Letters, "keyboard u and U", PLAIN),
    tok!("v", "19", Keyboard, Letters, "keyboard v and V", PLAIN),
    tok!("w", "1A", Keyboard, Letters, "keyboard w and W", PLAIN),
    tok!("x", "1B", Keyboard, Letters, "keyboard x and X", PLAIN),
    tok!("y", "1C", Keyboard, Letters, "keyboard y and Y", PLAIN),
    tok!("z", "1D", Keyboard, Letters, "keyboard z and Z", PLAIN),
    // ---- digits ----------------------------------------------------------------------
    tok!("1", "1E", Keyboard, Digits, "keyboard 1 and !", PLAIN),
    tok!("2", "1F", Keyboard, Digits, "keyboard 2 and @", PLAIN),
    tok!("3", "20", Keyboard, Digits, "keyboard 3 and #", PLAIN),
    tok!("4", "21", Keyboard, Digits, "keyboard 4 and $", PLAIN),
    tok!("5", "22", Keyboard, Digits, "keyboard 5 and %", PLAIN),
    tok!("6", "23", Keyboard, Digits, "keyboard 6 and ^", PLAIN),
    tok!("7", "24", Keyboard, Digits, "keyboard 7 and &", PLAIN),
    tok!("8", "25", Keyboard, Digits, "keyboard 8 and *", PLAIN),
    tok!("9", "26", Keyboard, Digits, "keyboard 9 and (", PLAIN),
    tok!("0", "27", Keyboard, Digits, "keyboard 0 and )", PLAIN),
    // ---- editing / control -----------------------------------------------------------
    tok!("ent", "28", Keyboard, Editing, "keyboard Return", PLAIN),
    tok!("esc", "29", Keyboard, Editing, "keyboard ESCAPE", PLAIN),
    tok!(
        "bspc",
        "2A",
        Keyboard,
        Editing,
        "keyboard DELETE (Backspace)",
        PLAIN
    ),
    tok!("tab", "2B", Keyboard, Editing, "keyboard Tab", PLAIN),
    tok!("spc", "2C", Keyboard, Editing, "keyboard Spacebar", PLAIN),
    tok!("caps", "39", Keyboard, Editing, "keyboard Caps Lock", PLAIN),
    tok!("ins", "49", Keyboard, Editing, "keyboard Insert", PLAIN),
    tok!("del", "4C", Keyboard, Editing, "keyboard Delete", PLAIN),
    tok!(
        "prnt",
        "46",
        Keyboard,
        Editing,
        "keyboard Print Screen",
        PLAIN
    ),
    tok!(
        "sclk",
        "47",
        Keyboard,
        Editing,
        "keyboard Scroll Lock",
        PLAIN
    ),
    tok!("paus", "48", Keyboard, Editing, "keyboard Pause", PLAIN),
    tok!(
        "app",
        "65",
        Keyboard,
        Editing,
        "keyboard Application (menu)",
        PLAIN
    ),
    tok!("powr", "66", Keyboard, Editing, "keyboard Power", PLAIN),
    // ---- punctuation (also the source of programming symbols) -------------------------
    tok!(
        "hyph",
        "2D",
        Keyboard,
        Punctuation,
        "keyboard - and _",
        PLAIN
    ),
    tok!(
        "eql",
        "2E",
        Keyboard,
        Punctuation,
        "keyboard = and +",
        PLAIN
    ),
    tok!(
        "obrk",
        "2F",
        Keyboard,
        Punctuation,
        "keyboard [ and {",
        PLAIN
    ),
    tok!(
        "cbrk",
        "30",
        Keyboard,
        Punctuation,
        "keyboard ] and }",
        PLAIN
    ),
    tok!(
        "bsls",
        "31",
        Keyboard,
        Punctuation,
        "keyboard \\ and |",
        PLAIN
    ),
    tok!(
        "int#",
        "32",
        Keyboard,
        Punctuation,
        "keyboard non-US # and ~",
        PLAIN
    ),
    tok!(
        "scol",
        "33",
        Keyboard,
        Punctuation,
        "keyboard ; and :",
        PLAIN
    ),
    tok!(
        "apos",
        "34",
        Keyboard,
        Punctuation,
        "keyboard ' and \"",
        PLAIN
    ),
    tok!(
        "grav",
        "35",
        Keyboard,
        Punctuation,
        "keyboard Grave Accent and Tilde",
        PLAIN
    ),
    tok!(
        "comm",
        "36",
        Keyboard,
        Punctuation,
        "keyboard , and <",
        PLAIN
    ),
    tok!(
        "perd",
        "37",
        Keyboard,
        Punctuation,
        "keyboard . and >",
        PLAIN
    ),
    tok!(
        "fsls",
        "38",
        Keyboard,
        Punctuation,
        "keyboard / and ?",
        PLAIN
    ),
    // ---- navigation ------------------------------------------------------------------
    tok!("home", "4A", Keyboard, Navigation, "keyboard Home", PLAIN),
    tok!(
        "pgup",
        "4B",
        Keyboard,
        Navigation,
        "keyboard Page Up",
        PLAIN
    ),
    tok!("end", "4D", Keyboard, Navigation, "keyboard End", PLAIN),
    tok!(
        "pgdn",
        "4E",
        Keyboard,
        Navigation,
        "keyboard Page Down",
        PLAIN
    ),
    tok!(
        "rght",
        "4F",
        Keyboard,
        Navigation,
        "keyboard Right Arrow",
        PLAIN
    ),
    tok!(
        "left",
        "50",
        Keyboard,
        Navigation,
        "keyboard Left Arrow",
        PLAIN
    ),
    tok!(
        "down",
        "51",
        Keyboard,
        Navigation,
        "keyboard Down Arrow",
        PLAIN
    ),
    tok!("up", "52", Keyboard, Navigation, "keyboard Up Arrow", PLAIN),
    // ---- function keys ---------------------------------------------------------------
    tok!("f1", "3A", Keyboard, FunctionKeys, "keyboard F1", PLAIN),
    tok!("f2", "3B", Keyboard, FunctionKeys, "keyboard F2", PLAIN),
    tok!("f3", "3C", Keyboard, FunctionKeys, "keyboard F3", PLAIN),
    tok!("f4", "3D", Keyboard, FunctionKeys, "keyboard F4", PLAIN),
    tok!("f5", "3E", Keyboard, FunctionKeys, "keyboard F5", PLAIN),
    tok!("f6", "3F", Keyboard, FunctionKeys, "keyboard F6", PLAIN),
    tok!("f7", "40", Keyboard, FunctionKeys, "keyboard F7", PLAIN),
    tok!("f8", "41", Keyboard, FunctionKeys, "keyboard F8", PLAIN),
    tok!("f9", "42", Keyboard, FunctionKeys, "keyboard F9", PLAIN),
    tok!("f10", "43", Keyboard, FunctionKeys, "keyboard F10", PLAIN),
    tok!("f11", "44", Keyboard, FunctionKeys, "keyboard F11", PLAIN),
    tok!("f12", "45", Keyboard, FunctionKeys, "keyboard F12", PLAIN),
    tok!("f13", "68", Keyboard, FunctionKeys, "keyboard F13", PLAIN),
    tok!("f14", "69", Keyboard, FunctionKeys, "keyboard F14", PLAIN),
    tok!("f15", "6A", Keyboard, FunctionKeys, "keyboard F15", PLAIN),
    tok!("f16", "6B", Keyboard, FunctionKeys, "keyboard F16", PLAIN),
    tok!("f17", "6C", Keyboard, FunctionKeys, "keyboard F17", PLAIN),
    tok!("f18", "6D", Keyboard, FunctionKeys, "keyboard F18", PLAIN),
    tok!("f19", "6E", Keyboard, FunctionKeys, "keyboard F19", PLAIN),
    tok!("f20", "6F", Keyboard, FunctionKeys, "keyboard F20", PLAIN),
    tok!("f21", "70", Keyboard, FunctionKeys, "keyboard F21", PLAIN),
    tok!("f22", "71", Keyboard, FunctionKeys, "keyboard F22", PLAIN),
    tok!("f23", "72", Keyboard, FunctionKeys, "keyboard F23", PLAIN),
    tok!("f24", "73", Keyboard, FunctionKeys, "keyboard F24", PLAIN),
    // ---- keypad ----------------------------------------------------------------------
    tok!(
        "nmlk",
        "53",
        Keyboard,
        Keypad,
        "keypad Num Lock and Clear",
        PLAIN
    ),
    tok!("kp/", "54", Keyboard, Keypad, "keypad /", PLAIN),
    tok!("kp*", "55", Keyboard, Keypad, "keypad *", PLAIN),
    tok!("kp-", "56", Keyboard, Keypad, "keypad -", PLAIN),
    tok!("kp+", "57", Keyboard, Keypad, "keypad +", PLAIN),
    tok!("kpen", "58", Keyboard, Keypad, "keypad Enter", PLAIN),
    tok!("kp1", "59", Keyboard, Keypad, "keypad 1 and End", PLAIN),
    tok!(
        "kp2",
        "5A",
        Keyboard,
        Keypad,
        "keypad 2 and Down Arrow",
        PLAIN
    ),
    tok!(
        "kp3",
        "5B",
        Keyboard,
        Keypad,
        "keypad 3 and Page Down",
        PLAIN
    ),
    tok!(
        "kp4",
        "5C",
        Keyboard,
        Keypad,
        "keypad 4 and Left Arrow",
        PLAIN
    ),
    tok!("kp5", "5D", Keyboard, Keypad, "keypad 5", PLAIN),
    tok!(
        "kp6",
        "5E",
        Keyboard,
        Keypad,
        "keypad 6 and Right Arrow",
        PLAIN
    ),
    tok!("kp7", "5F", Keyboard, Keypad, "keypad 7 and Home", PLAIN),
    tok!(
        "kp8",
        "60",
        Keyboard,
        Keypad,
        "keypad 8 and Up Arrow",
        PLAIN
    ),
    tok!("kp9", "61", Keyboard, Keypad, "keypad 9 and Page Up", PLAIN),
    tok!("kp0", "62", Keyboard, Keypad, "keypad 0 and Insert", PLAIN),
    tok!("kp.", "63", Keyboard, Keypad, "keypad . and Delete", PLAIN),
    tok!("kp=", "67", Keyboard, Keypad, "keypad =", PLAIN),
    tok!(
        "int\\",
        "64",
        Keyboard,
        International,
        "keyboard Non-US \\ and |",
        PLAIN
    ),
    tok!("kh85", "85", Keyboard, Keypad, "Keypad Comma", PLAIN),
    tok!("kh86", "86", Keyboard, Keypad, "Keypad Equal Sign", PLAIN),
    tok!("khB0", "B0", Keyboard, Keypad, "keypad 00", PLAIN),
    tok!("khB1", "B1", Keyboard, Keypad, "keypad 000", PLAIN),
    tok!("khB2", "B2", Keyboard, Keypad, "Thousands Separator", PLAIN),
    tok!("khB3", "B3", Keyboard, Keypad, "Decimal Separator", PLAIN),
    tok!("khB4", "B4", Keyboard, Keypad, "Currency Unit", PLAIN),
    tok!("khB5", "B5", Keyboard, Keypad, "Currency Sub-unit", PLAIN),
    tok!("khB6", "B6", Keyboard, Keypad, "keypad (", PLAIN),
    tok!("khB7", "B7", Keyboard, Keypad, "keypad )", PLAIN),
    tok!("khB8", "B8", Keyboard, Keypad, "keypad {", PLAIN),
    tok!("khB9", "B9", Keyboard, Keypad, "keypad }", PLAIN),
    tok!("khBA", "BA", Keyboard, Keypad, "keypad Tab", PLAIN),
    tok!("khBB", "BB", Keyboard, Keypad, "keypad Backspace", PLAIN),
    tok!("khBC", "BC", Keyboard, Keypad, "keypad A", PLAIN),
    tok!("khBD", "BD", Keyboard, Keypad, "keypad B", PLAIN),
    tok!("khBE", "BE", Keyboard, Keypad, "keypad C", PLAIN),
    tok!("khBF", "BF", Keyboard, Keypad, "keypad D", PLAIN),
    tok!("khC0", "C0", Keyboard, Keypad, "keypad E", PLAIN),
    tok!("khC1", "C1", Keyboard, Keypad, "keypad F", PLAIN),
    tok!("khC2", "C2", Keyboard, Keypad, "keypad XOR", PLAIN),
    tok!("khC3", "C3", Keyboard, Keypad, "keypad ^", PLAIN),
    tok!("khC4", "C4", Keyboard, Keypad, "keypad %", PLAIN),
    tok!("khC5", "C5", Keyboard, Keypad, "keypad <", PLAIN),
    tok!("khC6", "C6", Keyboard, Keypad, "keypad >", PLAIN),
    tok!("khC7", "C7", Keyboard, Keypad, "keypad &", PLAIN),
    tok!("khC8", "C8", Keyboard, Keypad, "keypad &&", PLAIN),
    tok!("khC9", "C9", Keyboard, Keypad, "keypad |", PLAIN),
    tok!("khCA", "CA", Keyboard, Keypad, "keypad ||", PLAIN),
    tok!("khCB", "CB", Keyboard, Keypad, "keypad :", PLAIN),
    tok!("khCC", "CC", Keyboard, Keypad, "keypad #", PLAIN),
    tok!("khCD", "CD", Keyboard, Keypad, "keypad Space", PLAIN),
    tok!("khCE", "CE", Keyboard, Keypad, "keypad @", PLAIN),
    tok!("khCF", "CF", Keyboard, Keypad, "keypad !", PLAIN),
    tok!("khD0", "D0", Keyboard, Keypad, "keypad Memory Store", PLAIN),
    tok!(
        "khD1",
        "D1",
        Keyboard,
        Keypad,
        "keypad Memory Recall",
        PLAIN
    ),
    tok!("khD2", "D2", Keyboard, Keypad, "keypad Memory Clear", PLAIN),
    tok!("khD3", "D3", Keyboard, Keypad, "keypad Memory Add", PLAIN),
    tok!(
        "khD4",
        "D4",
        Keyboard,
        Keypad,
        "keypad Memory Subtract",
        PLAIN
    ),
    tok!(
        "khD5",
        "D5",
        Keyboard,
        Keypad,
        "keypad Memory Multiply",
        PLAIN
    ),
    tok!(
        "khD6",
        "D6",
        Keyboard,
        Keypad,
        "keypad Memory Divide",
        PLAIN
    ),
    tok!("khD7", "D7", Keyboard, Keypad, "keypad +/-", PLAIN),
    tok!("khD8", "D8", Keyboard, Keypad, "keypad Clear", PLAIN),
    tok!("khD9", "D9", Keyboard, Keypad, "keypad Clear Entry", PLAIN),
    tok!("khDA", "DA", Keyboard, Keypad, "keypad Binary", PLAIN),
    tok!("khDB", "DB", Keyboard, Keypad, "keypad Octal", PLAIN),
    tok!("khDC", "DC", Keyboard, Keypad, "keypad Decimal", PLAIN),
    tok!("khDD", "DD", Keyboard, Keypad, "keypad Hexadecimal", PLAIN),
    // ---- modifiers -------------------------------------------------------------------
    tok!("lctr", "E0", Keyboard, Modifiers, "left control", MODIFIER),
    tok!("lshf", "E1", Keyboard, Modifiers, "left shift", MODIFIER),
    tok!("lalt", "E2", Keyboard, Modifiers, "left alt", MODIFIER),
    tok!(
        "lwin",
        "E3",
        Keyboard,
        Modifiers,
        "left GUI (Windows / Command)",
        MODIFIER
    ),
    tok!("rctr", "E4", Keyboard, Modifiers, "right control", MODIFIER),
    tok!("rshf", "E5", Keyboard, Modifiers, "right shift", MODIFIER),
    tok!("ralt", "E6", Keyboard, Modifiers, "right alt", MODIFIER),
    tok!(
        "rwin",
        "E7",
        Keyboard,
        Modifiers,
        "right GUI (Windows / Command)",
        MODIFIER
    ),
    // ---- remaining documented keyboard usage ids (Execute .. ExSel) --------------------
    tok!("kh74", "74", Keyboard, Internal, "keyboard Execute", PLAIN),
    tok!("kh75", "75", Keyboard, Internal, "keyboard Help", PLAIN),
    tok!("kh76", "76", Keyboard, Internal, "keyboard Menu", PLAIN),
    tok!("kh77", "77", Keyboard, Internal, "keyboard Select", PLAIN),
    tok!("kh78", "78", Keyboard, Internal, "keyboard Stop", PLAIN),
    tok!("kh79", "79", Keyboard, Internal, "keyboard Again", PLAIN),
    tok!("kh7A", "7A", Keyboard, Internal, "keyboard Undo", PLAIN),
    tok!("kh7B", "7B", Keyboard, Internal, "keyboard Cut", PLAIN),
    tok!("kh7C", "7C", Keyboard, Internal, "keyboard Copy", PLAIN),
    tok!("kh7D", "7D", Keyboard, Internal, "keyboard Paste", PLAIN),
    tok!("kh7E", "7E", Keyboard, Internal, "keyboard Find", PLAIN),
    tok!("kh7F", "7F", Keyboard, Internal, "keyboard Mute", PLAIN),
    tok!(
        "kh80",
        "80",
        Keyboard,
        Internal,
        "keyboard Volume Up",
        PLAIN
    ),
    tok!(
        "kh81",
        "81",
        Keyboard,
        Internal,
        "keyboard Volume Down",
        PLAIN
    ),
    tok!(
        "kh82",
        "82",
        Keyboard,
        Internal,
        "keyboard Locking Caps",
        PLAIN
    ),
    tok!(
        "kh83",
        "83",
        Keyboard,
        Internal,
        "keyboard Locking Num",
        PLAIN
    ),
    tok!("kh84", "84", Keyboard, Internal, "keyboard Locking", PLAIN),
    tok!(
        "int1",
        "87",
        Keyboard,
        International,
        "keyboard International1",
        PLAIN
    ),
    tok!(
        "int2",
        "88",
        Keyboard,
        International,
        "keyboard International2",
        PLAIN
    ),
    tok!(
        "int3",
        "89",
        Keyboard,
        International,
        "keyboard International3",
        PLAIN
    ),
    tok!(
        "int4",
        "8A",
        Keyboard,
        International,
        "keyboard International4",
        PLAIN
    ),
    tok!(
        "int5",
        "8B",
        Keyboard,
        International,
        "keyboard International5",
        PLAIN
    ),
    tok!(
        "int6",
        "8C",
        Keyboard,
        International,
        "keyboard International6",
        PLAIN
    ),
    tok!(
        "int7",
        "8D",
        Keyboard,
        International,
        "keyboard International7",
        PLAIN
    ),
    tok!(
        "int8",
        "8E",
        Keyboard,
        International,
        "keyboard International8",
        PLAIN
    ),
    tok!(
        "int9",
        "8F",
        Keyboard,
        International,
        "keyboard International9",
        PLAIN
    ),
    tok!(
        "lng1",
        "90",
        Keyboard,
        International,
        "keyboard LANG1",
        PLAIN
    ),
    tok!(
        "lng2",
        "91",
        Keyboard,
        International,
        "keyboard LANG2",
        PLAIN
    ),
    tok!(
        "lng3",
        "92",
        Keyboard,
        International,
        "keyboard LANG3",
        PLAIN
    ),
    tok!(
        "lng4",
        "93",
        Keyboard,
        International,
        "keyboard LANG4",
        PLAIN
    ),
    tok!(
        "lng5",
        "94",
        Keyboard,
        International,
        "keyboard LANG5",
        PLAIN
    ),
    tok!(
        "lng6",
        "95",
        Keyboard,
        International,
        "keyboard LANG6",
        PLAIN
    ),
    tok!(
        "lng7",
        "96",
        Keyboard,
        International,
        "keyboard LANG7",
        PLAIN
    ),
    tok!(
        "lng8",
        "97",
        Keyboard,
        International,
        "keyboard LANG8",
        PLAIN
    ),
    tok!(
        "lng9",
        "98",
        Keyboard,
        International,
        "keyboard LANG9",
        PLAIN
    ),
    tok!(
        "kh99",
        "99",
        Keyboard,
        Internal,
        "keyboard Alternate Erase",
        PLAIN
    ),
    tok!(
        "kh9A",
        "9A",
        Keyboard,
        Internal,
        "keyboard SysReq/Attention",
        PLAIN
    ),
    tok!("kh9B", "9B", Keyboard, Internal, "keyboard Cancel", PLAIN),
    tok!("kh9C", "9C", Keyboard, Internal, "keyboard Clear", PLAIN),
    tok!("kh9D", "9D", Keyboard, Internal, "keyboard Prior", PLAIN),
    tok!("kh9E", "9E", Keyboard, Internal, "keyboard Return", PLAIN),
    tok!(
        "kh9F",
        "9F",
        Keyboard,
        Internal,
        "keyboard Separator",
        PLAIN
    ),
    tok!("khA0", "A0", Keyboard, Internal, "keyboard Out", PLAIN),
    tok!("khA1", "A1", Keyboard, Internal, "keyboard Oper", PLAIN),
    tok!(
        "khA2",
        "A2",
        Keyboard,
        Internal,
        "keyboard Clear/Again",
        PLAIN
    ),
    tok!(
        "khA3",
        "A3",
        Keyboard,
        Internal,
        "keyboard CrSel/Props",
        PLAIN
    ),
    tok!("khA4", "A4", Keyboard, Internal, "keyboard ExSel", PLAIN),
    // ---- USB Consumer Page actions ----------------------------------------------------
    tok!("next", "B5", Consumer, Consumer, "scan next track", PLAIN),
    tok!(
        "prev",
        "B6",
        Consumer,
        Consumer,
        "scan previous track",
        PLAIN
    ),
    tok!("play", "B0", Consumer, Consumer, "play", PLAIN),
    tok!("stop", "B7", Consumer, Consumer, "stop", PLAIN),
    tok!("mute", "E2", Consumer, Consumer, "mute", PLAIN),
    tok!("vol+", "E9", Consumer, Consumer, "volume up", PLAIN),
    tok!("vol-", "EA", Consumer, Consumer, "volume down", PLAIN),
    tok!("fwrd", "B3", Consumer, Consumer, "forward", PLAIN),
    tok!("rewd", "B4", Consumer, Consumer, "rewind", PLAIN),
    tok!("cpau", "B1", Consumer, Consumer, "pause", PLAIN),
    tok!("ejct", "B8", Consumer, Consumer, "eject", PLAIN),
    tok!("plpa", "CD", Consumer, Consumer, "play/pause", PLAIN),
    tok!("recr", "B2", Consumer, Consumer, "record", PLAIN),
    tok!("ranp", "B9", Consumer, Consumer, "random play", PLAIN),
    tok!("plsk", "CE", Consumer, Consumer, "play/skip", PLAIN),
    tok!("calc", "192", Consumer, Consumer, "calculator", PLAIN),
    tok!("mmut", "E2", Consumer, Consumer, "microphone mute", PLAIN),
    // ---- custom Kinesis tokens: mouse ------------------------------------------------
    tok!(
        "lmou",
        "-",
        Kinesis,
        Mouse,
        "mouse button left click",
        PLAIN
    ),
    tok!(
        "rmou",
        "-",
        Kinesis,
        Mouse,
        "mouse button right click",
        PLAIN
    ),
    tok!(
        "mmou",
        "-",
        Kinesis,
        Mouse,
        "mouse button middle click",
        PLAIN
    ),
    tok!("4mou", "-", Kinesis, Mouse, "mouse button 4 click", PLAIN),
    tok!("5mou", "-", Kinesis, Mouse, "mouse button 5 click", PLAIN),
    tok!("sumo", "-", Kinesis, Mouse, "mouse scroll up", PLAIN),
    tok!("sdmo", "-", Kinesis, Mouse, "mouse scroll down", PLAIN),
    tok!("moul", "-", Kinesis, Mouse, "mouse move left", PLAIN),
    tok!("mour", "-", Kinesis, Mouse, "mouse move right", PLAIN),
    tok!("mouu", "-", Kinesis, Mouse, "mouse move up", PLAIN),
    tok!("moud", "-", Kinesis, Mouse, "mouse move down", PLAIN),
    // ---- custom Kinesis tokens: special ----------------------------------------------
    tok!("null", "-", Kinesis, Special, "no action", NO_ACTION),
    tok!(
        "ss",
        "-",
        Kinesis,
        Special,
        "SmartSet (the non-programmable gear key)",
        NO_ACTION
    ),
    tok!("mstp", "-", Kinesis, Special, "stop macro playback", PLAIN),
    tok!(
        "hypr",
        "-",
        Kinesis,
        Special,
        "hyper (Ctrl+Alt+Shift+Win chord)",
        PLAIN
    ),
    tok!(
        "meh",
        "-",
        Kinesis,
        Special,
        "meh (Ctrl+Alt+Shift chord)",
        PLAIN
    ),
    // ---- custom Kinesis tokens: eLED -------------------------------------------------
    tok!("ledt", "-", Kinesis, Led, "LED toggle on/off", LED),
    tok!("led+", "-", Kinesis, Led, "LED brightness up", LED),
    tok!("led-", "-", Kinesis, Led, "LED brightness down", LED),
    // ---- custom Kinesis tokens: layers -----------------------------------------------
    tok!("Defs", "-", Kinesis, Layer, "Default layer shift", LAYER),
    tok!("Deft", "-", Kinesis, Layer, "Default layer toggle", LAYER),
    tok!("keys", "-", Kinesis, Layer, "keypad layer shift", LAYER),
    tok!("keyt", "-", Kinesis, Layer, "keypad layer toggle", LAYER),
    tok!("fn1s", "-", Kinesis, Layer, "Fn1 layer shift", LAYER),
    tok!("fn1t", "-", Kinesis, Layer, "Fn1 layer toggle", LAYER),
    tok!("fn2s", "-", Kinesis, Layer, "Fn2 layer shift", LAYER),
    tok!("fn2t", "-", Kinesis, Layer, "Fn2 layer toggle", LAYER),
    tok!("fn3s", "-", Kinesis, Layer, "Fn3 layer shift", LAYER),
    tok!("fn3t", "-", Kinesis, Layer, "Fn3 layer toggle", LAYER),
    // ---- custom Kinesis tokens: profiles ---------------------------------------------
    tok!("pro0", "-", Kinesis, Profile, "load profile 0", PROFILE),
    tok!("pro1", "-", Kinesis, Profile, "load profile 1", PROFILE),
    tok!("pro2", "-", Kinesis, Profile, "load profile 2", PROFILE),
    tok!("pro3", "-", Kinesis, Profile, "load profile 3", PROFILE),
    tok!("pro4", "-", Kinesis, Profile, "load profile 4", PROFILE),
    tok!("pro5", "-", Kinesis, Profile, "load profile 5", PROFILE),
    tok!("pro6", "-", Kinesis, Profile, "load profile 6", PROFILE),
    tok!("pro7", "-", Kinesis, Profile, "load profile 7", PROFILE),
    tok!("pro8", "-", Kinesis, Profile, "load profile 8", PROFILE),
    tok!("pro9", "-", Kinesis, Profile, "load profile 9", PROFILE),
    // ---- custom Kinesis tokens: modifier chords --------------------------------------
    tok!(
        "caws",
        "-",
        Kinesis,
        Modifiers,
        "control + alt + win + shift",
        PLAIN
    ),
    tok!(
        "xaws",
        "-",
        Kinesis,
        Modifiers,
        "control + alt + win",
        PLAIN
    ),
    tok!(
        "cxws",
        "-",
        Kinesis,
        Modifiers,
        "control + win + shift",
        PLAIN
    ),
    tok!(
        "caxs",
        "-",
        Kinesis,
        Modifiers,
        "control + alt + shift",
        PLAIN
    ),
    tok!("cawx", "-", Kinesis, Modifiers, "alt + win + shift", PLAIN),
    tok!("caxx", "-", Kinesis, Modifiers, "control + alt", PLAIN),
    tok!("cxwx", "-", Kinesis, Modifiers, "control + win", PLAIN),
    tok!("cxxs", "-", Kinesis, Modifiers, "control + shift", PLAIN),
    tok!("xawx", "-", Kinesis, Modifiers, "alt + win", PLAIN),
    tok!("xaxs", "-", Kinesis, Modifiers, "alt + shift", PLAIN),
    tok!("xxws", "-", Kinesis, Modifiers, "win + shift", PLAIN),
];

/// Case-insensitive lookup of an action token.
///
/// Returns the catalog entry when the token is documented, `None` otherwise. Callers must treat
/// `None` as "unknown to this build", never as "safe to delete": an unknown token that already
/// exists in a file is preserved verbatim.
pub fn lookup(token: &str) -> Option<&'static Token> {
    let needle = token.trim();
    TOKENS.iter().find(|t| t.id.eq_ignore_ascii_case(needle))
}

/// True when the token is documented by the official list.
pub fn is_known(token: &str) -> bool {
    lookup(token).is_some()
}

/// The eight modifier tokens that can be co-triggers and can be emitted with `-` / `+` inside
/// macros (the guide's "downstrokes are indicated by placing a '-' inside the bracket").
pub const MODIFIER_TOKENS: &[&str] = &[
    "lctr", "lshf", "lalt", "lwin", "rctr", "rshf", "ralt", "rwin",
];

/// True when `token` names a left or right modifier key.
pub fn is_modifier(token: &str) -> bool {
    MODIFIER_TOKENS
        .iter()
        .any(|m| m.eq_ignore_ascii_case(token.trim()))
}

/// Free-text search used by both interfaces' action catalog.
///
/// Matches on the token id, the documented usage name and the usage id, case-insensitively.
/// An empty needle returns every token.
pub fn search(needle: &str) -> Vec<&'static Token> {
    let n = needle.trim().to_ascii_lowercase();
    if n.is_empty() {
        return TOKENS.iter().collect();
    }
    TOKENS
        .iter()
        .filter(|t| {
            t.id.to_ascii_lowercase().contains(&n)
                || t.name.to_ascii_lowercase().contains(&n)
                || t.usage.to_ascii_lowercase() == n
                || t.category.label().to_ascii_lowercase().contains(&n)
        })
        .collect()
}

/// Every token of a category, in catalog order.
pub fn by_category(category: TokenCategory) -> Vec<&'static Token> {
    TOKENS.iter().filter(|t| t.category == category).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_insensitive_as_documented() {
        assert_eq!(lookup("Q").map(|t| t.id), Some("q"));
        assert_eq!(lookup("DEFS").map(|t| t.id), Some("Defs"));
        assert_eq!(lookup("defs").map(|t| t.id), Some("Defs"));
        assert_eq!(lookup("kpen").map(|t| t.id), Some("kpen"));
    }

    #[test]
    fn unknown_tokens_are_not_invented() {
        assert!(lookup("not-a-token").is_none());
        assert!(!is_known("zzz"));
    }

    #[test]
    fn reserved_usage_ids_have_no_token() {
        // The official list marks A5..AF as "reserved" with no token at all.
        for reserved in [
            "A5", "A6", "A7", "A8", "A9", "AA", "AB", "AC", "AD", "AE", "AF",
        ] {
            assert!(
                TOKENS.iter().all(|t| t.usage != reserved),
                "usage {reserved} must not carry a token"
            );
        }
    }

    #[test]
    fn documented_examples_resolve() {
        // Example from the Direct Programming Guide 4.5 and 4.6.
        for token in [
            "q", "caps", "esc", "tab", "lctr", "hk1", "lshf", "h", "i", "vol+", "x",
        ] {
            // `hk1` is a *position* token, not an action token: it must not resolve here.
            if token == "hk1" {
                assert!(lookup(token).is_none());
            } else {
                assert!(
                    lookup(token).is_some(),
                    "{token} should be a documented action"
                );
            }
        }
    }

    #[test]
    fn modifiers_are_flagged() {
        for m in MODIFIER_TOKENS {
            assert!(lookup(m).unwrap().is_modifier(), "{m} must be a modifier");
            assert!(is_modifier(m));
        }
        assert!(!is_modifier("q"));
    }

    #[test]
    fn search_matches_names_and_ids() {
        assert!(search("volume").iter().any(|t| t.id == "vol+"));
        assert!(search("vol+").iter().any(|t| t.id == "vol+"));
        assert_eq!(search("").len(), TOKENS.len());
    }

    #[test]
    fn catalog_has_no_accidental_duplicate_ids() {
        let mut ids: Vec<String> = TOKENS.iter().map(|t| t.id.to_ascii_lowercase()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate token id in the catalog");
    }

    #[test]
    fn categories_are_all_reachable() {
        for c in TokenCategory::all() {
            assert!(
                TOKENS.iter().any(|t| t.category == *c),
                "category {} has no tokens",
                c.label()
            );
        }
    }
}
