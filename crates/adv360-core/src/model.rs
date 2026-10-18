//! The layout-file model.
//!
//! # Losslessness contract
//!
//! Every [`Line`] keeps the exact text it was read from (`raw`) together with its exact line
//! ending (`eol`). Serialising an unmodified file therefore reproduces the original bytes
//! character for character, including comments, undocumented instructions, ordering, blank
//! lines, CRLF endings, a UTF-8 BOM and a missing final newline.
//!
//! Only a line that was actually edited is re-rendered, and only that line changes. Nothing is
//! ever dropped: a line this build cannot interpret becomes [`LineKind::Unknown`] (preserved,
//! reported as a warning) or [`LineKind::Invalid`] (preserved, reported as an error that blocks
//! writing).

use serde::{Deserialize, Serialize};

use crate::layer::LayerId;
use crate::parse_error::ParseError;

/// A line ending, stored per line so mixed-ending files survive a round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Eol {
    Lf,
    CrLf,
    /// No terminator at all. Only legal on the final line.
    None,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::None => "",
        }
    }
}

/// Where a line came from. Used to distinguish "untouched, byte-exact" from "we rewrote this".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    /// Read from the file and never modified.
    Original,
    /// An existing line that the operator changed.
    Edited,
    /// A line this application added.
    Inserted,
}

/// A presumed comment.
///
/// The Direct Programming Guide documents **no** comment syntax. Lines starting with `#` or `//`
/// are recognised so that operators who already annotate their files keep those annotations, but
/// this classification is a convention of this tool, not a documented firmware feature. Such lines
/// are preserved verbatim and never interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommentStyle {
    Hash,
    SlashSlash,
}

impl CommentStyle {
    pub fn prefix(self) -> &'static str {
        match self {
            CommentStyle::Hash => "#",
            CommentStyle::SlashSlash => "//",
        }
    }
}

/// One action inside a square-bracket remap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemapAtom {
    /// A plain action token, e.g. `caps`.
    Action(String),
    /// The special Tap-and-Hold timing token `[t&hNNN]`.
    TapHold(u16),
}

impl RemapAtom {
    /// Render this atom exactly as the guide's syntax requires.
    pub fn render(&self) -> String {
        match self {
            RemapAtom::Action(a) => format!("[{a}]"),
            RemapAtom::TapHold(ms) => format!("[t&h{ms}]"),
        }
    }
}

/// A single-action remap or a Tap-and-Hold assignment:
/// `[position]>[action]` or `[position]>[tap][t&hNNN][hold]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemapLine {
    /// The position token exactly as written in the file.
    pub position: String,
    /// One or more action atoms. A plain remap has exactly one.
    pub atoms: Vec<RemapAtom>,
}

impl RemapLine {
    /// A plain `[position]>[action]` remap.
    pub fn single(position: impl Into<String>, action: impl Into<String>) -> Self {
        RemapLine {
            position: position.into(),
            atoms: vec![RemapAtom::Action(action.into())],
        }
    }

    /// A Tap-and-Hold remap: `[position]>[tap][t&hNNN][hold]`.
    pub fn tap_and_hold(
        position: impl Into<String>,
        tap: impl Into<String>,
        delay_ms: u16,
        hold: impl Into<String>,
    ) -> Self {
        RemapLine {
            position: position.into(),
            atoms: vec![
                RemapAtom::Action(tap.into()),
                RemapAtom::TapHold(delay_ms),
                RemapAtom::Action(hold.into()),
            ],
        }
    }

    /// The Tap-and-Hold delay, when this line uses one.
    pub fn tap_hold_delay(&self) -> Option<u16> {
        self.atoms.iter().find_map(|a| match a {
            RemapAtom::TapHold(ms) => Some(*ms),
            _ => None,
        })
    }

