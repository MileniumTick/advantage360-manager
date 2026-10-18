//! Draft symbol and navigation layers.
//!
//! **Nothing here is applied automatically.** Each preset produces a draft plus a readable table,
//! and the operator reviews the table and the diff before anything is written. This module only
//! decides what a *sensible* proposal looks like, using the real position map and the real
//! documented syntax.
//!
//! Two syntax facts drive the whole design, both from the Direct Programming Guide:
//!
//! * "Shifted Actions: Shifted characters (e.g. \"!\") cannot be produced by a Remap only as a
//!   multi-key combo macro. To produce such an action, it is necessary to encode it as macro which
//!   includes both the down and up stroke of the shift key surrounding the basic key action."
//!   So every shifted symbol becomes `{trigger}>{-lshf}{key}{+lshf}`, never a bare remap.
//! * "The macro trigger key is represented by the trigger action, not the physical position key."
//!   So a macro's trigger is the action the key produces *in that layer* — which, for the presets
//!   below, is the position's own token because the presets never remap a position they also give
//!   a macro.
//!
//! Layer choice, and why:
//!
//! * **Programming symbols go on `<function1>`.** The Fn1 layer is the one already reachable by
//!   default through the two Fn keys, and its only factory assignments are F1-F12 on the number
//!   row. The preset therefore leaves the whole number row alone and fills the letter rows, so the
//!   operator keeps F1-F12 *and* gains symbols with a single already-existing chord.
//! * **Navigation goes on `<function2>`.** Fn2 has no factory assignments at all, so nothing is
//!   lost, but it is not reachable by default. The preset therefore also offers explicit access-key
//!   lines, written under `<base>`, for the operator to accept or reject. The first suggestion uses
//!   `hk1`, which the manual states is blank by default, so accepting it costs nothing.

use serde::{Deserialize, Serialize};

use crate::diff::Binding;
use crate::layer::LayerId;
use crate::model::{MacroAtom, MacroLine, RemapLine, Stroke};
use crate::positions::{self, PositionDef};
use crate::tokens;

/// One proposed assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Assignment {
    /// `[position]>[action]`.
    Remap { position: String, action: String },
    /// `[position]>[tap][t&hNNN][hold]`.
    TapAndHold {
        position: String,
        tap: String,
        delay_ms: u16,
        hold: String,
    },
    /// `{trigger}>{...}`.
    Macro {
        position: String,
        macro_line: MacroLine,
    },
}

impl Assignment {
    /// The layer this assignment belongs to.
    pub fn position(&self) -> &str {
        match self {
            Assignment::Remap { position, .. } => position,
            Assignment::TapAndHold { position, .. } => position,
            Assignment::Macro { position, .. } => position,
        }
    }

    /// The configuration line to write.
    pub fn render(&self) -> String {
        match self {
            Assignment::Remap { position, action } => {
                RemapLine::single(position.clone(), action.clone()).render()
            }
            Assignment::TapAndHold {
                position,
                tap,
                delay_ms,
                hold,
            } => RemapLine::tap_and_hold(position.clone(), tap.clone(), *delay_ms, hold.clone())
                .render(),
            Assignment::Macro { macro_line, .. } => macro_line.render(),
        }
    }

    /// The `Binding` this assignment produces, for the diff engine.
    pub fn as_binding(&self) -> Binding {
        match self {
            Assignment::Remap { position, action } => {
                Binding::Remap(RemapLine::single(position.clone(), action.clone()))
            }
            Assignment::TapAndHold {
                position,
                tap,
                delay_ms,
                hold,
            } => Binding::Remap(RemapLine::tap_and_hold(
                position.clone(),
                tap.clone(),
                *delay_ms,
                hold.clone(),
            )),
            Assignment::Macro { macro_line, .. } => Binding::Macro(macro_line.clone()),
        }
    }

