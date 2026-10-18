//! Validation.
//!
//! Two severities matter operationally:
//!
//! * [`Severity::Error`] **blocks writing**. Anything that would make the keyboard reject a remap,
//!   or that this tool cannot vouch for, is an error.
//! * [`Severity::Warning`] does not block. It is used where the official documentation is silent,
//!   approximate, or where a pre-existing line is simply not understood by this build.
//!
//! The provenance rule is what keeps the second category useful without ever blocking an operator
//! on somebody else's data: a token this build does not recognise is a **warning** when it was
//! already in the file and is preserved untouched, and an **error** when this application is the
//! one introducing it. Nothing in either case is deleted.

use serde::{Deserialize, Serialize};

use crate::layer::LayerId;
use crate::model::{ConfigFile, FileRole, Line, LineKind, MacroAtom, Origin, RemapAtom, Stroke};
use crate::positions;
use crate::tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    /// Informational: a property of the file worth knowing, not a problem.
    Info,
    /// A problem that does not prevent writing.
    Warning,
    /// A problem that prevents writing.
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// One validation finding.
/// Findings are not serialised whole (their `code` is a static catalogue string); the
/// interfaces read them directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub relative_path: String,
    /// 1-based line number, when the finding is about a specific line.
    pub line: Option<usize>,
    /// 0-based column inside the line, when known.
    pub column: Option<usize>,
    /// Stable machine code, for tests and for the interfaces to group by.
    pub code: &'static str,
    pub message: String,
    /// What the operator can do about it.
    pub hint: Option<String>,
}

impl Finding {
    fn new(
        severity: Severity,
        relative_path: &str,
        line: Option<usize>,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Finding {
            severity,
            relative_path: relative_path.to_string(),
            line,
            column: None,
            code,
            message: message.into(),
            hint: None,
        }
    }

    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn at(mut self, column: Option<usize>) -> Self {
        self.column = column;
        self
    }
}

/// The result of validating one or more configuration files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub findings: Vec<Finding>,
}

impl ValidationReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, f: Finding) {
        self.findings.push(f);
    }

    pub fn extend(&mut self, other: ValidationReport) {
        self.findings.extend(other.findings);
    }

    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
    }

    pub fn infos(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Info)
    }

    pub fn error_count(&self) -> usize {
        self.errors().count()
    }

    pub fn warning_count(&self) -> usize {
        self.warnings().count()
    }

    pub fn info_count(&self) -> usize {
        self.infos().count()
    }

    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    /// Writing is allowed only when no error was found.
    pub fn blocks_write(&self) -> bool {
        self.has_errors()
    }

    /// Findings for one line, for the line inspector in both interfaces.
    pub fn for_line(&self, relative_path: &str, line: usize) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.relative_path == relative_path && f.line == Some(line))
            .collect()
    }

    /// Sort findings by file then line, so both interfaces show a stable order.
    pub fn sorted(mut self) -> Self {
        self.findings.sort_by(|a, b| {
            a.relative_path
                .cmp(&b.relative_path)
                .then(a.line.cmp(&b.line))
                .then(a.severity.cmp(&b.severity))
        });
        self
    }
}

