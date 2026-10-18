//! The pure, GTK-free view model for the ADV360 desktop interface.
//!
//! Nothing in this module touches GTK, the filesystem, or the keyboard: it turns a
//! [`Session`] plus the interface's own [`ViewState`] into plain Rust data that a test can
//! assert on without a display. The widget layer in [`crate::ui`] only places these values.
//!
//! The key-map geometry is deliberately here rather than in the widget layer:
//! [`render_key_map`] returns one [`KeyCell`] per physical position in physical order, so the
//! shape of both modules can be tested headlessly.

use std::collections::BTreeMap;

use adv360_core::diff::Binding;
use adv360_core::kbd_layout::{self, KeyboardLayout};
use adv360_core::layer::{LayerId, ProfileId};
use adv360_core::model::MacroAtom;
use adv360_core::positions::{self, DefaultSource, Half, KeyBlock, PositionDef};
use adv360_core::presets;
use adv360_core::session::{self, KeyState, Session};
use adv360_core::tokens::{self, TokenCategory};
use adv360_core::validate::Severity;

/// The interface's own mutable selection, independent of the [`Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewState {
    /// The profile whose layer is being shown and edited. Profile 0 is shown but never edited.
    pub profile: ProfileId,
    /// The layer whose keys are being shown.
    pub layer: LayerId,
    /// The position token currently selected in the key map, when one is.
    pub position: Option<String>,
    /// Free text typed into the action-catalogue search box.
    pub search: String,
    /// The catalogue category filter, when one is applied.
    pub category: Option<TokenCategory>,
    /// Index into the list of files shown in the diff view.
    pub diff_file: usize,
    /// True when the preset review panel is open.
    pub show_presets: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            profile: ProfileId(1),
            layer: LayerId::Base,
            position: None,
            search: String::new(),
            category: None,
            diff_file: 0,
            show_presets: false,
        }
    }
}

/// One physical key as the key map draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCell {
    /// The static physical definition; carries the grid coordinates used for placement.
    pub position: &'static PositionDef,
    /// The keycap legend printed in profile 0.
    pub label: String,
    /// The current function: the custom binding, or the factory action.
    pub sub_label: String,
    /// True when a custom binding is in effect.
    pub custom: bool,
    /// True when this key is the selected one.
    pub selected: bool,
    /// The worst validation finding attached to this key, when there is one.
    pub worst_finding: Option<Severity>,
}

/// A per-key value used by [`render_key_map`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRender {
    pub token: String,
    pub keycap: String,
    pub current: String,
    pub custom: bool,
    pub uncertain: bool,
    pub disabled: bool,
    pub finding: Option<Severity>,
    pub lines: Vec<usize>,
}

/// What is on the v-Drive and how to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceView {
    pub root: String,
    /// False when the root path no longer exists (the keyboard was unplugged).
    pub available: bool,
    pub evidence: Vec<String>,
    pub mount_instructions: Vec<String>,
    pub eject_instructions: Vec<String>,
    pub skipped: Vec<(String, String)>,
}

/// One entry of the profile selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOption {
    pub profile: ProfileId,
    pub label: String,
    pub editable: bool,
    pub has_layout_file: bool,
    pub relative_path: Option<String>,
    pub changed_lines: usize,
    pub errors: usize,
    pub led_color_hint: String,
}

/// One layer tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerTab {
    pub layer: LayerId,
    pub label: String,
    pub header: String,
    pub led_color_hint: String,
    pub selected: bool,
}

/// The character preview shown next to a key or catalogue action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewView {
    pub token: String,
    pub usb_label: String,
    pub character: Option<String>,
    pub shifted: Option<String>,
    pub confident: bool,
    pub note: Option<String>,
}

/// One validation finding, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingView {
    pub severity: Severity,
    pub severity_label: String,
    pub relative_path: String,
    pub line: Option<usize>,
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
}

impl FindingView {
    /// A single line of text, used by the validation panel and by tests.
    pub fn one_line(&self) -> String {
        let location = match self.line {
            Some(n) => format!("{}:{}", self.relative_path, n),
            None => self.relative_path.clone(),
        };
        let hint = self
            .hint
            .as_ref()
            .map_or_else(String::new, |h| format!(" — hint: {h}"));
        format!(
            "[{}] {} [{}] {}{}",
            self.severity_label, location, self.code, self.message, hint
        )
    }
}