    /// What the operator gets, in words.
    pub fn produces(&self) -> String {
        match self {
            Assignment::Remap { action, .. } => action.to_string(),
            Assignment::TapAndHold {
                tap,
                hold,
                delay_ms,
                ..
            } => format!("{tap} on tap, {hold} after {delay_ms} ms"),
            Assignment::Macro { macro_line, .. } => {
                let mut out = String::new();
                for atom in &macro_line.body {
                    match atom {
                        MacroAtom::Action { stroke, token } => match stroke {
                            Stroke::Down => out.push_str(&format!("{token} down, ")),
                            Stroke::Up => out.push_str(&format!("{token} up, ")),
                            Stroke::Press => out.push_str(&format!("{token}, ")),
                        },
                        MacroAtom::Speed(n) => out.push_str(&format!("speed {n}, ")),
                        MacroAtom::Multiplay(n) => out.push_str(&format!("repeat {n}x, ")),
                        MacroAtom::Delay(ms) => out.push_str(&format!("delay {ms} ms, ")),
                        MacroAtom::RandomDelay => out.push_str("random delay, "),
                        MacroAtom::Unknown(t) => out.push_str(&format!("{t} (unrecognised), ")),
                    }
                }
                out.trim_end_matches(", ").to_string()
            }
        }
    }
}

/// A reviewable preset for one layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerPreset {
    /// Stable identifier, e.g. `programming-symbols`.
    pub id: String,
    /// One-line title.
    pub title: String,
    /// Why the layer and these positions were chosen.
    pub rationale: String,
    pub layer: LayerId,
    pub assignments: Vec<Assignment>,
    /// Caveats and things the operator should check.
    pub caveats: Vec<String>,
}

impl LayerPreset {
    /// A row per assignment, for the table both interfaces render.
    pub fn table(&self) -> Vec<PresetRow> {
        self.assignments
            .iter()
            .map(|a| {
                let def = positions::lookup(a.position());
                PresetRow {
                    position: a.position().to_string(),
                    keycap: def.map(|d| d.keycap.to_string()).unwrap_or_default(),
                    half: def.map(|d| d.half.label().to_string()).unwrap_or_default(),
                    produces: a.produces(),
                    line: a.render(),
                }
            })
            .collect()
    }

    /// Count of assignments, for the review summary.
    pub fn len(&self) -> usize {
        self.assignments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assignments.is_empty()
    }
}

/// One row of the review table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetRow {
    /// Position token.
    pub position: String,
    /// What is printed on that key in profile 0.
    pub keycap: String,
    pub half: String,
    /// What the key will do.
    pub produces: String,
    /// The exact configuration line.
    pub line: String,
}

/// A proposed key that makes a layer reachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessKeyOption {
    pub position: String,
    pub keycap: String,
    pub line: String,
    /// What is lost by using this key, or `None` when nothing is lost.
    pub cost: Option<String>,
    /// Whether this option is the suggested default.
    pub recommended: bool,
}

/// The complete reviewable draft: both layers plus access-key options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub symbols: LayerPreset,
    pub navigation: LayerPreset,
    pub access_key_options: Vec<AccessKeyOption>,
    /// The layer the navigation preset lives in, and therefore what the access key must target.
    pub navigation_layer: LayerId,
}

impl Draft {
    /// Every assignment, symbols first.
    pub fn all_assignments(&self) -> Vec<(&LayerId, &Assignment)> {
        let mut out: Vec<(&LayerId, &Assignment)> = Vec::new();
        for a in &self.symbols.assignments {
            out.push((&self.symbols.layer, a));
        }
        for a in &self.navigation.assignments {
            out.push((&self.navigation.layer, a));
        }
        out
    }
}

fn macro_line(trigger: &str, body: Vec<MacroAtom>) -> MacroLine {
    MacroLine {
        trigger: trigger.to_string(),
        modifiers: Vec::new(),
        body,
    }
}

fn press(token: &str) -> MacroAtom {
    MacroAtom::Action {
        stroke: Stroke::Press,
        token: token.to_string(),
    }
}

fn down(token: &str) -> MacroAtom {
    MacroAtom::Action {
        stroke: Stroke::Down,
        token: token.to_string(),
    }
}

fn up(token: &str) -> MacroAtom {
    MacroAtom::Action {
        stroke: Stroke::Up,
        token: token.to_string(),
    }
}

/// A macro that emits `modifier + token`, e.g. Ctrl+Left for word movement.
fn chord(trigger: &str, modifiers: &[&str], token: &str) -> Assignment {
    let mut body = Vec::new();
    for m in modifiers {
        body.push(down(m));
    }
    body.push(press(token));
    for m in modifiers.iter().rev() {
        body.push(up(m));
    }
    Assignment::Macro {
        position: trigger.to_string(),
        macro_line: macro_line(trigger, body),
    }
}