/// Validate one configuration file.
pub fn validate_file(file: &ConfigFile) -> ValidationReport {
    let mut report = ValidationReport::new();
    let path = file.relative_path.as_str();

    if !file.role.is_writable() {
        report.push(
            Finding::new(
                Severity::Info,
                path,
                None,
                "role-read-only",
                format!(
                    "This file is {} and is never written by this tool.",
                    file.role.label()
                ),
            )
            .with_hint(match file.role {
                FileRole::Settings => {
                    "The Direct Programming Guide says: do not edit the settings.txt file."
                }
                FileRole::BackupLayout => {
                    "The guide says only the nine numbered layouts can be loaded to the keyboard."
                }
                FileRole::ProfileLighting => "Indicator lighting is shown for reference only.",
                _ => "Left untouched on purpose.",
            }),
        );
    }

    if file.bom {
        report.push(Finding::new(
            Severity::Info,
            path,
            Some(1),
            "bom",
            "The file starts with a UTF-8 byte-order mark; it is preserved exactly as found.",
        ));
    }
    if file.has_mixed_eol() {
        report.push(Finding::new(
            Severity::Info,
            path,
            None,
            "mixed-eol",
            "The file mixes LF and CRLF line endings; each line keeps the ending it already had.",
        ));
    }
    if file.lacks_final_newline() {
        report.push(Finding::new(
            Severity::Info,
            path,
            None,
            "no-final-newline",
            "The file does not end with a line terminator; that is preserved.",
        ));
    }

    validate_layer_headers(file, &mut report);
    for (idx, line) in file.lines.iter().enumerate() {
        let number = idx + 1;
        validate_line(file, line, number, &mut report);
    }
    validate_duplicates(file, &mut report);
    validate_macro_trigger_shadowing(file, &mut report);
    validate_macro_budget(file, &mut report);

    report.sorted()
}

fn validate_layer_headers(file: &ConfigFile, report: &mut ValidationReport) {
    let path = file.relative_path.as_str();
    let mut seen: Vec<LayerId> = Vec::new();
    for (idx, line) in file.lines.iter().enumerate() {
        if let LineKind::LayerHeader(l) = line.kind {
            if seen.contains(&l) {
                report.push(Finding::new(
                    Severity::Warning,
                    path,
                    Some(idx + 1),
                    "duplicate-layer-header",
                    format!(
                        "{} appears more than once; the later lines belong to the later header.",
                        l.header()
                    ),
                ));
            } else {
                seen.push(l);
            }
        }
    }

    // A remap or macro before any layer header has nowhere to go. LED files carry no layer
    // headers at all — the guide's indicator syntax is layer-independent — so they are exempt.
    let is_layout = matches!(file.role, FileRole::ProfileLayout | FileRole::BackupLayout);
    for (idx, line) in file.lines.iter().enumerate() {
        if is_layout
            && line.layer.is_none()
            && matches!(line.kind, LineKind::Remap(_) | LineKind::Macro(_))
        {
            report.push(
                Finding::new(
                    Severity::Error,
                    path,
                    Some(idx + 1),
                    "no-layer-context",
                    "This line is not inside any layer, so the keyboard has no layer to apply it to.",
                )
                .with_hint(
                    "Move it under one of the five documented layer headers: <base>, <keypad>, \
                     <function1>, <function2>, <function3>.",
                ),
            );
        }
    }

    if file.role == FileRole::ProfileLayout {
        let missing: Vec<&str> = LayerId::all()
            .iter()
            .filter(|l| !seen.contains(l))
            .map(|l| l.header())
            .collect();
        if !missing.is_empty() {
            report.push(Finding::new(
                Severity::Info,
                file.relative_path.as_str(),
                None,
                "missing-layer-headers",
                format!("No lines found for: {}", missing.join(", ")),
            ));
        }
    }
}