/// The key inspector: everything known about the selected key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorView {
    pub token: String,
    pub keycap: String,
    pub current: String,
    pub custom: bool,
    pub disabled: bool,
    pub lines: Vec<usize>,
    /// The factory action token.
    pub factory_action: String,
    /// A human label for the provenance of the factory action.
    pub factory_source: String,
    /// True when the factory action is not published for this key and layer.
    pub factory_uncertain: bool,
    /// The verbal explanation from the core, so nothing is presented as documented when it is not.
    pub factory_explanation: String,
    pub preview: Option<PreviewView>,
    pub findings: Vec<FindingView>,
}

/// One row of the searchable action catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueEntry {
    pub token: String,
    pub name: String,
    pub category: String,
    pub group: String,
    pub usage: String,
    pub tap_hold: String,
    pub modifier: bool,
    pub layer_action: bool,
    pub profile_action: bool,
}

/// The action catalogue and its filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueView {
    pub search: String,
    pub selected_category: Option<String>,
    pub categories: Vec<String>,
    pub entries: Vec<CatalogueEntry>,
    pub total_matches: usize,
}

/// One pending change, as shown before a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChangeView {
    pub description: String,
    pub file: String,
}

/// One file's textual diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiffView {
    pub relative_path: String,
    pub changed_lines: Vec<usize>,
    pub unified: String,
    pub identical: bool,
}

/// One row of a preset review table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetRowView {
    pub position: String,
    pub keycap: String,
    pub half: String,
    pub produces: String,
    pub line: String,
}

/// A reviewable preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetTableView {
    pub id: String,
    pub title: String,
    pub rationale: String,
    pub layer: LayerId,
    pub layer_header: String,
    pub rows: Vec<PresetRowView>,
    pub caveats: Vec<String>,
}

/// One way to make the navigation layer reachable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessKeyView {
    pub position: String,
    pub keycap: String,
    pub line: String,
    pub cost: Option<String>,
    pub recommended: bool,
}

/// The full preset review: both tables and the exact lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetReview {
    pub symbols: PresetTableView,
    pub navigation: PresetTableView,
    pub access_keys: Vec<AccessKeyView>,
    pub exact_lines: String,
}

/// Everything the write confirmation dialog must show before asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritePreview {
    pub profile: ProfileId,
    pub editable: bool,
    pub target_file: Option<String>,
    pub changed_lines: Vec<usize>,
    pub changed_line_count: usize,
    pub pending: Vec<PendingChangeView>,
    pub backup_root: String,
    pub backup_scope: String,
    pub eject_instructions: Vec<String>,
    pub validation_errors: usize,
    pub blocks_write: bool,
    pub has_file: bool,
}

impl WritePreview {
    /// The target path that will be written, or a clear "nothing to write" message.
    pub fn target_display(&self) -> String {
        match &self.target_file {
            Some(f) => f.clone(),
            None => "(no layout file for this profile on the v-Drive)".to_string(),
        }
    }

    /// True when a write could be attempted at all.
    pub fn can_write(&self) -> bool {
        self.editable && self.has_file && !self.blocks_write && self.changed_line_count > 0
    }
}

/// The keyboard layout and how much its character preview can be trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutView {
    pub layout: String,
    pub variant: Option<String>,
    pub source: String,
    pub detail: String,
    pub preview_reliable: bool,
    pub warning: Option<String>,
    pub supported: Vec<String>,
}

/// Everything on screen, as plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    pub device: DeviceView,
    pub profiles: Vec<ProfileOption>,
    pub layers: Vec<LayerTab>,
    pub selected_profile: ProfileId,
    pub selected_layer: LayerId,
    pub selected_position: Option<String>,
    pub keys: Vec<KeyRender>,
    pub key_index: BTreeMap<String, usize>,
    pub inspector: Option<InspectorView>,
    pub catalogue: CatalogueView,
    pub pending: Vec<PendingChangeView>,
    pub diffs: Vec<FileDiffView>,
    pub validation: Vec<FindingView>,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_descriptions: Vec<String>,
    pub redo_descriptions: Vec<String>,
    pub layout: LayoutView,
    pub presets: PresetReview,
    pub write: WritePreview,
}

impl ViewModel {
    /// Look up a rendered key by its position token, case-insensitively.
    pub fn key(&self, token: &str) -> Option<&KeyRender> {
        let needle = token.trim().to_ascii_lowercase();
        self.key_index.get(&needle).and_then(|i| self.keys.get(*i))
    }
}