    /// Actions on the "tap" side (everything before the Tap-and-Hold token).
    pub fn tap_actions(&self) -> Vec<&str> {
        match self
            .atoms
            .iter()
            .position(|a| matches!(a, RemapAtom::TapHold(_)))
        {
            Some(idx) => self.atoms[..idx]
                .iter()
                .filter_map(|a| match a {
                    RemapAtom::Action(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect(),
            None => self
                .atoms
                .iter()
                .filter_map(|a| match a {
                    RemapAtom::Action(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect(),
        }
    }

    /// Actions on the "hold" side, when this line uses Tap-and-Hold.
    pub fn hold_actions(&self) -> Vec<&str> {
        match self
            .atoms
            .iter()
            .position(|a| matches!(a, RemapAtom::TapHold(_)))
        {
            Some(idx) => self.atoms[idx + 1..]
                .iter()
                .filter_map(|a| match a {
                    RemapAtom::Action(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect(),
            None => Vec::new(),
        }
    }

    /// Render the whole line content (without the disabled `*` prefix).
    pub fn render(&self) -> String {
        let actions: String = self.atoms.iter().map(RemapAtom::render).collect();
        format!("[{}]>{}", self.position, actions)
    }
}

/// A modifier stroke direction inside a macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stroke {
    /// Plain press, e.g. `{h}`.
    Press,
    /// Downstroke, e.g. `{-lshf}`.
    Down,
    /// Upstroke, e.g. `{+lshf}`.
    Up,
}

impl Stroke {
    /// True for the two prefixes the guide describes for modifier strokes.
    pub fn is_modifier_stroke(self) -> bool {
        matches!(self, Stroke::Down | Stroke::Up)
    }
}

/// One item inside a macro body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroAtom {
    /// `{sN}`: individual playback speed prefix, N in 1-9.
    Speed(u8),
    /// `{xN}`: macro multipaly prefix, N in 1-9.
    Multiplay(u8),
    /// `{dNNN}`: fixed delay in milliseconds.
    Delay(u16),
    /// `{dran}`: random delay.
    RandomDelay,
    /// An action or modifier stroke.
    Action { stroke: Stroke, token: String },
    /// A group this build does not recognise. Preserved verbatim.
    Unknown(String),
}

impl MacroAtom {
    pub fn render(&self) -> String {
        match self {
            MacroAtom::Speed(n) => format!("{{s{n}}}"),
            MacroAtom::Multiplay(n) => format!("{{x{n}}}"),
            MacroAtom::Delay(ms) => format!("{{d{ms:03}}}"),
            MacroAtom::RandomDelay => "{dran}".to_string(),
            MacroAtom::Action { stroke, token } => match stroke {
                Stroke::Press => format!("{{{token}}}"),
                Stroke::Down => format!("{{-{token}}}"),
                Stroke::Up => format!("{{+{token}}}"),
            },
            MacroAtom::Unknown(raw) => format!("{{{raw}}}"),
        }
    }
}

/// A macro: trigger keys to the left of `>`, body to the right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroLine {
    /// The primary (non-modifier) trigger token.
    pub trigger: String,
    /// Modifier co-triggers. The manual allows up to four.
    pub modifiers: Vec<String>,
    pub body: Vec<MacroAtom>,
}

impl MacroLine {
    /// Number of action tokens in the body (the guide's macro-size unit). Prefixes and delays are
    /// not actions.
    pub fn action_count(&self) -> usize {
        self.body
            .iter()
            .filter(|a| matches!(a, MacroAtom::Action { .. }))
            .count()
    }

    /// Render the whole line content (without the disabled `*` prefix).
    ///
    /// Co-trigger modifiers are written before the primary trigger, which is the order the
    /// Direct Programming Guide uses in its own example (`{lctr}{hk4}>{s9}...`).
    pub fn render(&self) -> String {
        let mut out = String::new();
        for m in &self.modifiers {
            out.push('{');
            out.push_str(m);
            out.push('}');
        }
        out.push('{');
        out.push_str(&self.trigger);
        out.push('}');
        out.push('>');
        for a in &self.body {
            out.push_str(&a.render());
        }
        out
    }
}

/// One LED indicator colour, `[RRR][GGG][BBB]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn render(self) -> String {
        format!("[{:03}][{:03}][{:03}]", self.r, self.g, self.b)
    }
}

/// The six LED indicators, from the guide §5.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Indicator(pub u8);

impl Indicator {
    /// `[IND1]` through `[IND6]`.
    pub fn token(self) -> String {
        format!("IND{}", self.0)
    }

    pub fn parse(token: &str) -> Option<Indicator> {
        let t = token.trim();
        let (prefix, digits) = t.split_at_checked(3)?;
        if !prefix.eq_ignore_ascii_case("ind") {
            return None;
        }
        let n: u8 = digits.trim().parse().ok()?;
        (1..=6).contains(&n).then_some(Indicator(n))
    }

    /// Where the indicator physically sits, per the guide §5.1.
    pub fn location(self) -> &'static str {
        match self.0 {
            1 => "Left module, left LED",
            2 => "Left module, middle LED",
            3 => "Left module, right LED",
            4 => "Right module, left LED",
            5 => "Right module, middle LED",
            6 => "Right module, right LED",
            _ => "unknown",
        }
    }
}

/// An LED indicator function, from the guide §5.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedFunction {
    /// `[null]` — disable the LED.
    Disable,
    /// `[prof]` — active profile.
    Profile,
    /// `[caps]` — Caps Lock on/off.
    CapsLock,
    /// `[nmlk]` — Num Lock on/off.
    NumLock,
    /// `[sclk]` — Scroll Lock on/off.
    ScrollLock,
    /// `[layd]`/`[layk]`/`[lay1]`/`[lay2]`/`[lay]` — one specific layer, or "active layer"
    /// when written as `[layX]` with the layer's own colour set.
    Layer(LayerId),
}

impl LedFunction {
    /// Parse an LED function token.
    ///
    /// Note the documentation ambiguity this function deliberately preserves: §5.2 lists
    /// `Base: [layd], Keypad: [layk], Fn: [lay1], Fn2: [lay2], Fn3: [lay]`. The Fn3 entry is
    /// printed as `lay` (not `lay3`) in the official guide. Both spellings are accepted; the
    /// literal documented spelling is what this tool generates.
    pub fn parse(token: &str) -> Option<LedFunction> {
        let t = token.trim();
        let eq = |s: &str| t.eq_ignore_ascii_case(s);
        Some(match t.to_ascii_lowercase().as_str() {
            _ if eq("null") => LedFunction::Disable,
            _ if eq("prof") => LedFunction::Profile,
            _ if eq("caps") => LedFunction::CapsLock,
            _ if eq("nmlk") => LedFunction::NumLock,
            _ if eq("sclk") => LedFunction::ScrollLock,
            _ if eq("layd") => LedFunction::Layer(LayerId::Base),
            _ if eq("layk") => LedFunction::Layer(LayerId::Keypad),
            _ if eq("lay1") => LedFunction::Layer(LayerId::Function1),
            _ if eq("lay2") => LedFunction::Layer(LayerId::Function2),
            // Documented spelling for Fn3, plus the obvious variant in case a firmware build
            // expects it. Both are preserved on read; `lay` is what is generated.
            _ if eq("lay") || eq("lay3") => LedFunction::Layer(LayerId::Function3),
            _ => return None,
        })
    }

    /// The token this tool writes for the function.
    pub fn token(self) -> &'static str {
        match self {
            LedFunction::Disable => "null",
            LedFunction::Profile => "prof",
            LedFunction::CapsLock => "caps",
            LedFunction::NumLock => "nmlk",
            LedFunction::ScrollLock => "sclk",
            LedFunction::Layer(LayerId::Base) => "layd",
            LedFunction::Layer(LayerId::Keypad) => "layk",
            LedFunction::Layer(LayerId::Function1) => "lay1",
            LedFunction::Layer(LayerId::Function2) => "lay2",
            LedFunction::Layer(LayerId::Function3) => "lay",
        }
    }
}

/// `[INDn]>[FUNC][RRR][GGG][BBB]` — optionally repeated for the layer LED, which the guide says
/// "supports the assignment of up to 5 colors, one for each layer".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedLine {
    pub indicator: Indicator,
    pub function: LedFunction,
    /// One colour for every function except the layer function, which may carry up to five.
    pub colors: Vec<Rgb>,
}