fn validate_line(file: &ConfigFile, line: &Line, number: usize, report: &mut ValidationReport) {
    let path = file.relative_path.as_str();
    let introducing = line.origin != Origin::Original;

    if !line.enabled {
        report.push(Finding::new(
            Severity::Info,
            path,
            Some(number),
            "disabled-line",
            "This line is switched off with a leading '*' and is left exactly as it is.",
        ));
    }

    match &line.kind {
        LineKind::Blank | LineKind::Comment { .. } | LineKind::LayerHeader(_) => {}
        LineKind::Unknown { reason } => {
            report.push(
                Finding::new(
                    Severity::Info,
                    path,
                    Some(number),
                    "unknown-instruction",
                    format!("Not documented syntax ({reason}); preserved byte for byte."),
                )
                .with_hint("Nothing is removed or rewritten on this line."),
            );
        }
        LineKind::Invalid { error } => {
            report.push(
                Finding::new(
                    Severity::Error,
                    path,
                    Some(number),
                    "syntax-error",
                    format!("Malformed line: {error}"),
                )
                .at(error.column)
                .with_hint(
                    "The line is preserved as it is. Either fix it, or disable it with a leading \
                     '*', before writing.",
                ),
            );
        }
        LineKind::Remap(r) => {
            validate_position(file, &r.position, number, introducing, report);
            validate_remap_actions(r, number, introducing, path, report);
            if r.tap_hold_delay().is_some() {
                validate_tap_and_hold(r, number, introducing, path, report);
            } else if r.atoms.len() > 1 {
                report.push(
                    Finding::new(
                        Severity::Warning,
                        path,
                        Some(number),
                        "multi-action-remap",
                        format!(
                            "A square-bracket line lists {} actions but no [t&hNNN] token.",
                            r.atoms.len()
                        ),
                    )
                    .with_hint(
                        "The guide describes one action per remap; multi-action sequences belong \
                         in a macro with curly braces.",
                    ),
                );
            }
        }
        LineKind::Macro(m) => {
            validate_macro_trigger(m, number, introducing, path, report);
            for atom in &m.body {
                if let MacroAtom::Action { stroke, token } = atom {
                    if !tokens::is_known(token) {
                        report.push(insertion_finding(
                            introducing,
                            path,
                            Some(number),
                            "unknown-action-token",
                            format!("Action token '{token}' is not in the official token list."),
                        ));
                    }
                    if *stroke == Stroke::Press && tokens::is_modifier(token) {
                        report.push(Finding::new(
                            Severity::Warning,
                            path,
                            Some(number),
                            "modifier-without-stroke",
                            format!(
                                "Modifier '{token}' is pressed without a matching '-' / '+' stroke."
                            ),
                        ));
                    }
                }
            }
        }
        LineKind::Led(l) => {
            if l.function == crate::model::LedFunction::Layer(LayerId::Base)
                || matches!(l.function, crate::model::LedFunction::Layer(_))
            {
                if l.colors.len() > 5 {
                    report.push(Finding::new(
                        Severity::Error,
                        path,
                        Some(number),
                        "led-too-many-colors",
                        format!(
                            "The layer LED accepts up to 5 colours (one per layer); {} were given.",
                            l.colors.len()
                        ),
                    ));
                }
            } else if l.colors.len() > 1 {
                report.push(Finding::new(
                    Severity::Warning,
                    path,
                    Some(number),
                    "led-extra-colors",
                    format!(
                        "Only the layer function accepts several colours; {} were given for one \
                         non-layer function.",
                        l.colors.len()
                    ),
                ));
            }
            if l.colors.is_empty() {
                report.push(Finding::new(
                    Severity::Warning,
                    path,
                    Some(number),
                    "led-no-color",
                    "No colour was given, so the indicator has nothing to show.",
                ));
            }
        }
    }
}

fn insertion_finding(
    introducing: bool,
    path: &str,
    line: Option<usize>,
    code: &'static str,
    message: String,
) -> Finding {
    if introducing {
        Finding::new(Severity::Error, path, line, code, message).with_hint(
            "This application is introducing this token; pick one from the documented action \
             catalog instead.",
        )
    } else {
        Finding::new(Severity::Warning, path, line, code, message).with_hint(
            "This token was already in your file. It is preserved exactly and never rewritten.",
        )
    }
}

fn validate_position(
    _file: &ConfigFile,
    position: &str,
    number: usize,
    introducing: bool,
    report: &mut ValidationReport,
) {
    let path = _file.relative_path.as_str();
    let Some(def) = positions::lookup(position) else {
        report.push(insertion_finding(
            introducing,
            path,
            Some(number),
            "unknown-position-token",
            format!("Position token '{position}' is not in the official Appendix A position map."),
        ));
        return;
    };
    if !def.programmable {
        report.push(
            Finding::new(
                Severity::Error,
                path,
                Some(number),
                "smartset-key-not-programmable",
                format!(
                    "'{position}' is the dedicated SmartSet key. The official position map gives \
                     it no position token and the guide excludes it from programmable keys."
                ),
            )
            .with_hint("Remove this line; the SmartSet key cannot be reassigned."),
        );
    }
    if def.requires_foot_pedal() {
        report.push(Finding::new(
            Severity::Info,
            path,
            Some(number),
            "foot-pedal-position",
            format!("'{}' is a foot pedal position ({}).", def.token, def.note),
        ));
    }
}