/// Build the whole view model from the session and the interface's selection.
///
/// This is pure: it never mutates the session and never touches GTK or the filesystem.
pub fn build_view_model(session: &Session, view: &ViewState) -> ViewModel {
    let device = DeviceView {
        root: session.device().root.display().to_string(),
        available: session.device().root.exists(),
        evidence: session.device().evidence.clone(),
        mount_instructions: adv360_core::device::mount_instructions(),
        eject_instructions: adv360_core::device::eject_instructions(&session.device().root),
        skipped: session.skipped_files().to_vec(),
    };

    let profiles: Vec<ProfileOption> = session
        .profile_summary()
        .iter()
        .map(profile_option)
        .collect();

    let layers: Vec<LayerTab> = LayerId::all()
        .iter()
        .map(|l| LayerTab {
            layer: *l,
            label: l.short_label().to_string(),
            header: l.header().to_string(),
            led_color_hint: l.led_color_hint().to_string(),
            selected: *l == view.layer,
        })
        .collect();

    let mut keys: Vec<KeyRender> = session
        .layer_keys(view.profile, view.layer)
        .iter()
        .map(key_render)
        .collect();
    // The non-programmable SmartSet key is drawn but never addressed from a layout file.
    if let Some(ks) = session.key_state(view.profile, view.layer, "ss") {
        keys.push(key_render(&ks));
    }
    let mut key_index = BTreeMap::new();
    for (i, k) in keys.iter().enumerate() {
        key_index.insert(k.token.to_ascii_lowercase(), i);
    }

    let inspector = view
        .position
        .as_deref()
        .and_then(|pos| build_inspector(session, view, pos));

    let report = session.validation();
    let error_count = report.error_count();
    let warning_count = report.warning_count();
    let info_count = report.info_count();
    let validation: Vec<FindingView> = report.findings.iter().map(finding_view).collect();

    let diffs: Vec<FileDiffView> = session
        .diff()
        .files
        .iter()
        .map(|f| FileDiffView {
            relative_path: f.relative_path.clone(),
            changed_lines: f.changed_lines.clone(),
            unified: f.unified.clone(),
            identical: f.identical,
        })
        .collect();

    let pending: Vec<PendingChangeView> = session
        .pending_changes()
        .iter()
        .map(|p| PendingChangeView {
            description: p.description.clone(),
            file: p.file.clone(),
        })
        .collect();

    let catalogue = build_catalogue(view);
    let layout = layout_view(session.layout());
    let presets = build_preset_review();
    let write = build_write_preview(session, view, &pending);

    ViewModel {
        device,
        profiles,
        layers,
        selected_profile: view.profile,
        selected_layer: view.layer,
        selected_position: view.position.clone(),
        keys,
        key_index,
        inspector,
        catalogue,
        pending,
        diffs,
        validation,
        error_count,
        warning_count,
        info_count,
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
        undo_descriptions: session.undo_descriptions(),
        redo_descriptions: session.redo_descriptions(),
        layout,
        presets,
        write,
    }
}

/// Render the cells of one labelled block of one module, in physical order.
///
/// The returned vector is exactly [`positions::positions_in`] order, so the widget layer only
/// has to attach each cell at its `(x, y)` coordinate.
pub fn render_key_map(half: Half, block: KeyBlock, view: &ViewModel) -> Vec<KeyCell> {
    positions::positions_in(half, block)
        .into_iter()
        .map(|position| {
            let selected = view
                .selected_position
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(position.token));
            match view.key(position.token) {
                Some(k) => KeyCell {
                    position,
                    label: k.keycap.clone(),
                    sub_label: k.current.clone(),
                    custom: k.custom,
                    selected,
                    worst_finding: k.finding,
                },
                None if !position.programmable => KeyCell {
                    position,
                    label: position.keycap.to_string(),
                    sub_label: "not programmable".to_string(),
                    custom: false,
                    selected,
                    worst_finding: None,
                },
                None => KeyCell {
                    position,
                    label: position.keycap.to_string(),
                    sub_label: String::new(),
                    custom: false,
                    selected,
                    worst_finding: None,
                },
            }
        })
        .collect()
}

fn profile_option(p: &session::ProfileSummary) -> ProfileOption {
    ProfileOption {
        profile: p.profile,
        label: format!(
            "Profile {} - {}",
            p.profile.0,
            if p.editable {
                "editable"
            } else {
                "non-editable"
            }
        ),
        editable: p.editable,
        has_layout_file: p.has_layout_file,
        relative_path: p.relative_path.clone(),
        changed_lines: p.pending_changed_lines,
        errors: p.validation_errors,
        led_color_hint: p.led_color_hint.clone(),
    }
}