impl LedLine {
    pub fn render(&self) -> String {
        let colors: String = self.colors.iter().map(|c| c.render()).collect();
        format!(
            "[{}]>[{}]{}",
            self.indicator.token(),
            self.function.token(),
            colors
        )
    }
}

/// What a line turned out to be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineKind {
    /// An empty or whitespace-only line.
    Blank,
    /// A presumed comment (`#` or `//`). Undocumented by Kinesis; preserved verbatim.
    Comment { style: CommentStyle },
    /// One of the five documented layer headers.
    LayerHeader(LayerId),
    /// `[pos]>[action]` or a Tap-and-Hold assignment.
    Remap(RemapLine),
    /// `{trigger}>{actions}`.
    Macro(MacroLine),
    /// `[INDn]>[FUNC][RRR][GGG][BBB]`.
    Led(LedLine),
    /// A line that is not documented syntax and not malformed either. Preserved verbatim and
    /// reported as a warning, never removed.
    Unknown { reason: String },
    /// A line that starts like documented syntax but is malformed. Preserved verbatim and
    /// reported as an error that blocks writing.
    Invalid { error: ParseError },
}

impl LineKind {
    /// A short, human-readable description used by both interfaces.
    pub fn describe(&self) -> String {
        match self {
            LineKind::Blank => "blank".into(),
            LineKind::Comment { style } => format!("comment ({})", style.prefix()),
            LineKind::LayerHeader(l) => format!("layer header {}", l.header()),
            LineKind::Remap(r) => format!("remap of [{}]", r.position),
            LineKind::Macro(m) => format!("macro trigger {{{}}}", m.trigger),
            LineKind::Led(l) => format!("LED {}", l.indicator.token()),
            LineKind::Unknown { reason } => format!("unrecognised: {reason}"),
            LineKind::Invalid { error } => format!("invalid: {error}"),
        }
    }