fn validate_remap_actions(
    r: &crate::model::RemapLine,
    number: usize,
    introducing: bool,
    path: &str,
    report: &mut ValidationReport,
) {
    for atom in &r.atoms {
        let RemapAtom::Action(token) = atom else {
            continue;
        };
        if !tokens::is_known(token) {
            report.push(insertion_finding(
                introducing,
                path,
                Some(number),
                "unknown-action-token",
                format!("Action token '{token}' is not in the official token list."),
            ));
        }
    }
}

fn validate_tap_and_hold(
    r: &crate::model::RemapLine,
    number: usize,
    introducing: bool,
    path: &str,
    report: &mut ValidationReport,
) {
    let tap_idx = r
        .atoms
        .iter()
        .position(|a| matches!(a, RemapAtom::TapHold(_)));
    let shape_ok = r.atoms.len() == 3 && tap_idx == Some(1);
    if !shape_ok {
        report.push(
            Finding::new(
                Severity::Error,
                path,
                Some(number),
                "tap-and-hold-shape",
                "Tap-and-Hold must be exactly [tap][t&hNNN][hold].",
            )
            .with_hint("The guide's example is [caps]>[caps][t&h500][esc]."),
        );
        return;
    }

    let sides = r.tap_actions().into_iter().chain(r.hold_actions());
    let mut already_reported: Vec<String> = Vec::new();
    for token in sides {
        if already_reported
            .iter()
            .any(|x| x.eq_ignore_ascii_case(token))
        {
            continue;
        }
        if let Some(t) = tokens::lookup(token) {
            if t.caps.tap_hold == crate::tokens::TapHoldSupport::No {
                already_reported.push(token.to_string());
                report.push(Finding::new(
                    Severity::Warning,
                    path,
                    Some(number),
                    "tap-and-hold-unsupported",
                    format!(
                        "'{token}' is a {} action; the guide says tap-and-hold is not recommended \
                         or not supported for such actions.",
                        match (t.caps.layer_action, t.caps.profile_action) {
                            (true, _) => "layer",
                            (_, true) => "profile",
                            _ => "special",
                        }
                    ),
                ));
            } else if t.caps.tap_hold == crate::tokens::TapHoldSupport::Unknown {
                already_reported.push(token.to_string());
                report.push(Finding::new(
                    Severity::Warning,
                    path,
                    Some(number),
                    "tap-and-hold-unverified",
                    "The guide states \"not all key actions support Tap-and-Hold\" without \
                     publishing which ones do; this assignment is unverified.",
                ));
            }
        }
    }
    let _ = introducing;
}

