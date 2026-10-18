//! Differences between the file on the v-Drive and the draft in memory.
//!
//! Two views are produced, because the two interfaces need different things:
//!
//! * a **semantic** view, per key and per layer ("Escape in Base: caps -> q"), which is what the
//!   operator actually decided;
//! * a **textual** unified diff, so the operator can see exactly which lines of which file change
//!   before confirming a write.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::layer::LayerId;
use crate::model::{ConfigFile, LineKind, MacroLine, RemapLine};

/// A key's configured behaviour in one layer — either a single remap or a macro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Binding {
    Remap(RemapLine),
    Macro(MacroLine),
}

impl Binding {
    /// The configuration line this binding would be written as.
    pub fn render(&self) -> String {
        match self {
            Binding::Remap(r) => r.render(),
            Binding::Macro(m) => m.render(),
        }
    }

    /// A one-line description used in the change list.
    pub fn describe(&self) -> String {
        match self {
            Binding::Remap(r) => match r.tap_hold_delay() {
                Some(ms) => format!(
                    "tap {} / hold {} ({} ms)",
                    r.tap_actions().join("+"),
                    r.hold_actions().join("+"),
                    ms
                ),
                None => r.tap_actions().join("+"),
            },
            Binding::Macro(m) => {
                let mods = if m.modifiers.is_empty() {
                    String::new()
                } else {
                    format!("{}+", m.modifiers.join("+"))
                };
                format!("macro {mods}{} ({} actions)", m.trigger, m.action_count())
            }
        }
    }
}

/// What changed on one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyChangeKind {
    /// The key had no custom line and now has one.
    Added,
    /// The key had a custom line and no longer does.
    Removed,
    /// The key had a custom line whose content changed.
    Modified,
}

impl KeyChangeKind {
    pub fn label(self) -> &'static str {
        match self {
            KeyChangeKind::Added => "added",
            KeyChangeKind::Removed => "removed",
            KeyChangeKind::Modified => "modified",
        }
    }
}

/// One semantic change, addressed by layer and either a position token or a macro trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyChange {
    pub kind: KeyChangeKind,
    pub layer: LayerId,
    /// The position token, or the macro trigger written as `{trigger}`.
    pub target: String,
    pub before: Option<Binding>,
    pub after: Option<Binding>,
    /// 1-based line number in the original file, when the binding existed there.
    pub line_before: Option<usize>,
    /// 1-based line number in the draft file, when the binding exists there.
    pub line_after: Option<usize>,
}

impl KeyChange {
    /// A single sentence the operator can read without context.
    pub fn describe(&self) -> String {
        let layer = self.layer.short_label();
        match (&self.before, &self.after) {
            (Some(b), Some(a)) => {
                format!(
                    "{} in {}: {} -> {}",
                    self.target,
                    layer,
                    b.describe(),
                    a.describe()
                )
            }
            (Some(b), None) => format!(
                "{} in {}: {} -> (removed, factory action restored)",
                self.target,
                layer,
                b.describe()
            ),
            (None, Some(a)) => format!(
                "{} in {}: (factory action) -> {}",
                self.target,
                layer,
                a.describe()
            ),
            (None, None) => format!("{} in {}: no change", self.target, layer),
        }
    }
}

/// The textual differences of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub relative_path: String,
    /// 1-based line numbers that differ.
    pub changed_lines: Vec<usize>,
    pub unified: String,
    /// True when the rendered file is identical to what is on the v-Drive.
    pub identical: bool,
}

impl FileDiff {
    pub fn is_empty(&self) -> bool {
        self.changed_lines.is_empty()
    }
}

/// Everything that would change if the draft were written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    pub files: Vec<FileDiff>,
    pub key_changes: Vec<KeyChange>,
}

impl DiffSummary {
    /// True when writing would not modify the v-Drive at all.
    pub fn is_empty(&self) -> bool {
        self.files.iter().all(FileDiff::is_empty) && self.key_changes.is_empty()
    }

    /// Total number of differing lines across all files.
    pub fn changed_line_count(&self) -> usize {
        self.files.iter().map(|f| f.changed_lines.len()).sum()
    }
}

/// Key a binding map entry: the layer plus the addressed target.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Position(String),
    Macro(String),
}

impl Target {
    fn display(&self) -> String {
        match self {
            Target::Position(p) => format!("[{p}]"),
            Target::Macro(t) => format!("{{{t}}}"),
        }
    }