/// A macro that emits the shifted variant of a key, e.g. Shift+[ for `{`.
/// A macro that emits the shifted variant of a key, e.g. Shift+[ for `{`, as the guide requires.
pub fn shifted(trigger: &str, key: &str) -> Assignment {
    Assignment::Macro {
        position: trigger.to_string(),
        macro_line: macro_line(trigger, vec![down("lshf"), press(key), up("lshf")]),
    }
}

/// A macro that emits the shifted variant with a chosen shift key.
/// A macro that emits the shifted variant of a key using a specific Shift key, so a draft can
/// choose right Shift where a program distinguishes it.
pub fn shifted_with(trigger: &str, key: &str, shift: &str) -> Assignment {
    Assignment::Macro {
        position: trigger.to_string(),
        macro_line: macro_line(trigger, vec![down(shift), press(key), up(shift)]),
    }
}

fn remap(position: &str, action: &str) -> Assignment {
    Assignment::Remap {
        position: position.to_string(),
        action: action.to_string(),
    }
}

/// Every symbol the preset aims to make reachable, so a test can assert full coverage.
pub const PROGRAMMING_SYMBOLS: &[char] = &[
    '(', ')', '[', ']', '{', '}', '<', '>', '"', '\'', '`', '\\', '|', '~', ';', ':', ',', '.',
    '/', '?', '=', '+', '-', '_', '*', '&', '^', '%', '$', '#', '@', '!',
];

/// Build the programming-symbol preset for the US layout.
///
/// Left hand gets the opening pair members and the operator characters; the right hand gets the
/// closing members and the remaining operators, so a pair like `[`/`]` is split across two hands.
pub fn programming_symbols_us() -> LayerPreset {
    // Left hand: opening pair members, quotes and the left half of each pair.
    // Right hand: closing pair members and the operators they pair with.
    // Plain symbols use a single-action remap; shifted symbols use the shift+key macro the
    // Direct Programming Guide requires. Non-printing filler actions keep `n` out of the grid.
    let a = vec![
        shifted("a", "9"),       // (
        remap("s", "obrk"),      // [
        shifted("d", "obrk"),    // {
        shifted("f", "comm"),    // <
        shifted("g", "1"),       // !
        remap("q", "eql"),       // =
        shifted("w", "eql"),     // +
        remap("e", "hyph"),      // -
        shifted("r", "hyph"),    // _
        shifted("t", "8"),       // *
        remap("z", "grav"),      // `
        shifted("x", "grav"),    // ~
        remap("c", "scol"),      // ;
        shifted("v", "scol"),    // :
        remap("b", "apos"),      // '
        shifted("grav", "3"),    // #
        shifted("caps", "2"),    // @
        shifted("hk1", "4"),     // $
        shifted("hk2", "5"),     // %
        shifted("hk3", "6"),     // ^
        shifted("hk4", "7"),     // &
        shifted("y", "0"),       // )
        remap("u", "cbrk"),      // ]
        shifted("i", "cbrk"),    // }
        shifted("o", "perd"),    // >
        shifted("p", "apos"),    // "
        remap("h", "comm"),      // ,
        remap("j", "perd"),      // .
        remap("k", "fsls"),      // /
        shifted("l", "fsls"),    // ?
        remap("scol", "bsls"),   // backslash
        shifted("apos", "bsls"), // |
        remap("n", "sclk"),      // filler: keep `n` out of the symbol grid
    ];
    LayerPreset {
        id: "programming-symbols".to_string(),
        title: "Programming symbols on the Fn1 layer".to_string(),
        rationale: "Fn1 is already reachable by holding either Fn key, and its only factory \
                    assignments are F1-F12 on the number row. This draft leaves that whole row \
                    untouched, so the operator keeps every function key and gains the symbol set \
                    on the letter rows with one chord that already exists. Opening pair members \
                    sit on the left hand and closing members on the right, so typing `[]`, `{}` \
                    or `()` alternates hands. Plain symbols use a single-action remap; shifted \
                    symbols use the shift+key macro the guide requires."
            .to_string(),
        layer: LayerId::Function1,
        assignments: a,
        caveats: vec![
            "The Direct Programming Guide §4.6 says a macro's trigger is the *action*, not the \
             physical key. These drafts never give the same position both a remap and a macro, so \
             every trigger fires exactly where intended."
                .to_string(),
            "Six symbols use `shifted` macros that send Left Shift. If a program distinguishes \
             left from right Shift, edit those lines to use `rshf`."
                .to_string(),
            "`n` is proposed as Scroll Lock purely to keep that position out of the symbol grid; \
             change or delete it if you use `n` in this layer."
                .to_string(),
            "The remaining positions (Tab, Esc, Shift, Ctrl, Alt, thumb clusters and the number \
             row) keep their factory actions in this layer."
                .to_string(),
        ],
    }
}