fn validate_macro_trigger(
    m: &crate::model::MacroLine,
    number: usize,
    introducing: bool,
    path: &str,
    report: &mut ValidationReport,
) {
    let is_action = tokens::is_known(&m.trigger);
    let is_position = positions::lookup(&m.trigger).is_some();

    if !is_action && !is_position {
        report.push(insertion_finding(
            introducing,
            path,
            Some(number),
            "unknown-trigger-token",
            format!(
                "Trigger '{0}' is neither a documented action token nor a documented position.",
                m.trigger
            ),
        ));
    } else if !is_action {
        // Kinesis's own section 4.6 example 3 is `{hk1}>{x3}{vol+}`, and the manual states the
        // hotkeys are blank by default, so naming the position is documented by example rather
        // than invented here.
        report.push(Finding::new(
            Severity::Info,
            path,
            Some(number),
            "position-as-trigger",
            format!(
                "'{0}' is a position token, not an action token. The guide's own macro example 3 \
                 uses one this way, so it is accepted and the macro follows the key itself.",
                m.trigger
            ),
        ));
    }
    if tokens::is_modifier(&m.trigger) {
        report.push(Finding::new(
            Severity::Error,
            path,
            Some(number),
            "modifier-as-trigger",
            format!(
                "'{}' is a modifier; the guide says any *non-modifier* key can trigger a macro.",
                m.trigger
            ),
        ));
    }

    if m.modifiers.len() > 4 {
        report.push(Finding::new(
            Severity::Error,
            path,
            Some(number),
            "too-many-co-triggers",
            format!(
                "{} co-triggers given; the manual allows up to four modifiers as co-triggers.",
                m.modifiers.len()
            ),
        ));
    }
    for c in &m.modifiers {
        if !tokens::is_modifier(c) {
            report.push(Finding::new(
                Severity::Error,
                path,
                Some(number),
                "co-trigger-not-modifier",
                format!("'{c}' is not a modifier, so it cannot be a co-trigger."),
            ));
        }
    }

    // The manual: the default layer shifting keys cannot trigger a macro via onboard
    // programming, but can via the SmartSet App when paired with a co-trigger.
    if let Some(t) = tokens::lookup(&m.trigger) {
        if t.caps.layer_action && m.modifiers.is_empty() {
            report.push(Finding::new(
                Severity::Warning,
                path,
                Some(number),
                "layer-key-trigger-without-co-trigger",
                format!(
                    "The manual says the default layer shifting keys cannot trigger a macro via \
                     onboard programming; '{0}' has no co-trigger.",
                    m.trigger
                ),
            ));
        }
    }

    if m.body.is_empty() {
        report.push(Finding::new(
            Severity::Error,
            path,
            Some(number),
            "empty-macro",
            "The macro has no content after '>'.",
        ));
    }
}

/// Warn when a macro's trigger action is produced by a *different* key in the same layer.
///
/// The guide is explicit that a macro is triggered by the action rather than by the physical
/// position, so this is a genuine and easy-to-miss interaction rather than a stylistic preference.
fn validate_macro_trigger_shadowing(file: &ConfigFile, report: &mut ValidationReport) {
    let path = file.relative_path.as_str();
    // (layer, action) -> the position that produces it, and where.
    let mut producers: Vec<(LayerId, String, String, usize)> = Vec::new();
    for (idx, line) in file.lines.iter().enumerate() {
        if !line.enabled {
            continue;
        }
        let Some(layer) = line.layer else { continue };
        if let LineKind::Remap(r) = &line.kind {
            for action in r.tap_actions() {
                producers.push((
                    layer,
                    action.to_ascii_lowercase(),
                    r.position.clone(),
                    idx + 1,
                ));
            }
        }
    }
    for (idx, line) in file.lines.iter().enumerate() {
        if !line.enabled {
            continue;
        }
        let (Some(layer), LineKind::Macro(m)) = (line.layer, &line.kind) else {
            continue;
        };
        let trigger = m.trigger.to_ascii_lowercase();
        for (pl, action, position, producer_line) in &producers {
            if *pl == layer && *action == trigger && !position.eq_ignore_ascii_case(&m.trigger) {
                report.push(Finding::new(
                    Severity::Warning,
                    path,
                    Some(idx + 1),
                    "macro-trigger-shadowed",
                    format!(
                        "This macro is triggered by the action '{0}', which line {producer_line} \
                         also makes '{position}' produce in this layer, so it will fire from both \
                         keys.",
                        m.trigger
                    ),
                ));
            }
        }
    }
}