fn key_render(ks: &KeyState) -> KeyRender {
    let current = if !ks.position.programmable {
        "not programmable".to_string()
    } else {
        match &ks.binding {
            Some(b) => b.describe(),
            None if ks.default.source == DefaultSource::Uncertain => {
                format!("? {} (factory action not published)", ks.default.action)
            }
            None if ks.default.action == "null" => "no action".to_string(),
            None => ks.default.action.to_string(),
        }
    };
    KeyRender {
        token: ks.position.token.to_string(),
        keycap: ks.position.keycap.to_string(),
        current,
        custom: ks.is_custom(),
        uncertain: ks.is_default_uncertain(),
        disabled: ks.has_disabled_lines,
        finding: session::worst_severity(&ks.findings),
        lines: ks.lines.clone(),
    }
}

fn build_inspector(session: &Session, view: &ViewState, position: &str) -> Option<InspectorView> {
    let ks = session.key_state(view.profile, view.layer, position)?;
    let preview_token: Option<String> = match &ks.binding {
        Some(Binding::Remap(r)) => r.tap_actions().first().map(|s| s.to_string()),
        Some(Binding::Macro(m)) => m.body.iter().find_map(|a| match a {
            MacroAtom::Action { token, .. } if !tokens::is_modifier(token) => Some(token.clone()),
            _ => None,
        }),
        None => Some(ks.default.action.to_string()),
    };
    Some(InspectorView {
        token: ks.position.token.to_string(),
        keycap: ks.position.keycap.to_string(),
        current: ks.effective_description(),
        custom: ks.is_custom(),
        disabled: ks.has_disabled_lines,
        lines: ks.lines.clone(),
        factory_action: ks.default.action_token(),
        factory_source: default_source_label(ks.default.source).to_string(),
        factory_uncertain: !ks.default.is_documented(),
        factory_explanation: ks.effective_description(),
        preview: preview_token
            .as_deref()
            .map(|t| preview_view(t, session.layout())),
        findings: ks.findings.iter().map(finding_view).collect(),
    })
}

fn default_source_label(source: DefaultSource) -> &'static str {
    match source {
        DefaultSource::Documented => {
            "documented - printed for this key and layer in the official default layout map"
        }
        DefaultSource::DocumentedNoAction => "documented - the manual states this key is blank",
        DefaultSource::DocumentedSharedFunctionMap => {
            "documented - the manual's function-layer map is shared by Fn1, Fn2 and Fn3"
        }
        DefaultSource::InferredFromBase => {
            "uncertain - inferred from the base layer, not published for this layer"
        }
        DefaultSource::Uncertain => {
            "uncertain - no factory action is published for this position; it is not guessed"
        }
    }
}

fn preview_view(token: &str, layout: &KeyboardLayout) -> PreviewView {
    let p = kbd_layout::preview(token, layout);
    PreviewView {
        token: p.token.clone(),
        usb_label: p.usb_label.clone(),
        character: p.character.map(display_char),
        shifted: p.shifted_character.map(display_char),
        confident: p.confident,
        note: p.note.clone(),
    }
}

fn display_char(c: char) -> String {
    match c {
        '\n' => "\\n".to_string(),
        '\t' => "\\t".to_string(),
        ' ' => "(space)".to_string(),
        other => other.to_string(),
    }
}

fn finding_view(f: &adv360_core::validate::Finding) -> FindingView {
    FindingView {
        severity: f.severity,
        severity_label: f.severity.label().to_string(),
        relative_path: f.relative_path.clone(),
        line: f.line,
        code: f.code.to_string(),
        message: f.message.clone(),
        hint: f.hint.clone(),
    }
}

fn build_catalogue(view: &ViewState) -> CatalogueView {
    let mut entries = Vec::new();
    for t in tokens::search(&view.search) {
        if view.category.is_some_and(|category| t.category != category) {
            continue;
        }
        entries.push(CatalogueEntry {
            token: t.id.to_string(),
            name: t.name.to_string(),
            category: t.category.label().to_string(),
            group: group_label(t.group).to_string(),
            usage: t.usage.to_string(),
            tap_hold: tap_hold_label(t.caps.tap_hold).to_string(),
            modifier: t.caps.modifier,
            layer_action: t.caps.layer_action,
            profile_action: t.caps.profile_action,
        });
    }
    CatalogueView {
        search: view.search.clone(),
        selected_category: view.category.map(|c| c.label().to_string()),
        categories: TokenCategory::all()
            .iter()
            .map(|c| c.label().to_string())
            .collect(),
        total_matches: entries.len(),
        entries,
    }
}

fn group_label(group: tokens::TokenGroup) -> &'static str {
    match group {
        tokens::TokenGroup::Keyboard => "USB keyboard/keypad page",
        tokens::TokenGroup::Consumer => "USB consumer page",
        tokens::TokenGroup::Kinesis => "Kinesis custom token",
    }
}