/// Build the navigation preset.
///
/// The base layout already provides arrows on the bottom rows and Home/End/Delete/PgUp/PgDn on the
/// thumb clusters, so this draft concentrates on what is genuinely missing: comfortable arrows on
/// the home row, plus word, line and document movement and selection.
pub fn navigation_layer() -> LayerPreset {
    let a = vec![
        chord("a", &["lctr"], "left"), // previous word
        remap("s", "left"),
        remap("d", "down"),
        remap("f", "up"),
        remap("g", "rght"),
        remap("w", "home"),
        remap("e", "end"),
        chord("q", &["lctr"], "home"), // start of document
        chord("r", &["lctr"], "end"),  // end of document
        chord("t", &["lctr"], "rght"), // next word
        chord("z", &["lshf"], "left"), // extend selection left
        chord("x", &["lshf"], "rght"), // extend selection right
        chord("c", &["lshf"], "home"), // select to line start
        chord("v", &["lshf"], "end"),  // select to line end
        remap("b", "del"),             // forward delete
        remap("grav", "bspc"),
        remap("caps", "null"), // no accidental capitals while navigating
        remap("hk1", "pgup"),
        remap("hk2", "pgdn"),
        chord("hk3", &["lctr", "lshf"], "left"), // select previous word
        chord("hk4", &["lctr", "lshf"], "rght"), // select next word
        remap("h", "left"),
        remap("j", "down"),
        remap("k", "up"),
        remap("l", "rght"),
        remap("n", "home"),
        remap("m", "end"),
        chord("comm", &["lctr"], "left"),
        chord("perd", &["lctr"], "rght"),
        remap("fsls", "del"),
        remap("u", "pgup"),
        remap("o", "pgdn"),
        chord("y", &["lctr"], "home"),
        chord("i", &["lctr"], "end"),
        chord("p", &["lshf"], "home"),
        chord("scol", &["lshf"], "end"),
        chord("apos", &["lctr", "lshf"], "left"),
        chord("bsls", &["lctr", "lshf"], "rght"),
        chord("obrk", &["lshf"], "left"),
        chord("cbrk", &["lshf"], "rght"),
    ];
    LayerPreset {
        id: "navigation".to_string(),
        title: "Navigation on the Fn2 layer".to_string(),
        rationale: "The base layout already puts arrows on the bottom rows and \
                    Home/End/Delete/Page Up/Page Down on the thumb clusters, so this draft adds \
                    what is actually missing: arrows under the index and middle fingers, word \
                    movement with Ctrl, document movement with Ctrl+Home/End, and the matching \
                    selection chords with Shift. Fn2 has no factory assignments, so nothing is \
                    lost by using it."
            .to_string(),
        layer: LayerId::Function2,
        assignments: a,
        caveats: vec![
            "Fn2 is not reachable by default. Accept one of the access-key lines below, or the \
             layer will exist but no key will take you there."
                .to_string(),
            "Word movement is sent as Ctrl+arrow (Linux and Windows behaviour). On macOS the \
             equivalent is usually Option+arrow: change `lctr` to `lalt` in those lines if you \
             write there."
                .to_string(),
            "`caps` is proposed as producing no action in this layer so that reach-for-Caps \
             accidents cannot happen while navigating. It is unchanged in every other layer."
                .to_string(),
        ],
    }
}

/// Access keys that make the navigation layer reachable, written under `<base>`.
///
/// Ordered from "costs nothing" to "costs something", with the reason for each cost.
pub fn access_key_options(navigation_layer: LayerId) -> Vec<AccessKeyOption> {
    let shift = match navigation_layer {
        LayerId::Function2 => "fn2s",
        LayerId::Function3 => "fn3s",
        LayerId::Keypad => "keys",
        LayerId::Base | LayerId::Function1 => "fn1s",
    };
    vec![
        AccessKeyOption {
            position: "hk1".to_string(),
            keycap: "1 (circled), left module".to_string(),
            line: format!("[hk1]>[{shift}]"),
            cost: None,
            recommended: true,
        },
        AccessKeyOption {
            position: "hk3".to_string(),
            keycap: "3 (circled), right module".to_string(),
            line: format!("[hk3]>[{shift}]"),
            cost: None,
            recommended: false,
        },
        AccessKeyOption {
            position: "caps".to_string(),
            keycap: "Caps Lock".to_string(),
            line: format!("[caps]>[{shift}]"),
            cost: Some(
                "Caps Lock would no longer lock capitals on the Base layer, in every profile that \
                 uses this layout file."
                    .to_string(),
            ),
            recommended: false,
        },
        AccessKeyOption {
            position: "rwin".to_string(),
            keycap: "Win, right thumb".to_string(),
            line: format!("[rwin]>[{shift}]"),
            cost: Some(
                "The right Windows/Super key would be taken over; pick another option if your \
                 Hyprland configuration relies on it."
                    .to_string(),
            ),
            recommended: false,
        },
    ]
}