fn validate_duplicates(file: &ConfigFile, report: &mut ValidationReport) {
    let path = file.relative_path.as_str();
    // (layer, position) already seen for a remap.
    let mut seen: Vec<(Option<LayerId>, String, usize)> = Vec::new();
    for (idx, line) in file.lines.iter().enumerate() {
        let LineKind::Remap(r) = &line.kind else {
            continue;
        };
        let key = (line.layer, r.position.to_ascii_lowercase());
        if let Some((_, _, first)) = seen.iter().find(|(l, p, _)| *l == key.0 && *p == key.1) {
            report.push(
                Finding::new(
                    Severity::Warning,
                    path,
                    Some(idx + 1),
                    "duplicate-remap",
                    format!(
                        "The same key is remapped again on line {first} for this layer; the guide \
                         says the command closest to the bottom of the file wins."
                    ),
                )
                .with_hint(
                    "Delete or disable the earlier line if you did not intend to override it.",
                ),
            );
        } else {
            seen.push((line.layer, r.position.to_ascii_lowercase(), idx + 1));
        }
    }

    // (indicator, layer-ish) duplicates for LEDs.
    let mut led_seen: Vec<(u8, usize)> = Vec::new();
    for (idx, line) in file.lines.iter().enumerate() {
        let LineKind::Led(l) = &line.kind else {
            continue;
        };
        let key = l.indicator.0;
        if led_seen.iter().any(|(k, _)| *k == key) {
            report.push(Finding::new(
                Severity::Warning,
                path,
                Some(idx + 1),
                "duplicate-led",
                format!(
                    "Indicator {} is defined more than once in this file.",
                    l.indicator.token()
                ),
            ));
        } else {
            led_seen.push((key, idx + 1));
        }
    }
}

fn validate_macro_budget(file: &ConfigFile, report: &mut ValidationReport) {
    let path = file.relative_path.as_str();
    let macros: Vec<(usize, &crate::model::MacroLine)> = file
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| match &l.kind {
            LineKind::Macro(m) => Some((i, m)),
            _ => None,
        })
        .collect();

    if macros.len() > 100 {
        report.push(Finding::new(
            Severity::Warning,
            path,
            None,
            "macro-count-limit",
            format!(
                "{} macros found; the guide says each layout stores up to 100 macros.",
                macros.len()
            ),
        ));
    }
    let total: usize = macros.iter().map(|(_, m)| m.action_count()).sum();
    if total > 7200 {
        report.push(Finding::new(
            Severity::Warning,
            path,
            None,
            "macro-token-budget",
            format!(
                "{total} macro action tokens in total; the guide says each layout stores up to \
                 7,200."
            ),
        ));
    }
    for (idx, m) in &macros {
        if m.action_count() > 300 {
            report.push(Finding::new(
                Severity::Warning,
                path,
                Some(idx + 1),
                "macro-too-long",
                format!(
                    "This macro has {} action tokens; the guide says a macro typically comprises \
                     up to about 300.",
                    m.action_count()
                ),
            ));
        }
    }
}