fn tap_hold_label(support: tokens::TapHoldSupport) -> &'static str {
    match support {
        tokens::TapHoldSupport::No => "tap-and-hold not supported",
        tokens::TapHoldSupport::Unknown => "tap-and-hold unverified",
    }
}

fn layout_view(layout: &KeyboardLayout) -> LayoutView {
    LayoutView {
        layout: layout.layout.clone(),
        variant: layout.variant.clone(),
        source: layout.source.label().to_string(),
        detail: layout.detail.clone(),
        preview_reliable: layout.preview_is_reliable(),
        warning: layout.reliability_warning(),
        supported: kbd_layout::supported_layouts()
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

fn build_preset_review() -> PresetReview {
    let draft = presets::build_draft();
    PresetReview {
        symbols: preset_table(&draft.symbols),
        navigation: preset_table(&draft.navigation),
        access_keys: draft
            .access_key_options
            .iter()
            .map(|o| AccessKeyView {
                position: o.position.clone(),
                keycap: o.keycap.clone(),
                line: o.line.clone(),
                cost: o.cost.clone(),
                recommended: o.recommended,
            })
            .collect(),
        exact_lines: presets::render_layout_draft(&draft, None),
    }
}

fn preset_table(preset: &presets::LayerPreset) -> PresetTableView {
    PresetTableView {
        id: preset.id.clone(),
        title: preset.title.clone(),
        rationale: preset.rationale.clone(),
        layer: preset.layer,
        layer_header: preset.layer.header().to_string(),
        rows: preset
            .table()
            .iter()
            .map(|r| PresetRowView {
                position: r.position.clone(),
                keycap: r.keycap.clone(),
                half: r.half.clone(),
                produces: r.produces.clone(),
                line: r.line.clone(),
            })
            .collect(),
        caveats: preset.caveats.clone(),
    }
}

fn build_write_preview(
    session: &Session,
    view: &ViewState,
    pending: &[PendingChangeView],
) -> WritePreview {
    let profile = view.profile;
    let file_index = session.profile_file_index(profile);
    let target_file = file_index
        .and_then(|i| session.file(i))
        .map(|f| f.relative_path.clone());
    let changed_lines = file_index
        .and_then(|i| session.file_diff(i))
        .map_or_else(Vec::new, |d| d.changed_lines);
    let report = session.validation_for_write(profile);
    let pending: Vec<PendingChangeView> = pending
        .iter()
        .filter(|p| target_file.as_deref() == Some(p.file.as_str()))
        .cloned()
        .collect();
    WritePreview {
        profile,
        editable: profile.is_editable(),
        target_file,
        changed_line_count: changed_lines.len(),
        changed_lines,
        pending,
        backup_root: session.backup_root().display().to_string(),
        backup_scope: format!("profile{}", profile.0),
        eject_instructions: adv360_core::device::eject_instructions(&session.device().root),
        validation_errors: report.error_count(),
        blocks_write: report.blocks_write(),
        has_file: file_index.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_write_preview_target_display_handles_a_missing_file() {
        let preview = WritePreview {
            profile: ProfileId(1),
            editable: true,
            target_file: None,
            changed_lines: vec![],
            changed_line_count: 0,
            pending: vec![],
            backup_root: "/tmp/backups".into(),
            backup_scope: "profile1".into(),
            eject_instructions: vec![],
            validation_errors: 0,
            blocks_write: false,
            has_file: false,
        };
        assert!(preview.target_display().contains("no layout file"));
        assert!(!preview.can_write());
    }

    #[test]
    fn finding_views_render_their_line_and_hint() {
        let view = FindingView {
            severity: Severity::Error,
            severity_label: "error".into(),
            relative_path: "layouts/layout1.txt".into(),
            line: Some(4),
            code: "syntax-error".into(),
            message: "Malformed line".into(),
            hint: Some("Fix it".into()),
        };
        let line = view.one_line();
        assert!(line.contains("layouts/layout1.txt:4"));
        assert!(line.contains("syntax-error"));
        assert!(line.contains("Fix it"));
    }

    #[test]
    fn default_source_labels_never_call_an_uncertain_value_documented() {
        for source in [DefaultSource::InferredFromBase, DefaultSource::Uncertain] {
            let label = default_source_label(source);
            assert!(label.starts_with("uncertain"), "{label}");
        }
        for source in [
            DefaultSource::Documented,
            DefaultSource::DocumentedNoAction,
            DefaultSource::DocumentedSharedFunctionMap,
        ] {
            let label = default_source_label(source);
            assert!(label.starts_with("documented"), "{label}");
        }
    }
}