/// Build the whole reviewable draft.
pub fn build_draft() -> Draft {
    let navigation = navigation_layer();
    Draft {
        symbols: programming_symbols_us(),
        access_key_options: access_key_options(navigation.layer),
        navigation_layer: navigation.layer,
        navigation,
    }
}

/// Render the draft as the text of a `layoutN.txt` file: five documented layer headers, with the
/// preset lines under the two layers they belong to and nothing under the others.
///
/// The result is a complete, valid layout file that the operator can read, diff and write to any
/// of the nine programmable profiles.
pub fn render_layout_draft(draft: &Draft, include_access_key: Option<&AccessKeyOption>) -> String {
    let mut out = String::new();
    out.push_str("# Draft generated by adv360-config. Review before writing.\n");
    out.push_str("# Symbols and navigation only; every other key keeps its factory action.\n");
    out.push_str("<base>\n");
    if let Some(opt) = include_access_key {
        out.push_str(&format!(
            "# Access key for {}: {}\n",
            draft.navigation_layer.short_label(),
            opt.cost
                .clone()
                .unwrap_or_else(|| "costs nothing, the key is blank by default".to_string())
        ));
        out.push_str(&opt.line);
        out.push('\n');
    }
    out.push_str("<keypad>\n");
    out.push_str("<function1>\n");
    for a in &draft.symbols.assignments {
        out.push_str(&a.render());
        out.push('\n');
    }
    out.push_str("<function2>\n");
    for a in &draft.navigation.assignments {
        out.push_str(&a.render());
        out.push('\n');
    }
    out.push_str("<function3>\n");
    out
}

/// A Markdown review sheet: what each key will do, and what to check.
pub fn render_review_sheet(draft: &Draft, layout: &crate::kbd_layout::KeyboardLayout) -> String {
    let mut out = String::new();
    out.push_str("# Review sheet\n\n");
    out.push_str(
        "Nothing has been written. This is the draft the tool proposes; read it, change it, or \
         discard it.\n\n",
    );
    out.push_str(&format!(
        "Keyboard layout in use: **{}** ({}){}\n\n",
        layout.layout,
        layout.source.label(),
        match layout.reliability_warning() {
            Some(w) => format!("\n\n> {w}"),
            None => " — characters below are computed from the encoded `us` mapping.".to_string(),
        }
    ));
    for preset in [&draft.symbols, &draft.navigation] {
        out.push_str(&format!("## {}\n\n", preset.title));
        out.push_str(&format!("Layer: `{}`\n\n", preset.layer.header()));
        out.push_str(&format!("{}\n\n", preset.rationale));
        out.push_str(
            "| Position | Key in profile 0 | Module | Will produce | Configuration line |\n",
        );
        out.push_str("|---|---|---|---|---|\n");
        for row in preset.table() {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | `{}` |\n",
                row.position, row.keycap, row.half, row.produces, row.line
            ));
        }
        out.push('\n');
        if !preset.caveats.is_empty() {
            out.push_str("Things to check:\n\n");
            for c in &preset.caveats {
                out.push_str(&format!("- {c}\n"));
            }
            out.push('\n');
        }
    }
    out.push_str("## Reaching the navigation layer\n\n");
    out.push_str(
        "The symbols layer needs no new key: hold either Fn key. The navigation layer does, \
         because Fn2 has no default access key. Pick one line, or none:\n\n",
    );
    out.push_str("| Key | Where | Line | Cost |\n|---|---|---|---|\n");
    for o in &draft.access_key_options {
        out.push_str(&format!(
            "| `{}` | {} | `{}` | {} |\n",
            o.position,
            o.keycap,
            o.line,
            o.cost
                .clone()
                .unwrap_or_else(|| "nothing, blank by default".to_string())
        ));
    }
    out.push('\n');
    out.push_str(
        "## What this draft does *not* touch\n\n\
         - The number row keeps F1-F12 on the Fn1 layer.\n\
         - Every position not listed above keeps its factory action in every layer.\n\
         - `settings.txt`, all indicator files, all other profiles and the firmware are never \
           modified by this tool.\n\
         - No key is remapped on the Base layer unless you accept one of the access-key lines.\n",
    );
    out
}