    fn sort_key(&self) -> (u8, String) {
        match self {
            Target::Position(p) => (0, p.to_ascii_lowercase()),
            Target::Macro(t) => (1, t.to_ascii_lowercase()),
        }
    }
}

/// Normalise a binding for comparison.
///
/// The guide states that tokens are not case-sensitive, so `[ESC]>[CAPS]` and `[esc]>[caps]` are
/// the same configuration and must not be reported as a change.
fn normalise(binding: &Binding) -> Binding {
    match binding {
        Binding::Remap(r) => Binding::Remap(RemapLine {
            position: r.position.to_ascii_lowercase(),
            atoms: r
                .atoms
                .iter()
                .map(|a| match a {
                    crate::model::RemapAtom::Action(t) => {
                        crate::model::RemapAtom::Action(t.to_ascii_lowercase())
                    }
                    crate::model::RemapAtom::TapHold(ms) => crate::model::RemapAtom::TapHold(*ms),
                })
                .collect(),
        }),
        Binding::Macro(m) => Binding::Macro(MacroLine {
            trigger: m.trigger.to_ascii_lowercase(),
            modifiers: m.modifiers.iter().map(|s| s.to_ascii_lowercase()).collect(),
            body: m
                .body
                .iter()
                .map(|a| match a {
                    crate::model::MacroAtom::Action { stroke, token } => {
                        crate::model::MacroAtom::Action {
                            stroke: *stroke,
                            token: token.to_ascii_lowercase(),
                        }
                    }
                    other => other.clone(),
                })
                .collect(),
        }),
    }
}

fn binding_map(file: &ConfigFile) -> BTreeMap<(LayerId, Target), (Binding, usize)> {
    let mut map = BTreeMap::new();
    for (idx, line) in file.lines.iter().enumerate() {
        if !line.enabled {
            // A disabled line is not in effect, so it is not part of the effective configuration.
            continue;
        }
        let Some(layer) = line.layer else { continue };
        let entry = match &line.kind {
            LineKind::Remap(r) => (normalise(&Binding::Remap(r.clone())), idx + 1),
            LineKind::Macro(m) => (normalise(&Binding::Macro(m.clone())), idx + 1),
            _ => continue,
        };
        let target = match &entry.0 {
            Binding::Remap(r) => Target::Position(r.position.to_ascii_lowercase()),
            Binding::Macro(m) => Target::Macro(m.trigger.to_ascii_lowercase()),
        };
        // The guide: the command closest to the bottom of the file wins.
        map.insert((layer, target), entry);
    }
    map
}

/// Compare two versions of the same file and list the key-level changes.
pub fn key_changes(original: &ConfigFile, current: &ConfigFile) -> Vec<KeyChange> {
    let before = binding_map(original);
    let after = binding_map(current);
    let mut keys: Vec<&(LayerId, Target)> = before.keys().chain(after.keys()).collect();
    keys.sort_by_key(|(l, t)| (*l as u8, t.sort_key()));

    let mut out: Vec<KeyChange> = Vec::new();
    let mut seen: Vec<(LayerId, Target)> = Vec::new();
    for key in keys {
        if seen.contains(key) {
            continue;
        }
        seen.push(key.clone());
        let b = before.get(key);
        let a = after.get(key);
        let kind = match (b, a) {
            (Some(_), None) => KeyChangeKind::Removed,
            (None, Some(_)) => KeyChangeKind::Added,
            (Some(x), Some(y)) if x.0 == y.0 => continue,
            (Some(_), Some(_)) => KeyChangeKind::Modified,
            (None, None) => continue,
        };
        out.push(KeyChange {
            kind,
            layer: key.0,
            target: key.1.display(),
            before: b.map(|(binding, _)| binding.clone()),
            after: a.map(|(binding, _)| binding.clone()),
            line_before: b.map(|(_, n)| *n),
            line_after: a.map(|(_, n)| *n),
        });
    }
    out
}

/// Build the textual diff of a file against its on-disk content.
pub fn file_diff(relative_path: &str, on_disk: Option<&str>, draft: &str) -> FileDiff {
    let before = on_disk.unwrap_or("");
    let changed_lines = crate::writer::changed_line_numbers(on_disk, draft);
    let identical = on_disk.is_some_and(|b| b == draft);
    FileDiff {
        relative_path: relative_path.to_string(),
        changed_lines,
        unified: unified_diff(relative_path, before, draft, on_disk.is_some()),
        identical,
    }
}