    /// True when the operator can change this line from the key editor.
    pub fn is_editable_structure(&self) -> bool {
        matches!(
            self,
            LineKind::Remap(_) | LineKind::Macro(_) | LineKind::Led(_)
        )
    }
}

/// One line of a configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// The exact original text, without the line terminator. This is what gets written back.
    pub raw: String,
    /// The exact terminator that followed the line.
    pub eol: Eol,
    pub kind: LineKind,
    /// `false` when the line is disabled with the documented leading `*`.
    pub enabled: bool,
    /// The layer this line belongs to, resolved from the nearest preceding layer header.
    /// A header line itself is `Some(its own layer)`.
    pub layer: Option<LayerId>,
    pub origin: Origin,
}

impl Line {
    /// Build a brand-new line that this application is adding to a file.
    pub fn inserted(content: String, eol: Eol, kind: LineKind, layer: Option<LayerId>) -> Line {
        Line {
            raw: content,
            eol,
            kind,
            enabled: true,
            layer,
            origin: Origin::Inserted,
        }
    }

    /// The line exactly as it will be written, including the disabled marker when present.
    ///
    /// The raw text is authoritative: an unmodified line is written back byte for byte, with
    /// whatever leading whitespace, casing and disabled marker it originally had.
    pub fn rendered(&self) -> &str {
        &self.raw
    }

    /// Serialise this line including its terminator.
    pub fn to_file_text(&self) -> String {
        let mut s = self.raw.clone();
        s.push_str(self.eol.as_str());
        s
    }

    /// True when this line is configured but switched off with `*`.
    pub fn is_disabled(&self) -> bool {
        !self.enabled
    }

    /// The line content with the disabled marker removed.
    pub fn content(&self) -> &str {
        let trimmed = self.raw.trim_start();
        match trimmed.strip_prefix('*') {
            Some(rest) => rest.trim_start(),
            None => &self.raw,
        }
    }

    /// Replace the content of the line, keeping the disabled state. Marks the line as edited.
    pub fn set_content(&mut self, content: String, kind: LineKind, layer: Option<LayerId>) {
        self.raw = if self.enabled {
            content
        } else {
            format!("*{content}")
        };
        self.kind = kind;
        self.layer = layer;
        self.origin = Origin::Edited;
    }