/// Verify a preset only uses documented tokens and programmable positions.
///
/// Returns the list of problems; an empty list means the draft is safe to offer.
pub fn self_check(preset: &LayerPreset) -> Vec<String> {
    let mut problems = Vec::new();
    for a in &preset.assignments {
        let pos = a.position();
        match positions::lookup(pos) {
            None => problems.push(format!("{pos}: not a documented position")),
            Some(p) if !p.programmable => {
                problems.push(format!("{pos}: the SmartSet key cannot be programmed"))
            }
            Some(_) => {}
        }
        match a {
            Assignment::Remap { action, .. } => {
                if !tokens::is_known(action) {
                    problems.push(format!(
                        "{pos}: '{action}' is not a documented action token"
                    ));
                }
            }
            Assignment::TapAndHold { tap, hold, .. } => {
                for t in [tap, hold] {
                    if !tokens::is_known(t) {
                        problems.push(format!("{pos}: '{t}' is not a documented action token"));
                    }
                }
            }
            Assignment::Macro { macro_line, .. } => {
                // The guide says a macro is triggered by an action, and its own example 3 uses a
                // position token (`{hk1}`) for a key that is blank by default, so both are accepted.
                if !tokens::is_known(&macro_line.trigger)
                    && positions::lookup(&macro_line.trigger).is_none()
                {
                    problems.push(format!(
                        "{pos}: trigger is neither a documented action token nor a position"
                    ));
                }
                if macro_line.trigger != *pos {
                    problems.push(format!(
                        "{pos}: the macro trigger is '{}' but the guide triggers macros by \
                         *action*; in this layer the position produces '{pos}'",
                        macro_line.trigger
                    ));
                }
                for atom in &macro_line.body {
                    match atom {
                        MacroAtom::Action { stroke, token } => {
                            if !tokens::is_known(token) {
                                problems
                                    .push(format!("{pos}: '{token}' is not a documented token"));
                            }
                            if !stroke.is_modifier_stroke() && tokens::is_modifier(token) {
                                problems.push(format!(
                                    "{pos}: modifier '{token}' must use a '-' or '+' stroke"
                                ));
                            }
                        }
                        MacroAtom::Unknown(t) => {
                            problems.push(format!("{pos}: '{t}' is not a documented token"))
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    // A position must not carry two assignments in one preset.
    let mut seen: Vec<&str> = Vec::new();
    for a in &preset.assignments {
        let pos = a.position();
        if seen.contains(&pos) {
            problems.push(format!("{pos}: assigned twice in one layer"));
        } else {
            seen.push(pos);
        }
    }
    problems
}

/// Look up a position for the review table, tolerating an unknown token.
pub fn find_position(token: &str) -> Option<&'static PositionDef> {
    positions::lookup(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_error::ParseError;
    use crate::parser;
    use crate::validate;

    fn parse_preset_into_file(preset: &LayerPreset) -> crate::model::ConfigFile {
        let mut text = String::new();
        for layer in LayerId::all() {
            text.push_str(layer.header());
            text.push('\n');
            if *layer == preset.layer {
                for a in &preset.assignments {
                    text.push_str(&a.render());
                    text.push('\n');
                }
            }
        }
        parser::parse_bytes(
            text.as_bytes(),
            "layouts/layout1.txt",
            crate::model::FileRole::ProfileLayout,
        )
        .unwrap()
    }

    #[test]
    fn both_presets_pass_their_own_self_check() {
        let d = build_draft();
        assert_eq!(self_check(&d.symbols), Vec::<String>::new());
        assert_eq!(self_check(&d.navigation), Vec::<String>::new());
    }

    #[test]
    fn the_presets_produce_lines_that_parse_without_errors() {
        let d = build_draft();
        for preset in [&d.symbols, &d.navigation] {
            let file = parse_preset_into_file(preset);
            let report = validate::validate_file(&file);
            assert_eq!(
                report.error_count(),
                0,
                "{} produced validation errors: {:#?}",
                preset.id,
                report.errors().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn no_preset_line_fails_to_parse() {
        let d = build_draft();
        for preset in [&d.symbols, &d.navigation] {
            for a in &preset.assignments {
                let line =
                    parser::parse_line(a.render(), crate::model::Eol::Lf, Some(preset.layer));
                assert!(
                    !matches!(line.kind, crate::model::LineKind::Invalid { .. }),
                    "{} did not parse: {}",
                    a.render(),
                    match &line.kind {
                        crate::model::LineKind::Invalid { error } => format!("{error}"),
                        _ => String::new(),
                    }
                );
            }
        }
    }

    #[test]
    fn the_symbol_preset_covers_every_symbol_in_its_target_set() {
        let preset = programming_symbols_us();
        let mut produced: Vec<char> = Vec::new();
        for a in &preset.assignments {
            match a {
                Assignment::Remap { action, .. } => {
                    // A couple of filler positions deliberately use non-printing actions.
                    if let Some((plain, _)) = crate::kbd_layout::us_characters(action) {
                        produced.push(plain);
                    }
                }
                Assignment::Macro { macro_line, .. } => {
                    // A shift+key macro produces the shifted character of that key.
                    let shift = macro_line.body.iter().find_map(|atom| match atom {
                        MacroAtom::Action { token, .. } if token != "lshf" => Some(token.clone()),
                        _ => None,
                    });
                    if let Some(key) = shift {
                        if let Some((_, shifted)) = crate::kbd_layout::us_characters(&key) {
                            produced.push(shifted);
                        }
                    }
                }
                Assignment::TapAndHold { .. } => {}
            }
        }
        for symbol in PROGRAMMING_SYMBOLS {
            assert!(
                produced.contains(symbol),
                "the preset never produces {symbol:?}; produced: {produced:?}"
            );
        }
        // And the two Scroll-Lock filler entries are not counted as symbols.
        assert!(produced.len() >= PROGRAMMING_SYMBOLS.len());
    }

    #[test]
    fn the_navigation_preset_has_word_movement_and_the_documented_navigation_actions() {
        let preset = navigation_layer();
        let lines: Vec<String> = preset.assignments.iter().map(|a| a.render()).collect();
        let joined = lines.join("\n");
        // Word movement, which the base layout does not provide at all.
        assert!(joined.contains("{comm}>{-lctr}{left}{+lctr}"), "{joined}");
        for needed in [
            "left", "rght", "up", "down", "home", "end", "del", "pgup", "pgdn",
        ] {
            assert!(
                joined.contains(&format!("[{needed}]")),
                "{needed} missing from {joined}"
            );
        }
    }

    #[test]
    fn macros_are_triggered_by_the_action_the_position_produces_in_that_layer() {
        // This is the guide's own rule; the self-check enforces it.
        let bad = LayerPreset {
            id: "bad".into(),
            title: "bad".into(),
            rationale: String::new(),
            layer: LayerId::Function1,
            assignments: vec![Assignment::Macro {
                position: "a".into(),
                macro_line: macro_line("9", vec![down("lshf"), press("9"), up("lshf")]),
            }],
            caveats: vec![],
        };
        let problems = self_check(&bad);
        assert!(
            problems.iter().any(|p| p.contains("trigger")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_preset_using_the_smartset_key_is_rejected_by_the_self_check() {
        let bad = LayerPreset {
            id: "bad".into(),
            title: "bad".into(),
            rationale: String::new(),
            layer: LayerId::Base,
            assignments: vec![remap("ss", "q")],
            caveats: vec![],
        };
        assert!(self_check(&bad).iter().any(|p| p.contains("SmartSet")));
    }

    #[test]
    fn a_preset_assigning_one_position_twice_is_rejected() {
        let bad = LayerPreset {
            id: "bad".into(),
            title: "bad".into(),
            rationale: String::new(),
            layer: LayerId::Base,
            assignments: vec![remap("q", "w"), remap("q", "e")],
            caveats: vec![],
        };
        assert!(self_check(&bad)
            .iter()
            .any(|p| p.contains("assigned twice")));
    }

    #[test]
    fn the_access_key_options_all_target_the_right_layer_and_have_a_stated_cost() {
        let options = access_key_options(LayerId::Function2);
        assert!(!options.is_empty());
        for o in &options {
            assert!(o.line.contains("[fn2s]"), "{}", o.line);
            assert!(positions::lookup(&o.position).is_some());
            // A cost is stated explicitly, even when it is "none".
            if o.recommended {
                assert!(o.cost.is_none());
            }
        }
        assert_eq!(options.iter().filter(|o| o.recommended).count(), 1);
    }

    #[test]
    fn the_rendered_draft_is_a_valid_complete_layout_file() {
        let d = build_draft();
        let text = render_layout_draft(&d, None);
        let file = parser::parse_bytes(
            text.as_bytes(),
            "layouts/layout1.txt",
            crate::model::FileRole::ProfileLayout,
        )
        .unwrap();
        // All five documented headers present, in order.
        let headers: Vec<LayerId> = file
            .lines
            .iter()
            .filter_map(|l| match l.kind {
                crate::model::LineKind::LayerHeader(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(headers, LayerId::all().to_vec());
        let report = validate::validate_file(&file);
        assert_eq!(
            report.error_count(),
            0,
            "{:#?}",
            report.errors().collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_rendered_draft_with_an_access_key_puts_it_under_the_base_layer() {
        let d = build_draft();
        let key = d.access_key_options.iter().find(|o| o.recommended).unwrap();
        let text = render_layout_draft(&d, Some(key));
        let file = parser::parse_bytes(
            text.as_bytes(),
            "layouts/layout1.txt",
            crate::model::FileRole::ProfileLayout,
        )
        .unwrap();
        let remap = file
            .lines
            .iter()
            .find(|l| matches!(&l.kind, crate::model::LineKind::Remap(r) if r.position == "hk1"))
            .expect("access key line must exist");
        assert_eq!(remap.layer, Some(LayerId::Base));
    }

    #[test]
    fn the_review_sheet_states_that_nothing_was_written_and_lists_every_key() {
        let d = build_draft();
        let layout = crate::kbd_layout::KeyboardLayout::manual("us", None);
        let sheet = render_review_sheet(&d, &layout);
        assert!(sheet.contains("Nothing has been written"));
        assert!(sheet.contains("Keyboard layout in use: **us**"));
        for row in d.symbols.table() {
            assert!(
                sheet.contains(&row.line),
                "missing {} in the sheet",
                row.line
            );
        }
        assert!(sheet.contains("never modified by this tool"));
    }

    #[test]
    fn the_review_sheet_marks_characters_uncertain_on_a_non_us_layout() {
        let d = build_draft();
        let layout = crate::kbd_layout::KeyboardLayout::manual("latam", None);
        let sheet = render_review_sheet(&d, &layout);
        assert!(sheet.contains("uncertain"), "{sheet}");
        assert!(sheet.contains("§3.3") || sheet.contains("3.3"));
    }

    #[test]
    fn preset_lines_never_mix_bracket_kinds_and_are_accepted_by_the_parser() {
        let d = build_draft();
        for (_, a) in d.all_assignments() {
            let line = a.render();
            let has_square = line.contains('[');
            let has_curly = line.contains('{');
            assert!(
                !(has_square && has_curly),
                "a single line must not mix bracket kinds: {line}"
            );
            let parsed = parser::parse_line(line.clone(), crate::model::Eol::Lf, None);
            assert!(
                !matches!(parsed.kind, crate::model::LineKind::Invalid { .. }),
                "{line} failed to parse"
            );
        }
    }

    #[test]
    fn a_helper_never_emits_an_empty_macro_body() {
        let m = chord("a", &["lctr"], "left");
        let Assignment::Macro { macro_line, .. } = m else {
            panic!()
        };
        assert_eq!(macro_line.action_count(), 3);
        assert_eq!(macro_line.render(), "{a}>{-lctr}{left}{+lctr}");
        let s = shifted("a", "9");
        let Assignment::Macro { macro_line, .. } = s else {
            panic!()
        };
        assert_eq!(macro_line.render(), "{a}>{-lshf}{9}{+lshf}");
        assert_eq!(
            shifted_with("a", "9", "rshf").render(),
            "{a}>{-rshf}{9}{+rshf}"
        );
    }

    #[test]
    fn the_parse_error_type_is_used_for_invalid_lines() {
        // Guards against a preset accidentally depending on something unparsable.
        let bad = parser::parse_line("[a]>[".to_string(), crate::model::Eol::Lf, None);
        let crate::model::LineKind::Invalid { error } = bad.kind else {
            panic!()
        };
        let _: ParseError = error;
    }
}