/// A compact unified diff with up to `CONTEXT` lines of context.
pub fn unified_diff(name: &str, before: &str, after: &str, before_exists: bool) -> String {
    const CONTEXT: usize = 3;
    let a: Vec<&str> = if before_exists {
        before.lines().collect()
    } else {
        Vec::new()
    };
    let b: Vec<&str> = after.lines().collect();
    if a == b {
        return String::new();
    }

    let ops = diff_ops(&a, &b);
    let mut out = String::new();
    out.push_str(&format!("--- {name} (on the v-Drive)\n"));
    out.push_str(&format!("+++ {name} (draft to write)\n"));

    let mut idx = 0usize;
    while idx < ops.len() {
        match ops[idx] {
            Op::Same(_, _) => idx += 1,
            _ => {
                let start = idx.saturating_sub(CONTEXT);
                let mut end = idx;
                let mut gap = 0usize;
                while end < ops.len() {
                    match ops[end] {
                        Op::Same(_, _) => {
                            gap += 1;
                            if gap > CONTEXT * 2 {
                                break;
                            }
                        }
                        _ => gap = 0,
                    }
                    end += 1;
                }
                let end = (end).min(ops.len());
                emit_hunk(&mut out, name, &ops[start..end], &a, &b);
                idx = end;
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Same(usize, usize),
    Del(usize),
    Ins(usize),
}

fn diff_ops(a: &[&str], b: &[&str]) -> Vec<Op> {
    let n = a.len();
    let m = b.len();
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Same(i, j));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            ops.push(Op::Del(i));
            i += 1;
        } else {
            ops.push(Op::Ins(j));
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Del(i));
        i += 1;
    }
    while j < m {
        ops.push(Op::Ins(j));
        j += 1;
    }
    ops
}