    /// Switch the line on or off with the documented leading `*`. Marks it as edited when the
    /// state actually changes.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        let content = self.content().to_string();
        self.enabled = enabled;
        self.raw = if enabled {
            content
        } else {
            format!("*{content}")
        };
        if self.origin == Origin::Original {
            self.origin = Origin::Edited;
        }
    }
}

/// Which role a discovered `.txt` file plays on the v-Drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileRole {
    /// `layouts/layout<N>.txt` — a profile layout that can be edited and written back.
    ProfileLayout,
    /// A `.txt` file in `layouts/` that is not named `layout<N>.txt`. The guide §4.1 says such
    /// files "cannot be loaded to the keyboard without first renaming", so they are readable and
    /// exportable but never written back.
    BackupLayout,
    /// `lighting/led<N>.txt` — the indicator configuration.
    ProfileLighting,
    /// `settings.txt`. The guide says "Do not edit the settings.txt file": read-only, always.
    Settings,
    /// Anything else found on the v-Drive.
    Other,
}

impl FileRole {
    pub fn is_writable(self) -> bool {
        matches!(self, FileRole::ProfileLayout)
    }

    pub fn label(self) -> &'static str {
        match self {
            FileRole::ProfileLayout => "profile layout",
            FileRole::BackupLayout => "backup layout (not loadable without renaming)",
            FileRole::ProfileLighting => "indicator lighting",
            FileRole::Settings => "settings (never modified by this tool)",
            FileRole::Other => "other",
        }
    }
}

/// A parsed configuration file, losslessly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFile {
    /// Path inside the v-Drive, repository-relative to the v-Drive root.
    pub relative_path: String,
    pub role: FileRole,
    /// The profile this file belongs to, when its name carries one.
    pub profile: Option<crate::layer::ProfileId>,
    /// True when the file began with a UTF-8 BOM.
    pub bom: bool,
    pub lines: Vec<Line>,
    /// Set when the file is byte-identical to what was read from disk.
    pub dirty: bool,
}

impl ConfigFile {
    /// Serialise the whole file exactly as it will be written to disk.
    pub fn render(&self) -> String {
        let mut s = String::new();
        if self.bom {
            s.push('\u{feff}');
        }
        for l in &self.lines {
            s.push_str(&l.to_file_text());
        }
        s
    }

    /// True when any line ending in the file is CRLF.
    pub fn has_crlf(&self) -> bool {
        self.lines.iter().any(|l| l.eol == Eol::CrLf)
    }

    /// True when the line endings are not uniform.
    pub fn has_mixed_eol(&self) -> bool {
        let has_lf = self.lines.iter().any(|l| l.eol == Eol::Lf);
        has_lf && self.has_crlf()
    }

    /// True when the file does not end with a line terminator.
    pub fn lacks_final_newline(&self) -> bool {
        self.lines.last().is_some_and(|l| l.eol == Eol::None)
    }

