//! Profiles and layers, exactly as the SmartSet firmware names them.

use serde::{Deserialize, Serialize};
use std::fmt;

/// One of the five documented layer headers.
///
/// The Direct Programming Guide §4.2: "Write your custom syntax under the desired 'Layer Header'
/// to assign it to one of the five layers: `<base>`, `<keypad>`, `<function1>`, `<function2>`,
/// `<function3>`". The header text is the token used in the file, and matching is
/// case-insensitive because the guide states that tokens are not case-sensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LayerId {
    Base,
    Keypad,
    Function1,
    Function2,
    Function3,
}

impl LayerId {
    /// All five layers in the order the guide lists them.
    pub fn all() -> &'static [LayerId] {
        &[
            LayerId::Base,
            LayerId::Keypad,
            LayerId::Function1,
            LayerId::Function2,
            LayerId::Function3,
        ]
    }

    /// The layer header as written in the configuration file, including the angle brackets.
    pub fn header(self) -> &'static str {
        match self {
            LayerId::Base => "<base>",
            LayerId::Keypad => "<keypad>",
            LayerId::Function1 => "<function1>",
            LayerId::Function2 => "<function2>",
            LayerId::Function3 => "<function3>",
        }
    }

    /// The layer name without brackets, for compact display.
    pub fn name(self) -> &'static str {
        match self {
            LayerId::Base => "base",
            LayerId::Keypad => "keypad",
            LayerId::Function1 => "function1",
            LayerId::Function2 => "function2",
            LayerId::Function3 => "function3",
        }
    }

    /// Short label used in tables and tab strips.
    pub fn short_label(self) -> &'static str {
        match self {
            LayerId::Base => "Base",
            LayerId::Keypad => "Keypad",
            LayerId::Function1 => "Fn1",
            LayerId::Function2 => "Fn2",
            LayerId::Function3 => "Fn3",
        }
    }

    /// How the layer LED reports this layer, from the manual §2.3 ("Layers> Base: Off, Kp: Blue,
    /// Fn1: White, Fn2: Green, Fn3: Red").
    pub fn led_color_hint(self) -> &'static str {
        match self {
            LayerId::Base => "LED off",
            LayerId::Keypad => "LED blue",
            LayerId::Function1 => "LED white",
            LayerId::Function2 => "LED green",
            LayerId::Function3 => "LED red",
        }
    }

    /// Parse a layer header line. Accepts the exact documented spelling with or without the
    /// angle brackets and in any case.
    pub fn parse(line: &str) -> Option<LayerId> {
        let t = line.trim();
        let inner = t
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(t);
        let inner = inner.trim();
        LayerId::all()
            .iter()
            .copied()
            .find(|l| l.name().eq_ignore_ascii_case(inner))
    }
}

impl fmt::Display for LayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short_label())
    }
}

/// A keyboard profile.
///
/// The manual §5.1: "The Advantage360 has ten available Profiles. Profiles 1-9 are fully
/// programmable, but 'Profile 0' is non-programmable". The guide §4.0 adds: "Profile 0 is
/// non-programmable and thus does not have a corresponding layout.txt file."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProfileId(pub u8);

impl ProfileId {
    /// Profile 0: the non-programmable factory profile.
    pub const ZERO: ProfileId = ProfileId(0);

    /// Every profile the firmware exposes.
    pub fn all() -> impl Iterator<Item = ProfileId> {
        (0u8..=9).map(ProfileId)
    }

    /// The nine programmable profiles.
    pub fn programmable() -> impl Iterator<Item = ProfileId> {
        (1u8..=9).map(ProfileId)
    }

    /// Profile 0 is readable and selectable but never writable.
    pub fn is_editable(self) -> bool {
        self.0 != 0
    }

    /// The file name this profile's layout is expected under. The reader never relies on this
    /// alone: it also discovers whatever `*.txt` files the v-Drive actually contains, so a
    /// differently named file is reported rather than ignored.
    pub fn layout_file_name(self) -> String {
        format!("layout{}.txt", self.0)
    }

    /// The file name this profile's indicator lighting is expected under.
    pub fn lighting_file_name(self) -> String {
        format!("led{}.txt", self.0)
    }

    /// Manual §2.3 default LED colour per profile.
    pub fn led_color_hint(self) -> &'static str {
        match self.0 {
            0 => "LED off",
            1 => "LED white",
            2 => "LED cyan",
            3 => "LED green",
            4 => "LED red",
            5 => "LED blue",
            6 => "LED yellow",
            7 => "LED orange",
            8 => "LED green-blue",
            9 => "LED pink",
            _ => "unknown",
        }
    }

    /// Parse `layout<N>.txt` (case-insensitive, with or without the extension).
    pub fn parse_layout_file_name(name: &str) -> Option<ProfileId> {
        parse_numbered_file(name, "layout").map(ProfileId)
    }

    /// Parse `led<N>.txt` (case-insensitive, with or without the extension).
    pub fn parse_lighting_file_name(name: &str) -> Option<ProfileId> {
        parse_numbered_file(name, "led").map(ProfileId)
    }
}

fn parse_numbered_file(name: &str, prefix: &str) -> Option<u8> {
    let stem = name
        .rsplit_once('.')
        .map(|(stem, _ext)| stem)
        .unwrap_or(name);
    let rest = stem.strip_prefix(prefix).or_else(|| {
        let lower = stem.to_ascii_lowercase();
        let p = prefix.to_ascii_lowercase();
        lower.strip_prefix(&p).map(|_| &stem[prefix.len()..])
    })?;
    let n: u8 = rest.parse().ok()?;
    if n <= 9 {
        Some(n)
    } else {
        None
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Profile {}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_headers_round_trip() {
        for l in LayerId::all() {
            assert_eq!(LayerId::parse(l.header()), Some(*l));
            assert_eq!(LayerId::parse(&l.header().to_uppercase()), Some(*l));
            assert_eq!(LayerId::parse(l.name()), Some(*l));
            assert_eq!(LayerId::parse(&format!("  {}  ", l.header())), Some(*l));
        }
        assert_eq!(LayerId::parse("<nope>"), None);
        assert_eq!(LayerId::parse(""), None);
    }

    #[test]
    fn profile_zero_is_not_editable() {
        assert!(!ProfileId::ZERO.is_editable());
        assert_eq!(ProfileId::programmable().count(), 9);
        assert_eq!(ProfileId::all().count(), 10);
        for p in ProfileId::programmable() {
            assert!(p.is_editable());
        }
    }

    #[test]
    fn file_names_parse_including_odd_casing() {
        assert_eq!(
            ProfileId::parse_layout_file_name("layout3.txt"),
            Some(ProfileId(3))
        );
        assert_eq!(
            ProfileId::parse_layout_file_name("Layout9.TXT"),
            Some(ProfileId(9))
        );
        assert_eq!(ProfileId::parse_layout_file_name("layout"), None);
        assert_eq!(ProfileId::parse_layout_file_name("layout10.txt"), None);
        assert_eq!(ProfileId::parse_layout_file_name("mybackup.txt"), None);
        assert_eq!(
            ProfileId::parse_lighting_file_name("led2.txt"),
            Some(ProfileId(2))
        );
        assert_eq!(ProfileId::parse_lighting_file_name("layout2.txt"), None);
    }

    #[test]
    fn expected_file_names_match_the_documented_scheme() {
        assert_eq!(ProfileId(4).layout_file_name(), "layout4.txt");
        assert_eq!(ProfileId(4).lighting_file_name(), "led4.txt");
    }
}