fn emit_hunk(out: &mut String, _name: &str, ops: &[Op], a: &[&str], b: &[&str]) {
    let first_a = ops.iter().find_map(|o| match o {
        Op::Same(i, _) | Op::Del(i) => Some(*i + 1),
        Op::Ins(_) => None,
    });
    let first_b = ops.iter().find_map(|o| match o {
        Op::Same(_, j) | Op::Ins(j) => Some(*j + 1),
        Op::Del(_) => None,
    });
    let count_a = ops.iter().filter(|o| !matches!(o, Op::Ins(_))).count();
    let count_b = ops.iter().filter(|o| !matches!(o, Op::Del(_))).count();
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        first_a.unwrap_or(0),
        count_a,
        first_b.unwrap_or(0),
        count_b
    ));
    for op in ops {
        match op {
            Op::Same(i, _) => out.push_str(&format!(" {}\n", a[*i])),
            Op::Del(i) => out.push_str(&format!("-{}\n", a[*i])),
            Op::Ins(j) => out.push_str(&format!("+{}\n", b[*j])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileRole;
    use crate::parser::parse_bytes;

    fn file(text: &str) -> ConfigFile {
        parse_bytes(
            text.as_bytes(),
            "layouts/layout1.txt",
            FileRole::ProfileLayout,
        )
        .unwrap()
    }

    fn edit(file: &mut ConfigFile, layer: LayerId, position: &str, action: &str) {
        let remap = RemapLine::single(position, action);
        if let Some(line) = file.lines.iter_mut().find(|l| {
            l.layer == Some(layer)
                && matches!(&l.kind, LineKind::Remap(r) if r.position.eq_ignore_ascii_case(position))
        }) {
            line.set_content(remap.render(), LineKind::Remap(remap), Some(layer));
        } else {
            let idx = file
                .lines
                .iter()
                .rposition(|l| l.layer == Some(layer))
                .map(|i| i + 1)
                .unwrap_or(file.lines.len());
            file.lines.insert(
                idx,
                crate::model::Line::inserted(
                    remap.render(),
                    crate::model::Eol::Lf,
                    LineKind::Remap(remap),
                    Some(layer),
                ),
            );
        }
    }

    #[test]
    fn an_unchanged_file_has_no_diff() {
        let f = file("<base>\n[esc]>[caps]\n");
        let d = file_diff("layouts/layout1.txt", Some(&f.render()), &f.render());
        assert!(d.identical);
        assert!(d.changed_lines.is_empty());
        assert!(d.unified.is_empty());
    }

    #[test]
    fn an_added_remap_is_reported_semantically() {
        let original = file("<base>\n");
        let mut current = original.clone();
        edit(&mut current, LayerId::Base, "esc", "caps");
        let changes = key_changes(&original, &current);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, KeyChangeKind::Added);
        assert_eq!(changes[0].layer, LayerId::Base);
        assert_eq!(changes[0].target, "[esc]");
        assert!(changes[0].before.is_none());
        assert!(changes[0].after.is_some());
        assert!(changes[0].describe().contains("Base"));
        assert!(changes[0].describe().contains("factory action"));
    }

    #[test]
    fn a_modified_remap_is_reported_with_both_values() {
        let original = file("<base>\n[esc]>[caps]\n");
        let mut current = original.clone();
        edit(&mut current, LayerId::Base, "esc", "q");
        let changes = key_changes(&original, &current);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, KeyChangeKind::Modified);
        let text = changes[0].describe();
        assert!(text.contains("caps"), "{text}");
        assert!(text.contains('q'), "{text}");
        assert_eq!(changes[0].line_before, Some(2));
        assert_eq!(changes[0].line_after, Some(2));
    }

    #[test]
    fn removing_a_remap_restores_the_factory_action() {
        let original = file("<base>\n[esc]>[caps]\n");
        let current = file("<base>\n");
        let changes = key_changes(&original, &current);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, KeyChangeKind::Removed);
        assert!(changes[0].describe().contains("factory action restored"));
    }

    #[test]
    fn a_semantically_identical_but_reformatted_line_is_not_a_key_change() {
        let original = file("<base>\n[ESC]>[CAPS]\n");
        let current = file("<base>\n[esc]>[caps]\n");
        let changes = key_changes(&original, &current);
        assert!(changes.is_empty(), "{changes:?}");
    }

    #[test]
    fn a_disabled_line_is_not_part_of_the_effective_configuration() {
        let original = file("<base>\n*[esc]>[caps]\n");
        let current = file("<base>\n");
        assert!(key_changes(&original, &current).is_empty());
    }

    #[test]
    fn the_last_remap_for_a_key_wins_when_computing_changes() {
        let original = file("<base>\n[esc]>[caps]\n");
        let current = file("<base>\n[esc]>[caps]\n[esc]>[q]\n");
        let changes = key_changes(&original, &current);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, KeyChangeKind::Modified);
        let Binding::Remap(r) = changes[0].after.clone().unwrap() else {
            panic!()
        };
        assert_eq!(r.tap_actions(), vec!["q"]);
    }

    #[test]
    fn macro_changes_are_reported_by_trigger() {
        let original = file("<base>\n");
        let current = file("<base>\n{tab}>{-lshf}{h}{+lshf}\n");
        let changes = key_changes(&original, &current);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].target, "{tab}");
        assert!(changes[0].describe().contains("macro"));
        assert!(changes[0].describe().contains("3 actions"));
    }

    #[test]
    fn the_unified_diff_marks_the_changed_line_and_shows_context() {
        let before = "<base>\n[q]>[w]\n[e]>[r]\n[t]>[y]\n<keypad>\n";
        let after = "<base>\n[q]>[w]\n[e]>[u]\n[t]>[y]\n<keypad>\n";
        let d = unified_diff("layouts/layout1.txt", before, after, true);
        assert!(d.contains("--- layouts/layout1.txt (on the v-Drive)"));
        assert!(d.contains("+++ layouts/layout1.txt (draft to write)"));
        assert!(d.contains("-[e]>[r]"), "{d}");
        assert!(d.contains("+[e]>[u]"), "{d}");
        assert!(d.contains(" <base>"), "context expected: {d}");
        assert_eq!(
            super::super::writer::changed_line_numbers(Some(before), after),
            vec![3]
        );
    }

    #[test]
    fn a_new_file_diff_shows_every_line_as_added() {
        let d = unified_diff("layouts/layout9.txt", "", "<base>\n[q]>[w]\n", false);
        assert!(d.contains("+<base>"), "{d}");
        assert!(d.contains("+[q]>[w]"), "{d}");
    }

    #[test]
    fn the_summary_reports_emptiness_correctly() {
        let mut s = DiffSummary::default();
        assert!(s.is_empty());
        s.files.push(FileDiff {
            relative_path: "a".into(),
            changed_lines: vec![1],
            unified: String::new(),
            identical: false,
        });
        assert!(!s.is_empty());
        assert_eq!(s.changed_line_count(), 1);
    }
}