/// Validate a whole set of files as one report.
pub fn validate_files<'a, I>(files: I) -> ValidationReport
where
    I: IntoIterator<Item = &'a ConfigFile>,
{
    let mut report = ValidationReport::new();
    for f in files {
        report.extend(validate_file(f));
    }
    report.sorted()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_bytes;

    fn file(text: &str) -> ConfigFile {
        parse_bytes(
            text.as_bytes(),
            "layouts/layout1.txt",
            FileRole::ProfileLayout,
        )
        .unwrap()
    }

    fn codes(report: &ValidationReport) -> Vec<&str> {
        report.findings.iter().map(|f| f.code).collect()
    }

    #[test]
    fn a_clean_file_has_no_errors_or_warnings() {
        let f = file("<base>\n[esc]>[caps]\n<keypad>\n<function1>\n<function2>\n<function3>\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0, "{:?}", r.findings);
        assert_eq!(r.warning_count(), 0, "{:?}", r.findings);
        assert!(!r.blocks_write());
    }

    #[test]
    fn documented_examples_validate_cleanly() {
        let f = file(
            "<base>\n[hk1]>[q]\n[esc]>[caps]\n[caps]>[caps][t&h500][esc]\n\
             {tab}>{-lshf}{h}{+lshf}{i}\n{lctr}{hk4}>{s9}{q}{w}{e}{r}{t}{y}\n\
             {hk1}>{x3}{vol+}\n<keypad>\n<function1>\n<function2>\n<function3>\n",
        );
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0, "{:?}", r.findings);
    }

    #[test]
    fn a_syntax_error_blocks_writing() {
        let f = file("<base>\n[q]>[\n");
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(codes(&r).contains(&"syntax-error"));
        // The line is still there.
        assert_eq!(f.lines[1].raw, "[q]>[");
    }

    #[test]
    fn the_smartset_key_is_an_error_if_a_file_tries_to_program_it() {
        let f = file("<base>\n[ss]>[q]\n");
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(codes(&r).contains(&"smartset-key-not-programmable"));
    }

    #[test]
    fn an_unknown_token_already_in_the_file_is_a_warning_not_an_error() {
        let f = file("<base>\n[q]>[totally-unknown-token]\n");
        let r = validate_file(&f);
        assert_eq!(
            r.error_count(),
            0,
            "pre-existing data must not block: {:#?}",
            r.findings
        );
        assert!(codes(&r).contains(&"unknown-action-token"));
    }

    #[test]
    fn an_unknown_token_this_tool_introduces_is_an_error() {
        let mut f = file("<base>\n");
        let bad = crate::model::RemapLine::single("q", "totally-unknown-token");
        f.lines.push(crate::model::Line::inserted(
            bad.render(),
            crate::model::Eol::Lf,
            LineKind::Remap(bad),
            Some(LayerId::Base),
        ));
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(codes(&r).contains(&"unknown-action-token"));
    }

    #[test]
    fn an_unknown_position_already_in_the_file_is_a_warning() {
        let f = file("<base>\n[not-a-position]>[q]\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"unknown-position-token"));
    }

    #[test]
    fn a_remap_before_any_layer_header_is_an_error() {
        let f = file("[q]>[w]\n");
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(codes(&r).contains(&"no-layer-context"));
    }

    #[test]
    fn a_duplicate_remap_warns_about_the_documented_last_one_wins_rule() {
        let f = file("<base>\n[q]>[w]\n[q]>[e]\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"duplicate-remap"));
        assert!(r.findings.iter().any(|f| f.message.contains("line 2")));
    }

    #[test]
    fn the_same_position_in_two_layers_is_not_a_duplicate() {
        let f = file("<base>\n[q]>[w]\n<keypad>\n[q]>[e]\n");
        let r = validate_file(&f);
        assert!(!codes(&r).contains(&"duplicate-remap"));
    }

    #[test]
    fn multi_action_square_bracket_lines_without_tap_and_hold_warn() {
        let f = file("<base>\n[q]>[w][e]\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"multi-action-remap"));
    }

    #[test]
    fn a_broken_tap_and_hold_shape_is_an_error() {
        let f = file("<base>\n[q]>[t&h250][w]\n");
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(codes(&r).contains(&"tap-and-hold-shape"));
    }

    #[test]
    fn tap_and_hold_on_a_layer_key_warns_because_the_documentation_is_qualified() {
        let f = file("<base>\n[hk1]>[fn1s][t&h250][esc]\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"tap-and-hold-unsupported"));
    }

    #[test]
    fn tap_and_hold_on_a_plain_key_reports_that_the_documentation_is_silent() {
        let f = file("<base>\n[caps]>[caps][t&h250][esc]\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"tap-and-hold-unverified"));
    }

    #[test]
    fn a_modifier_cannot_be_a_macro_trigger() {
        let f = file("<base>\n{lshf}>{a}\n");
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(codes(&r).contains(&"modifier-as-trigger"));
    }

    #[test]
    fn five_co_triggers_is_an_error_because_the_manual_allows_four() {
        let f = file("<base>\n{lctr}{lshf}{lalt}{lwin}{rctr}{q}>{a}\n");
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(codes(&r).contains(&"too-many-co-triggers"));
    }

    #[test]
    fn a_non_modifier_co_trigger_is_an_error() {
        let f = file("<base>\n{q}{w}>{a}\n");
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(codes(&r).contains(&"co-trigger-not-modifier"));
    }

    #[test]
    fn a_layer_key_trigger_without_a_co_trigger_warns_as_the_manual_says() {
        let f = file("<base>\n{fn1s}>{a}\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"layer-key-trigger-without-co-trigger"));
    }

    #[test]
    fn a_pressed_modifier_in_a_macro_body_warns() {
        let f = file("<base>\n{tab}>{lshf}{h}\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"modifier-without-stroke"));
    }

    #[test]
    fn macro_size_limits_warn_with_the_documented_numbers() {
        let mut body = String::from("{tab}>");
        for _ in 0..301 {
            body.push_str("{a}");
        }
        let f = file(&format!("<base>\n{body}\n"));
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"macro-too-long"));
    }

    #[test]
    fn led_colour_rules_follow_the_guide() {
        // A layer LED may carry up to five colours.
        let ok = file("<base>\n[IND3]>[lay][255][000][000][000][255][000]\n");
        assert_eq!(validate_file(&ok).error_count(), 0);
        // Six is too many.
        let bad = file(
            "<base>\n[IND3]>[lay][255][000][000][000][255][000][000][000][255][000][000][255][000][000][255][000][000][255]\n",
        );
        assert!(validate_file(&bad).blocks_write());
        // A non-layer function takes one colour; more is a warning.
        let warn = file("<base>\n[IND2]>[prof][255][000][000][000][255][000]\n");
        assert_eq!(validate_file(&warn).error_count(), 0);
        assert!(codes(&validate_file(&warn)).contains(&"led-extra-colors"));
    }

    #[test]
    fn a_lighting_file_needs_no_layer_header() {
        // The guide's indicator syntax has no layer headers, so an LED line at the top of
        // lighting/ledN.txt must not be reported as having no layer.
        let f = parse_bytes(
            b"[IND1]>[caps][255][000][000]\n[IND2]>[prof]\n",
            "lighting/led1.txt",
            FileRole::ProfileLighting,
        )
        .unwrap();
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0, "{:#?}", r.errors().collect::<Vec<_>>());
        assert!(!codes(&r).contains(&"no-layer-context"));
        assert!(!r.blocks_write());
    }

    #[test]
    fn settings_txt_is_reported_as_never_written() {
        let f = parse_bytes(b"key=value\n", "settings.txt", FileRole::Settings).unwrap();
        let r = validate_file(&f);
        assert!(codes(&r).contains(&"role-read-only"));
        assert!(!r.blocks_write());
    }

    #[test]
    fn an_undocumented_instruction_is_info_only_and_is_kept() {
        let f = file("<base>\nweird vendor thing\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert_eq!(r.warning_count(), 0);
        assert!(codes(&r).contains(&"unknown-instruction"));
        assert_eq!(f.lines[1].raw, "weird vendor thing");
    }

    #[test]
    fn a_disabled_broken_line_still_reports_its_error() {
        // Disabling is a firmware feature; the line is still syntactically there, so this tool
        // keeps reporting it. It must be fixed, not silently ignored.
        let f = file("<base>\n*[q]>[\n");
        let r = validate_file(&f);
        assert!(r.blocks_write());
        assert!(r.findings.iter().any(|f| f.code == "disabled-line"));
    }

    #[test]
    fn a_file_missing_layers_is_only_informational() {
        let f = file("<base>\n[q]>[w]\n");
        let r = validate_file(&f);
        assert_eq!(r.error_count(), 0);
        assert!(codes(&r).contains(&"missing-layer-headers"));
    }

    #[test]
    fn findings_are_sorted_by_file_then_line() {
        let f = file("<base>\n[q]>[\n[zz]>[q]\n");
        let r = validate_file(&f);
        let lines: Vec<Option<usize>> = r.findings.iter().map(|f| f.line).collect();
        let mut sorted = lines.clone();
        sorted.sort();
        assert_eq!(lines, sorted);
    }
}