    /// Every line of a layer, with its index.
    pub fn lines_of_layer(&self, layer: LayerId) -> Vec<(usize, &Line)> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.layer == Some(layer))
            .collect()
    }

    /// The index of a layer's header line.
    pub fn layer_header_index(&self, layer: LayerId) -> Option<usize> {
        self.lines
            .iter()
            .position(|l| l.kind == LineKind::LayerHeader(layer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_renders_like_the_guide_examples() {
        // Guide §4.5: "Hotkey 1 performs Q: [hk1]>[q]".
        assert_eq!(RemapLine::single("hk1", "q").render(), "[hk1]>[q]");
        // Guide §4.7: "[caps]>[caps][t&h500][esc]".
        assert_eq!(
            RemapLine::tap_and_hold("caps", "caps", 500, "esc").render(),
            "[caps]>[caps][t&h500][esc]"
        );
    }

    #[test]
    fn macro_renders_like_the_guide_examples() {
        // Guide §4.6 example 1: "{tab}>{-lshf}{h}{+lshf}{i}".
        let m = MacroLine {
            trigger: "tab".into(),
            modifiers: vec![],
            body: vec![
                MacroAtom::Action {
                    stroke: Stroke::Down,
                    token: "lshf".into(),
                },
                MacroAtom::Action {
                    stroke: Stroke::Press,
                    token: "h".into(),
                },
                MacroAtom::Action {
                    stroke: Stroke::Up,
                    token: "lshf".into(),
                },
                MacroAtom::Action {
                    stroke: Stroke::Press,
                    token: "i".into(),
                },
            ],
        };
        assert_eq!(m.render(), "{tab}>{-lshf}{h}{+lshf}{i}");

        // Guide §4.6 example 2: "{lctr}{hk4}>{s9}{q}{w}{e}{r}{t}{y}".
        let m = MacroLine {
            trigger: "hk4".into(),
            modifiers: vec!["lctr".into()],
            body: vec![
                MacroAtom::Speed(9),
                MacroAtom::Action {
                    stroke: Stroke::Press,
                    token: "q".into(),
                },
                MacroAtom::Action {
                    stroke: Stroke::Press,
                    token: "w".into(),
                },
            ],
        };
        assert_eq!(m.render(), "{lctr}{hk4}>{s9}{q}{w}");

        // Guide §4.6 example 3: "{hk1}>{x3}{vol+}".
        let m = MacroLine {
            trigger: "hk1".into(),
            modifiers: vec![],
            body: vec![
                MacroAtom::Multiplay(3),
                MacroAtom::Action {
                    stroke: Stroke::Press,
                    token: "vol+".into(),
                },
            ],
        };
        assert_eq!(m.render(), "{hk1}>{x3}{vol+}");

        // Delays: "{d001}" .. "{d999}" and "{dran}".
        assert_eq!(MacroAtom::Delay(1).render(), "{d001}");
        assert_eq!(MacroAtom::Delay(999).render(), "{d999}");
        assert_eq!(MacroAtom::RandomDelay.render(), "{dran}");
    }

    #[test]
    fn led_line_renders_like_the_guide_syntax() {
        let l = LedLine {
            indicator: Indicator(1),
            function: LedFunction::CapsLock,
            colors: vec![Rgb { r: 255, g: 0, b: 0 }],
        };
        assert_eq!(l.render(), "[IND1]>[caps][255][000][000]");
    }

    #[test]
    fn indicator_parsing_is_forgiving_about_case() {
        assert_eq!(Indicator::parse("IND3"), Some(Indicator(3)));
        assert_eq!(Indicator::parse("ind6"), Some(Indicator(6)));
        assert_eq!(Indicator::parse("IND7"), None);
        assert_eq!(Indicator::parse("IND0"), None);
        assert_eq!(Indicator::parse("XND1"), None);
    }

    #[test]
    fn led_function_accepts_the_documented_fn3_spelling_and_the_variant() {
        assert_eq!(
            LedFunction::parse("lay"),
            Some(LedFunction::Layer(LayerId::Function3))
        );
        assert_eq!(
            LedFunction::parse("lay3"),
            Some(LedFunction::Layer(LayerId::Function3))
        );
        assert_eq!(
            LedFunction::parse("layd"),
            Some(LedFunction::Layer(LayerId::Base))
        );
        assert_eq!(
            LedFunction::parse("layk"),
            Some(LedFunction::Layer(LayerId::Keypad))
        );
        assert_eq!(
            LedFunction::parse("lay1"),
            Some(LedFunction::Layer(LayerId::Function1))
        );
        assert_eq!(
            LedFunction::parse("lay2"),
            Some(LedFunction::Layer(LayerId::Function2))
        );
        // The token this tool generates for Fn3 is the literal one from the guide.
        assert_eq!(LedFunction::Layer(LayerId::Function3).token(), "lay");
        assert_eq!(LedFunction::parse("bogus"), None);
    }

    #[test]
    fn tap_and_hold_sides_split_correctly() {
        let r = RemapLine::tap_and_hold("caps", "caps", 250, "esc");
        assert_eq!(r.tap_actions(), vec!["caps"]);
        assert_eq!(r.hold_actions(), vec!["esc"]);
        assert_eq!(r.tap_hold_delay(), Some(250));

        let plain = RemapLine::single("q", "w");
        assert_eq!(plain.tap_actions(), vec!["w"]);
        assert!(plain.hold_actions().is_empty());
        assert_eq!(plain.tap_hold_delay(), None);
    }

    #[test]
    fn a_disabled_line_is_written_back_byte_for_byte() {
        let l = Line {
            raw: "*[q]>[w]".into(),
            eol: Eol::Lf,
            kind: LineKind::Remap(RemapLine::single("q", "w")),
            enabled: false,
            layer: Some(LayerId::Base),
            origin: Origin::Original,
        };
        assert_eq!(l.rendered(), "*[q]>[w]");
        assert_eq!(l.to_file_text(), "*[q]>[w]\n");
        assert!(l.is_disabled());
        assert_eq!(l.content(), "[q]>[w]");
    }

    #[test]
    fn toggling_the_disabled_marker_rewrites_only_that_line() {
        let mut l = Line {
            raw: "[q]>[w]".into(),
            eol: Eol::Lf,
            kind: LineKind::Remap(RemapLine::single("q", "w")),
            enabled: true,
            layer: Some(LayerId::Base),
            origin: Origin::Original,
        };
        l.set_enabled(false);
        assert_eq!(l.rendered(), "*[q]>[w]");
        l.set_enabled(true);
        assert_eq!(l.rendered(), "[q]>[w]");
        assert_eq!(l.origin, Origin::Edited);

        // The guide puts the '*' at the very beginning of the line, so any leading whitespace the
        // file already had is kept behind it rather than rewritten away.
        let mut spaced = Line {
            raw: "   [q]>[w]".into(),
            eol: Eol::Lf,
            kind: LineKind::Remap(RemapLine::single("q", "w")),
            enabled: true,
            layer: Some(LayerId::Base),
            origin: Origin::Original,
        };
        spaced.set_enabled(false);
        assert_eq!(spaced.rendered(), "*   [q]>[w]");
        assert_eq!(spaced.content(), "[q]>[w]");
    }

    #[test]
    fn editing_content_keeps_the_disabled_state() {
        let mut l = Line {
            raw: "*[q]>[w]".into(),
            eol: Eol::Lf,
            kind: LineKind::Remap(RemapLine::single("q", "w")),
            enabled: false,
            layer: Some(LayerId::Base),
            origin: Origin::Original,
        };
        let new = RemapLine::single("q", "e");
        l.set_content(new.render(), LineKind::Remap(new), Some(LayerId::Base));
        assert_eq!(l.rendered(), "*[q]>[e]");
        assert_eq!(l.origin, Origin::Edited);
    }

    #[test]
    fn crlf_and_missing_final_newline_are_detected() {
        let f = ConfigFile {
            relative_path: "layouts/layout1.txt".into(),
            role: FileRole::ProfileLayout,
            profile: Some(crate::layer::ProfileId(1)),
            bom: true,
            lines: vec![
                Line {
                    raw: "<base>".into(),
                    eol: Eol::CrLf,
                    kind: LineKind::LayerHeader(LayerId::Base),
                    enabled: true,
                    layer: Some(LayerId::Base),
                    origin: Origin::Original,
                },
                Line {
                    raw: "[q]>[w]".into(),
                    eol: Eol::None,
                    kind: LineKind::Unknown {
                        reason: "test".into(),
                    },
                    enabled: true,
                    layer: Some(LayerId::Base),
                    origin: Origin::Original,
                },
            ],
            dirty: false,
        };
        assert!(f.has_crlf());
        assert!(!f.has_mixed_eol());
        assert!(f.lacks_final_newline());
        assert_eq!(f.render(), "\u{feff}<base>\r\n[q]>[w]");
    }

    #[test]
    fn macro_action_count_ignores_prefixes_and_delays() {
        let m = MacroLine {
            trigger: "tab".into(),
            modifiers: vec![],
            body: vec![
                MacroAtom::Speed(9),
                MacroAtom::Delay(50),
                MacroAtom::RandomDelay,
                MacroAtom::Action {
                    stroke: Stroke::Press,
                    token: "a".into(),
                },
                MacroAtom::Action {
                    stroke: Stroke::Press,
                    token: "b".into(),
                },
            ],
        };
        assert_eq!(m.action_count(), 2);
    }
}
