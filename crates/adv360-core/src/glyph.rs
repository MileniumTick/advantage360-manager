//! Short display glyphs for action tokens.
//!
//! Both interfaces need to draw a token inside a small keycap. The glyph is presentation only: it
//! never appears in a configuration file.

/// A short glyph for an action token, suitable for a keycap.
///
/// Unknown tokens are returned unchanged, so nothing is hidden from the operator.
pub fn glyph(token: &str) -> String {
    let t = token.trim();
    let lower = t.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        // modifiers
        "lctr" | "rctr" => "Ctrl",
        "lshf" | "rshf" => "Shift",
        "lalt" | "ralt" => "Alt",
        "lwin" | "rwin" => "Win",
        // editing
        "ent" => "\u{23ce}",
        "esc" => "Esc",
        "bspc" => "\u{232b}",
        "tab" => "\u{21e5}",
        "spc" => "Space",
        "caps" => "Caps",
        "del" => "Del",
        "ins" => "Ins",
        "prnt" => "PrtSc",
        "sclk" => "ScrLk",
        "paus" => "Pause",
        "app" => "Menu",
        "powr" => "Power",
        // navigation
        "home" => "Home",
        "end" => "End",
        "pgup" => "PgUp",
        "pgdn" => "PgDn",
        "left" => "\u{2190}",
        "rght" => "\u{2192}",
        "up" => "\u{2191}",
        "down" => "\u{2193}",
        // punctuation, as printed on a US keycap
        "hyph" => "-",
        "eql" => "=",
        "obrk" => "[",
        "cbrk" => "]",
        "bsls" => "\\",
        "int#" => "#",
        "scol" => ";",
        "apos" => "'",
        "grav" => "`",
        "comm" => ",",
        "perd" => ".",
        "fsls" => "/",
        "int\\" => "\\",
        // keypad
        "nmlk" => "NumLk",
        "kpen" => "KP\u{23ce}",
        "kp/" => "KP/",
        "kp*" => "KP*",
        "kp-" => "KP-",
        "kp+" => "KP+",
        "kp." => "KP.",
        "kp=" => "KP=",
        // consumer
        "vol+" => "Vol+",
        "vol-" => "Vol-",
        "mute" => "Mute",
        "mmut" => "Mic\u{207f}",
        "next" => "Next",
        "prev" => "Prev",
        "play" => "Play",
        "stop" => "Stop",
        "plpa" => "Pl/Pa",
        "plsk" => "Pl/Sk",
        "cpau" => "Pause",
        "ejct" => "Eject",
        "fwrd" => "Fwd",
        "rewd" => "Rew",
        "recr" => "Rec",
        "ranp" => "Rnd",
        "calc" => "Calc",
        // mouse
        "lmou" => "M\u{2190}", // left click
        "rmou" => "M\u{2192}", // right click
        "mmou" => "M\u{25CF}",
        "4mou" => "M4",
        "5mou" => "M5",
        "sumo" => "MWhl\u{2191}",
        "sdmo" => "MWhl\u{2193}",
        "moul" => "M\u{2190}\u{2190}",
        "mour" => "M\u{2192}\u{2192}",
        "mouu" => "M\u{2191}\u{2191}",
        "moud" => "M\u{2193}\u{2193}",
        // kinesis specials
        "null" => "\u{2205}",
        "ss" => "\u{2699}",
        "ledt" => "LED",
        "led+" => "LED+",
        "led-" => "LED-",
        "mstp" => "MacStop",
        "hypr" => "Hyper",
        "meh" => "Meh",
        // layers
        "defs" => "Def\u{21e7}",
        "deft" => "Def\u{21ea}",
        "keys" => "Kp\u{21e7}",
        "keyt" => "Kp\u{21ea}",
        "fn1s" => "Fn1\u{21e7}",
        "fn1t" => "Fn1\u{21ea}",
        "fn2s" => "Fn2\u{21e7}",
        "fn2t" => "Fn2\u{21ea}",
        "fn3s" => "Fn3\u{21e7}",
        "fn3t" => "Fn3\u{21ea}",
        _ => "",
    };
    if !mapped.is_empty() {
        return mapped.to_string();
    }

    // Function keys and keypad digits read better uppercased.
    if let Some(rest) = lower.strip_prefix('f') {
        if rest.parse::<u8>().is_ok() {
            return lower.to_uppercase();
        }
    }
    if let Some(rest) = lower.strip_prefix("kp") {
        if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_digit()) {
            return format!("KP{rest}");
        }
    }
    if let Some(rest) = lower.strip_prefix("kh") {
        if !rest.is_empty() {
            return format!("kh{rest}");
        }
    }
    if let Some(rest) = lower.strip_prefix("pro") {
        if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_digit()) {
            return format!("P{rest}");
        }
    }
    if t.len() == 1 {
        return t.to_uppercase();
    }
    t.to_string()
}

/// The long, human description of a token: the official usage name when the catalog knows it,
/// otherwise the token itself.
pub fn describe(token: &str) -> String {
    match crate::tokens::lookup(token) {
        Some(t) => t.name.to_string(),
        None => token.to_string(),
    }
}

/// A glyph for a *position* token (the physical key), used when the key has no custom action.
pub fn position_glyph(token: &str) -> String {
    match crate::positions::lookup(token) {
        Some(p) if !p.keycap.is_empty() => p.keycap.to_string(),
        _ => token.to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tokens_have_readable_glyphs() {
        assert_eq!(glyph("ent"), "\u{23ce}");
        assert_eq!(glyph("spc"), "Space");
        assert_eq!(glyph("eql"), "=");
        assert_eq!(glyph("kp7"), "KP7");
        assert_eq!(glyph("f12"), "F12");
        assert_eq!(glyph("null"), "\u{2205}");
        assert_eq!(glyph("q"), "Q");
        assert_eq!(glyph("pro3"), "P3");
    }

    #[test]
    fn unknown_tokens_are_shown_unchanged() {
        assert_eq!(glyph("weird-thing"), "weird-thing");
        assert_eq!(describe("weird-thing"), "weird-thing");
    }

    #[test]
    fn descriptions_come_from_the_official_list() {
        assert_eq!(describe("caps"), "keyboard Caps Lock");
        assert_eq!(describe("kp0"), "keypad 0 and Insert");
        assert!(describe("vol+").contains("volume up"));
        assert_eq!(describe("fn1s"), "Fn1 layer shift");
    }

    #[test]
    fn position_glyphs_use_the_printed_keycap() {
        assert_eq!(position_glyph("eql"), "=");
        assert_eq!(position_glyph("bspc"), "Backspace");
        assert_eq!(position_glyph("kp"), "Kp Toggle");
        assert_eq!(position_glyph("left"), "\u{2190}");
        assert_eq!(position_glyph("ss"), "(gear icon)");
    }
}
