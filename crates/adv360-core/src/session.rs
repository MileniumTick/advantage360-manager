//! The session: the one core both interfaces drive.
//!
//! A [`Session`] owns
//!
//! * the detected v-Drive and the files read from it, kept **twice** — an immutable `original`
//!   snapshot for diffing, and the editable draft;
//! * an undo/redo history of whole line edits;
//! * layout detection, validation, the diff, drafts, exports, backups and the guarded writer.
//!
//! Neither interface parses, validates, backs up or writes anything itself: they render
//! [`Session`] state and call these methods.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backup::{self, BackupSet};
use crate::device::{self, VDrive};
use crate::diff::{self, Binding, DiffSummary, KeyChange};
use crate::error::{CoreError, CoreResult};
use crate::kbd_layout::{self, KeyboardLayout};
use crate::layer::{LayerId, ProfileId};
use crate::model::{
    ConfigFile, Eol, FileRole, Indicator, LedFunction, LedLine, Line, LineKind, MacroLine, Origin,
    RemapLine, Rgb,
};
use crate::parser;
use crate::positions::{self, LayerDefault, PositionDef};
use crate::presets::Assignment;
use crate::validate::{self, Finding, Severity, ValidationReport};
use crate::writer;

/// How a session was opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionOrigin {
    /// A real or copied v-Drive found on disk.
    VDrive { root: PathBuf },
}

/// Everything the operator configured once for this run.
#[derive(Debug, Clone)]
pub struct SessionOptions {
    /// Use this path instead of searching for the v-Drive.
    pub device: Option<PathBuf>,
    /// Override the detected keyboard layout.
    pub layout_override: Option<(String, Option<String>)>,
    /// Where dated backups are written.
    pub backup_root: PathBuf,
}

impl Default for SessionOptions {
    fn default() -> Self {
        SessionOptions {
            device: None,
            layout_override: None,
            backup_root: backup::default_backup_root(),
        }
    }
}

/// A single line edit, invertible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    /// Index into the session's file list.
    pub file: usize,
    /// Index into that file's line list.
    pub index: usize,
    /// `None` when the edit inserted a line where there was none.
    pub before: Option<Line>,
    /// `None` when the edit removed a line.
    pub after: Option<Line>,
}

/// One operator action, which may touch several lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditBatch {
    /// What the operator did, in their words.
    pub description: String,
    pub edits: Vec<LineEdit>,
}

/// The state of one physical key in one layer, as the interfaces need to render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyState {
    pub position: &'static PositionDef,
    pub layer: LayerId,
    /// The custom configuration in effect, if any.
    pub binding: Option<Binding>,
    /// The factory action the key falls back to.
    pub default: LayerDefault,
    /// 1-based line numbers in the draft that address this key in this layer.
    pub lines: Vec<usize>,
    /// True when lines exist for this key but all of them are switched off with `*`.
    pub has_disabled_lines: bool,
    pub findings: Vec<Finding>,
}

impl KeyState {
    /// True when the key has custom configuration that is active.
    pub fn is_custom(&self) -> bool {
        self.binding.is_some()
    }

    /// A single line describing what the key will do.
    pub fn effective_description(&self) -> String {
        match &self.binding {
            Some(b) => b.describe(),
            None => {
                if self.default.source == positions::DefaultSource::Uncertain {
                    format!(
                        "factory action unknown (documented position, but the factory action is \
                         not published); currently sends {}",
                        self.default.action
                    )
                } else if self.default.action == "null" {
                    "no action (blank by default)".to_string()
                } else {
                    format!("factory action {}", self.default.action)
                }
            }
        }
    }

    /// True when the effective value is not published by Kinesis and must be presented as such.
    pub fn is_default_uncertain(&self) -> bool {
        self.binding.is_none() && !self.default.is_documented()
    }
}

/// A pending change, ready for the "what will change" list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingChange {
    pub description: String,
    pub file: String,
    pub key_change: Option<KeyChange>,
}

/// The explicit confirmation required before anything is written.
#[derive(Debug, Clone)]
pub struct WriteConfirmation {
    /// The profile the operator selected.
    pub profile: ProfileId,
    /// Must be `true`; the writer refuses otherwise.
    pub confirmed_by_operator: bool,
    /// The number of changed lines the operator saw before confirming.
    pub acknowledged_changed_lines: usize,
}

/// What a completed write did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteReport {
    pub profile: ProfileId,
    pub relative_path: String,
    pub bytes_written: usize,
    pub verified: bool,
    pub changed_lines: Vec<usize>,
    pub backup_directory: PathBuf,
    pub backup_files: Vec<String>,
    pub eject_instructions: Vec<String>,
    pub next_steps: Vec<String>,
}

/// The core session.
pub struct Session {
    origin: SessionOrigin,
    device: VDrive,
    /// The draft, index-aligned with `original`.
    files: Vec<ConfigFile>,
    /// The immutable snapshot taken when the session was opened.
    original: Vec<ConfigFile>,
    layout: KeyboardLayout,
    backup_root: PathBuf,
    undo_stack: Vec<EditBatch>,
    redo_stack: Vec<EditBatch>,
    /// The most recent backup, so a write can prove one exists.
    last_backup: Option<BackupSet>,
    /// Files that could not be read, with the reason.
    skipped: Vec<(String, String)>,
}

impl Session {
    /// Open a session on the keyboard's v-Drive, or on a copy of it.
    pub fn open(options: SessionOptions) -> CoreResult<Session> {
        let discovery = device::discover(options.device.as_deref())?;
        let drive = discovery.found.ok_or_else(|| CoreError::DeviceNotFound {
            detail: "discovery reported no drive".to_string(),
        })?;
        Self::from_drive(drive, options)
    }

    /// Open a session on a specific directory that must look like a v-Drive.
    pub fn open_at(root: &Path, options: SessionOptions) -> CoreResult<Session> {
        let evidence = device::assess(root).map_err(|reason| CoreError::NotAVDrive {
            root: root.to_path_buf(),
            missing: reason,
        })?;
        let drive = device::build(root, evidence)?;
        Self::from_drive(drive, options)
    }

    fn from_drive(drive: VDrive, options: SessionOptions) -> CoreResult<Session> {
        let (files, skipped) = drive.read_all();
        let original = files.clone();
        let layout = match options.layout_override {
            Some((l, v)) => KeyboardLayout::manual(l, v),
            None => kbd_layout::detect(),
        };
        Ok(Session {
            origin: SessionOrigin::VDrive {
                root: drive.root.clone(),
            },
            device: drive,
            files,
            original,
            layout,
            backup_root: options.backup_root,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_backup: None,
            skipped,
        })
    }

    // ---------------------------------------------------------------------------------------
    // Reading
    // ---------------------------------------------------------------------------------------

    pub fn origin(&self) -> &SessionOrigin {
        &self.origin
    }

    pub fn device(&self) -> &VDrive {
        &self.device
    }

    pub fn layout(&self) -> &KeyboardLayout {
        &self.layout
    }

    pub fn set_layout_override(&mut self, layout: String, variant: Option<String>) {
        self.layout = KeyboardLayout::manual(layout, variant);
    }

    pub fn draft_files(&self) -> &[ConfigFile] {
        &self.files
    }

    pub fn original_files(&self) -> &[ConfigFile] {
        &self.original
    }

    pub fn file(&self, index: usize) -> Option<&ConfigFile> {
        self.files.get(index)
    }

    pub fn original_file(&self, index: usize) -> Option<&ConfigFile> {
        self.original.get(index)
    }

    pub fn skipped_files(&self) -> &[(String, String)] {
        &self.skipped
    }

    pub fn backup_root(&self) -> &Path {
        &self.backup_root
    }

    pub fn last_backup(&self) -> Option<&BackupSet> {
        self.last_backup.as_ref()
    }

    /// The index of the file holding a profile's layout, when it exists.
    pub fn profile_file_index(&self, profile: ProfileId) -> Option<usize> {
        self.files
            .iter()
            .position(|f| f.role == FileRole::ProfileLayout && f.profile == Some(profile))
    }

    /// The index of the file holding a profile's indicator lighting, when it exists.
    pub fn lighting_file_index(&self, profile: ProfileId) -> Option<usize> {
        self.files
            .iter()
            .position(|f| f.role == FileRole::ProfileLighting && f.profile == Some(profile))
    }

    /// The profiles that have a layout file on the drive, in order.
    pub fn available_profiles(&self) -> Vec<ProfileId> {
        let mut v: Vec<ProfileId> = self
            .files
            .iter()
            .filter(|f| f.role == FileRole::ProfileLayout)
            .filter_map(|f| f.profile)
            .collect();
        v.sort_by_key(|p| p.0);
        v.dedup();
        v
    }

    /// The nine programmable profiles, marking the ones with no file yet.
    pub fn profile_summary(&self) -> Vec<ProfileSummary> {
        ProfileId::all()
            .map(|p| {
                let idx = self.profile_file_index(p);
                let changes = idx.map(|i| self.file_change_count(i)).unwrap_or(0);
                let errors = idx
                    .map(|i| validate::validate_file(&self.files[i]).errors().count())
                    .unwrap_or(0);
                ProfileSummary {
                    profile: p,
                    editable: p.is_editable(),
                    has_layout_file: idx.is_some(),
                    has_lighting_file: self.lighting_file_index(p).is_some(),
                    relative_path: idx.map(|i| self.files[i].relative_path.clone()),
                    pending_changed_lines: changes,
                    validation_errors: errors,
                    led_color_hint: p.led_color_hint().to_string(),
                }
            })
            .collect()
    }

    /// How many lines of a file differ from the v-Drive.
    pub fn file_change_count(&self, index: usize) -> usize {
        let Some(f) = self.files.get(index) else {
            return 0;
        };
        let before = self.original.get(index).map(|o| o.render());
        writer::changed_line_numbers(before.as_deref(), &f.render()).len()
    }

    // ---------------------------------------------------------------------------------------
    // Key state
    // ---------------------------------------------------------------------------------------

    /// The state of one key in one layer of one profile.
    pub fn key_state(
        &self,
        profile: ProfileId,
        layer: LayerId,
        position: &str,
    ) -> Option<KeyState> {
        let def = positions::lookup(position)?;
        let file_index = self.profile_file_index(profile);
        let mut lines = Vec::new();
        let mut has_disabled = false;
        let mut binding: Option<Binding> = None;
        let mut findings = Vec::new();

        if let Some(idx) = file_index {
            let file = &self.files[idx];
            for (i, line) in file.lines.iter().enumerate() {
                if line.layer != Some(layer) {
                    continue;
                }
                let matches = match &line.kind {
                    LineKind::Remap(r) => r.position.eq_ignore_ascii_case(def.token),
                    LineKind::Macro(m) => m.trigger.eq_ignore_ascii_case(def.token),
                    _ => false,
                };
                if !matches {
                    continue;
                }
                lines.push(i + 1);
                if line.enabled {
                    // The guide: the command closest to the bottom wins.
                    binding = match &line.kind {
                        LineKind::Remap(r) => Some(Binding::Remap(r.clone())),
                        LineKind::Macro(m) => Some(Binding::Macro(m.clone())),
                        _ => binding,
                    };
                } else {
                    has_disabled = true;
                }
            }
            let report = self.validation();
            for l in &lines {
                findings.extend(
                    report
                        .for_line(&file.relative_path, *l)
                        .into_iter()
                        .cloned(),
                );
            }
        }

        Some(KeyState {
            position: def,
            layer,
            binding,
            default: positions::documented_default(def, layer),
            lines,
            has_disabled_lines: has_disabled,
            findings,
        })
    }

    /// Every key of one layer of one profile, in physical order.
    pub fn layer_keys(&self, profile: ProfileId, layer: LayerId) -> Vec<KeyState> {
        positions::programmable()
            .filter_map(|p| self.key_state(profile, layer, p.token))
            .collect()
    }

    /// The factory action of a key in a layer, without needing a session file.
    pub fn documented_default(position: &str, layer: LayerId) -> Option<LayerDefault> {
        positions::lookup(position).map(|p| positions::documented_default(p, layer))
    }

    // ---------------------------------------------------------------------------------------
    // Editing
    // ---------------------------------------------------------------------------------------

    /// True when there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Descriptions of the edits that can be undone, newest first.
    pub fn undo_descriptions(&self) -> Vec<String> {
        self.undo_stack
            .iter()
            .rev()
            .map(|b| b.description.clone())
            .collect()
    }

    pub fn redo_descriptions(&self) -> Vec<String> {
        self.redo_stack
            .iter()
            .rev()
            .map(|b| b.description.clone())
            .collect()
    }

    /// Undo the most recent action. Returns its description.
    pub fn undo(&mut self) -> Option<String> {
        let batch = self.undo_stack.pop()?;
        self.apply_batch(&batch, true);
        let description = batch.description.clone();
        self.redo_stack.push(batch);
        Some(description)
    }

    /// Redo the most recently undone action.
    pub fn redo(&mut self) -> Option<String> {
        let batch = self.redo_stack.pop()?;
        self.apply_batch(&batch, false);
        let description = batch.description.clone();
        self.undo_stack.push(batch);
        Some(description)
    }

    fn apply_batch(&mut self, batch: &EditBatch, reverse: bool) {
        let edits: Vec<&LineEdit> = if reverse {
            batch.edits.iter().rev().collect()
        } else {
            batch.edits.iter().collect()
        };
        for edit in edits {
            let Some(file) = self.files.get_mut(edit.file) else {
                continue;
            };
            if reverse {
                match (&edit.before, &edit.after) {
                    (None, Some(_)) => {
                        if edit.index < file.lines.len() {
                            file.lines.remove(edit.index);
                        }
                    }
                    (Some(before), None) => {
                        file.lines
                            .insert(edit.index.min(file.lines.len()), before.clone());
                    }
                    (Some(before), Some(_)) => {
                        if edit.index < file.lines.len() {
                            file.lines[edit.index] = before.clone();
                        }
                    }
                    (None, None) => {}
                }
            } else {
                match (&edit.before, &edit.after) {
                    (None, Some(after)) => {
                        file.lines
                            .insert(edit.index.min(file.lines.len()), after.clone());
                    }
                    (Some(_), None) => {
                        if edit.index < file.lines.len() {
                            file.lines.remove(edit.index);
                        }
                    }
                    (Some(_), Some(after)) => {
                        if edit.index < file.lines.len() {
                            file.lines[edit.index] = after.clone();
                        }
                    }
                    (None, None) => {}
                }
            }
        }
        for f in &mut self.files {
            f.dirty = true;
        }
    }

    fn record(&mut self, batch: EditBatch) {
        if batch.edits.is_empty() {
            return;
        }
        self.undo_stack.push(batch);
        self.redo_stack.clear();
    }

    /// Set, replace or remove the binding of one key in one layer.
    ///
    /// `None` removes it, which restores the factory action. Removal is done the way the firmware
    /// documents it — the line is switched off with a leading `*` — so the original text is never
    /// destroyed and the change is trivially undoable.
    pub fn set_binding(
        &mut self,
        profile: ProfileId,
        layer: LayerId,
        position: &str,
        assignment: Option<Assignment>,
    ) -> CoreResult<String> {
        if !profile.is_editable() {
            return Err(CoreError::ProfileZeroNotEditable);
        }
        let def = positions::lookup(position).ok_or_else(|| CoreError::UnknownPositionToken {
            token: position.to_string(),
        })?;
        if !def.programmable {
            return Err(CoreError::RefusedWrite {
                reason: format!(
                    "'{position}' is the dedicated SmartSet key; the official position map gives \
                     it no position token and it cannot be programmed"
                ),
            });
        }
        let file_index =
            self.profile_file_index(profile)
                .ok_or_else(|| CoreError::ReadOnlyFile {
                    path: PathBuf::from(profile.layout_file_name()),
                    role: "not present on the v-Drive",
                })?;

        let new_line = match &assignment {
            Some(a) => Some(self.build_line(a, layer)?),
            None => None,
        };

        let description = match &assignment {
            Some(a) => format!(
                "{} in {}, {}: set to {}",
                def.token,
                layer.short_label(),
                profile,
                a.produces()
            ),
            None => format!(
                "{} in {}, {}: removed, factory action restored",
                def.token,
                layer.short_label(),
                profile
            ),
        };

        let mut edits: Vec<LineEdit> = Vec::new();
        let file = &mut self.files[file_index];

        // Every active line in this layer that addresses this key.
        let targets: Vec<usize> = file
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.layer == Some(layer)
                    && match &l.kind {
                        LineKind::Remap(r) => r.position.eq_ignore_ascii_case(def.token),
                        LineKind::Macro(m) => m.trigger.eq_ignore_ascii_case(def.token),
                        _ => false,
                    }
            })
            .map(|(i, _)| i)
            .collect();

        match new_line {
            Some(new_line) => {
                if let Some(&last) = targets.last() {
                    // Replace the effective line in place; switch off any earlier ones so they
                    // cannot win and are not lost.
                    for &idx in targets.iter().take(targets.len() - 1) {
                        if file.lines[idx].enabled {
                            let mut disabled = file.lines[idx].clone();
                            disabled.set_enabled(false);
                            edits.push(LineEdit {
                                file: file_index,
                                index: idx,
                                before: Some(file.lines[idx].clone()),
                                after: Some(disabled.clone()),
                            });
                            file.lines[idx] = disabled;
                        }
                    }
                    let before = file.lines[last].clone();
                    let mut after = new_line;
                    // Keep the original line ending and disabled state of the line we replace.
                    after.eol = before.eol;
                    if !before.enabled {
                        after.set_enabled(false);
                    }
                    edits.push(LineEdit {
                        file: file_index,
                        index: last,
                        before: Some(before),
                        after: Some(after.clone()),
                    });
                    file.lines[last] = after;
                } else {
                    let index = self.insertion_index(file_index, layer);
                    edits.extend(self.insert_line_at(file_index, index, new_line));
                }
            }
            None => {
                for &idx in &targets {
                    if !self.files[file_index].lines[idx].enabled {
                        continue;
                    }
                    let before = self.files[file_index].lines[idx].clone();
                    let mut disabled = before.clone();
                    disabled.set_enabled(false);
                    edits.push(LineEdit {
                        file: file_index,
                        index: idx,
                        before: Some(before.clone()),
                        after: Some(disabled.clone()),
                    });
                    self.files[file_index].lines[idx] = disabled;
                }
                if edits.is_empty() {
                    return Err(CoreError::Other(format!(
                        "{} in {} has no active custom line, so there is nothing to remove",
                        def.token,
                        layer.short_label()
                    )));
                }
            }
        }

        self.files[file_index].dirty = true;
        self.record(EditBatch { description, edits });
        Ok(self
            .undo_stack
            .last()
            .map(|b| b.description.clone())
            .unwrap_or_default())
    }

    /// Turn a preset assignment into a draft line.
    fn build_line(&self, assignment: &Assignment, layer: LayerId) -> CoreResult<Line> {
        let (content, kind) = match assignment {
            Assignment::Remap { position, action } => {
                let r = RemapLine::single(position.clone(), action.clone());
                (r.render(), LineKind::Remap(r))
            }
            Assignment::TapAndHold {
                position,
                tap,
                delay_ms,
                hold,
            } => {
                let r =
                    RemapLine::tap_and_hold(position.clone(), tap.clone(), *delay_ms, hold.clone());
                (r.render(), LineKind::Remap(r))
            }
            Assignment::Macro { macro_line, .. } => {
                let m: MacroLine = macro_line.clone();
                (m.render(), LineKind::Macro(m))
            }
        };
        Ok(Line::inserted(content, Eol::Lf, kind, Some(layer)))
    }

    /// Where a new line for `layer` should go: after the last line already in that layer.
    fn insertion_index(&self, file_index: usize, layer: LayerId) -> usize {
        let file = &self.files[file_index];
        let mut index = None;
        for (i, l) in file.lines.iter().enumerate() {
            if l.layer == Some(layer) {
                index = Some(i + 1);
            }
        }
        index.unwrap_or(file.lines.len())
    }

    /// Insert a line, keeping every line except the last terminated.
    ///
    /// A file that ends without a newline must gain one before anything is appended, otherwise the
    /// new line would be glued to the end of the previous one. That adjustment is recorded as its
    /// own invertible edit so undo restores the file exactly.
    fn insert_line_at(&mut self, file_index: usize, index: usize, mut line: Line) -> Vec<LineEdit> {
        let eol = self.preferred_eol(file_index);
        let mut edits = Vec::new();
        if index == self.files[file_index].lines.len() && index > 0 {
            let last_index = index - 1;
            let last = self.files[file_index].lines[last_index].clone();
            if last.eol == Eol::None {
                let mut fixed = last.clone();
                fixed.eol = eol;
                self.files[file_index].lines[last_index] = fixed.clone();
                edits.push(LineEdit {
                    file: file_index,
                    index: last_index,
                    before: Some(last),
                    after: Some(fixed),
                });
            }
        }
        // A brand-new line follows whatever the file already uses, so a CRLF file stays CRLF.
        line.eol = eol;
        let after = line.clone();
        self.files[file_index].lines.insert(index, line);
        self.files[file_index].dirty = true;
        edits.push(LineEdit {
            file: file_index,
            index,
            before: None,
            after: Some(after),
        });
        edits
    }

    /// Append raw text as a new line at the end of a layer's block.
    ///
    /// Returns the index of the new line. Malformed text is accepted here on purpose: the operator
    /// sees it flagged by validation and writing is blocked until it is fixed, which is clearer
    /// than silently refusing the edit.
    pub fn append_line_raw(
        &mut self,
        file_index: usize,
        layer: LayerId,
        content: String,
    ) -> CoreResult<usize> {
        if self.files.get(file_index).is_none() {
            return Err(CoreError::Other(format!("no file at index {file_index}")));
        }
        let index = self.insertion_index(file_index, layer);
        let mut line = parser::parse_line(content, Eol::Lf, Some(layer));
        if matches!(line.kind, LineKind::LayerHeader(_)) {
            return Err(CoreError::RefusedWrite {
                reason: "a layer header is structural and cannot be added as a plain line"
                    .to_string(),
            });
        }
        line.layer = Some(layer);
        line.origin = Origin::Inserted;
        let parsed_kind = line.kind.clone();
        let edits = self.insert_line_at(file_index, index, line);
        self.record(EditBatch {
            description: format!(
                "{}: line added to {}: {}",
                self.files[file_index].relative_path,
                layer.short_label(),
                parsed_kind.describe()
            ),
            edits,
        });
        Ok(index)
    }

    fn preferred_eol(&self, file_index: usize) -> Eol {
        let file = &self.files[file_index];
        // Match whatever the file already uses, so a CRLF file stays a CRLF file.
        if file.has_crlf() {
            return Eol::CrLf;
        }
        if file.lines.is_empty() {
            return Eol::Lf;
        }
        // A file whose last line has no terminator needs one added before we append.
        for l in file.lines.iter().rev() {
            if l.eol != Eol::None {
                return l.eol;
            }
        }
        Eol::Lf
    }

    /// Replace a line's content with raw text the operator typed.
    ///
    /// The text is parsed immediately. If it is malformed the edit still happens — nothing is
    /// silently corrected — but validation will block writing until it is fixed.
    pub fn set_line_raw(
        &mut self,
        file_index: usize,
        line_index: usize,
        content: String,
    ) -> CoreResult<String> {
        let file = self
            .files
            .get_mut(file_index)
            .ok_or_else(|| CoreError::Other(format!("no file at index {file_index}")))?;
        let line = file
            .lines
            .get(line_index)
            .ok_or_else(|| CoreError::Other(format!("no line at index {line_index}")))?
            .clone();
        if line.kind == LineKind::LayerHeader(layer_of(&line)) {
            // A header line is structural; rewriting it would move every following line.
            return Err(CoreError::RefusedWrite {
                reason: "a layer header is structural: change the layer with the layer selector \
                         instead of editing the line"
                    .to_string(),
            });
        }
        let layer = line.layer;
        let mut new_line = parser::parse_line(content.clone(), line.eol, layer);
        // A user must not be able to introduce a layer header by typing one: that would silently
        // reparent every following line.
        if matches!(new_line.kind, LineKind::LayerHeader(_)) {
            return Err(CoreError::RefusedWrite {
                reason: "a layer header is structural: it decides which layer the following lines \
                         belong to, so it cannot be introduced by editing a line"
                    .to_string(),
            });
        }
        new_line.layer = layer;
        new_line.origin = Origin::Edited;
        file.lines[line_index] = new_line.clone();
        file.dirty = true;
        let description = format!("{}: line {} edited", file.relative_path, line_index + 1);
        self.record(EditBatch {
            description,
            edits: vec![LineEdit {
                file: file_index,
                index: line_index,
                before: Some(line),
                after: Some(new_line),
            }],
        });
        Ok(content)
    }

    /// Switch a line on or off with the documented leading `*`.
    pub fn set_line_enabled(
        &mut self,
        file_index: usize,
        line_index: usize,
        enabled: bool,
    ) -> CoreResult<()> {
        let file = self
            .files
            .get_mut(file_index)
            .ok_or_else(|| CoreError::Other(format!("no file at index {file_index}")))?;
        let before = file
            .lines
            .get(line_index)
            .ok_or_else(|| CoreError::Other(format!("no line at index {line_index}")))?
            .clone();
        if before.enabled == enabled {
            return Ok(());
        }
        let mut after = before.clone();
        after.set_enabled(enabled);
        file.lines[line_index] = after.clone();
        file.dirty = true;
        let description = format!(
            "{}: line {} {}",
            file.relative_path,
            line_index + 1,
            if enabled {
                "enabled"
            } else {
                "switched off with '*'"
            }
        );
        self.record(EditBatch {
            description,
            edits: vec![LineEdit {
                file: file_index,
                index: line_index,
                before: Some(before),
                after: Some(after),
            }],
        });
        Ok(())
    }

    /// Set an indicator LED definition, replacing the last matching line for that indicator.
    pub fn set_led(
        &mut self,
        profile: ProfileId,
        indicator: Indicator,
        function: LedFunction,
        colors: Vec<Rgb>,
    ) -> CoreResult<()> {
        if !profile.is_editable() {
            return Err(CoreError::ProfileZeroNotEditable);
        }
        let file_index =
            self.lighting_file_index(profile)
                .ok_or_else(|| CoreError::ReadOnlyFile {
                    path: PathBuf::from(profile.lighting_file_name()),
                    role: "indicator file not present",
                })?;
        let led = LedLine {
            indicator,
            function,
            colors,
        };
        let target = self.files[file_index].lines.iter().rposition(|l| {
            matches!(&l.kind, LineKind::Led(x) if x.indicator == indicator) && l.enabled
        });
        let relative_path = self.files[file_index].relative_path.clone();
        let eol = self.preferred_eol(file_index);
        match target {
            Some(idx) => {
                let before = self.files[file_index].lines[idx].clone();
                let mut after = Line::inserted(led.render(), Eol::Lf, LineKind::Led(led), None);
                after.eol = before.eol;
                after.layer = before.layer;
                self.files[file_index].lines[idx] = after.clone();
                self.files[file_index].dirty = true;
                self.record(EditBatch {
                    description: format!("{}: {} set", relative_path, indicator.token()),
                    edits: vec![LineEdit {
                        file: file_index,
                        index: idx,
                        before: Some(before),
                        after: Some(after),
                    }],
                });
            }
            None => {
                let idx = self.files[file_index].lines.len();
                let mut after = Line::inserted(led.render(), Eol::Lf, LineKind::Led(led), None);
                after.eol = eol;
                self.files[file_index].lines.push(after.clone());
                self.files[file_index].dirty = true;
                self.record(EditBatch {
                    description: format!("{}: {} added", relative_path, indicator.token()),
                    edits: vec![LineEdit {
                        file: file_index,
                        index: idx,
                        before: None,
                        after: Some(after),
                    }],
                });
            }
        }
        Ok(())
    }

    /// Apply a whole preset to one profile.
    ///
    /// Each assignment goes through [`Session::set_binding`], so every single change is
    /// undoable, appears in the diff, and is validated.
    pub fn apply_preset(
        &mut self,
        profile: ProfileId,
        preset: &crate::presets::LayerPreset,
    ) -> CoreResult<usize> {
        let mut applied = 0usize;
        for a in &preset.assignments {
            self.set_binding(profile, preset.layer, a.position(), Some(a.clone()))?;
            applied += 1;
        }
        Ok(applied)
    }

    /// Apply an access-key line under the Base layer.
    pub fn apply_access_key(
        &mut self,
        profile: ProfileId,
        option: &crate::presets::AccessKeyOption,
    ) -> CoreResult<()> {
        let parsed = parser::parse_line(option.line.clone(), Eol::Lf, Some(LayerId::Base));
        let LineKind::Remap(r) = parsed.kind.clone() else {
            return Err(CoreError::Other(format!(
                "the access-key line '{}' is malformed",
                option.line
            )));
        };
        self.set_binding(
            profile,
            LayerId::Base,
            &r.position,
            Some(Assignment::Remap {
                position: r.position.clone(),
                action: r.tap_actions().first().copied().unwrap_or("").to_string(),
            }),
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // Diff, validation, changes
    // ---------------------------------------------------------------------------------------

    /// Everything that would change if the draft were written.
    pub fn diff(&self) -> DiffSummary {
        let mut summary = DiffSummary::default();
        for (i, draft) in self.files.iter().enumerate() {
            let on_disk = self.original.get(i).map(|o| o.render()).unwrap_or_default();
            summary.files.push(diff::file_diff(
                &draft.relative_path,
                Some(&on_disk),
                &draft.render(),
            ));
            if let Some(orig) = self.original.get(i) {
                summary.key_changes.extend(diff::key_changes(orig, draft));
            }
        }
        summary.key_changes.sort_by(|a, b| {
            (a.layer as u8)
                .cmp(&(b.layer as u8))
                .then(a.target.cmp(&b.target))
        });
        summary
    }

    /// The diff of one file.
    pub fn file_diff(&self, index: usize) -> Option<crate::diff::FileDiff> {
        let draft = self.files.get(index)?;
        let on_disk = self.original.get(index).map(|o| o.render())?;
        Some(diff::file_diff(
            &draft.relative_path,
            Some(&on_disk),
            &draft.render(),
        ))
    }

    /// Validate every file in the draft.
    pub fn validation(&self) -> ValidationReport {
        validate::validate_files(self.files.iter())
    }

    /// Validate only the writable files of a profile set.
    pub fn validation_for_write(&self, profile: ProfileId) -> ValidationReport {
        match self.profile_file_index(profile) {
            Some(idx) => validate::validate_file(&self.files[idx]),
            None => ValidationReport::new(),
        }
    }

    /// The pending list shown before a write.
    pub fn pending_changes(&self) -> Vec<PendingChange> {
        let mut out = Vec::new();
        for (i, draft) in self.files.iter().enumerate() {
            let Some(orig) = self.original.get(i) else {
                continue;
            };
            let mut described_a_key_change = false;
            for kc in diff::key_changes(orig, draft) {
                described_a_key_change = true;
                out.push(PendingChange {
                    description: kc.describe(),
                    file: draft.relative_path.clone(),
                    key_change: Some(kc),
                });
            }
            let fd = diff::file_diff(&draft.relative_path, Some(&orig.render()), &draft.render());
            if !fd.changed_lines.is_empty() && !described_a_key_change {
                out.push(PendingChange {
                    description: format!(
                        "{} line(s) differ but are not key remaps (comments, unknown instructions \
                         or indicator lines)",
                        fd.changed_lines.len()
                    ),
                    file: draft.relative_path.clone(),
                    key_change: None,
                });
            }
        }
        out
    }

    /// A compact textual summary of the draft, for the CLI and for saving.
    pub fn summary(&self) -> SessionSummary {
        let diff = self.diff();
        let validation = self.validation();
        SessionSummary {
            device_root: self.device.root.clone(),
            evidence: self.device.evidence.clone(),
            files: self
                .files
                .iter()
                .map(|f| FileSummary {
                    relative_path: f.relative_path.clone(),
                    role: f.role,
                    profile: f.profile,
                    lines: f.lines.len(),
                    bytes: f.render().len(),
                })
                .collect(),
            profiles: self.profile_summary(),
            changed_lines: diff.changed_line_count(),
            key_changes: diff.key_changes.len(),
            errors: validation.error_count(),
            warnings: validation.warning_count(),
            layout: self.layout.clone(),
            undo_depth: self.undo_stack.len(),
            redo_depth: self.redo_stack.len(),
            skipped_files: self.skipped.clone(),
        }
    }

    // ---------------------------------------------------------------------------------------
    // Drafts and export
    // ---------------------------------------------------------------------------------------

    /// Render every draft file, keyed by its relative path.
    pub fn render_all(&self) -> BTreeMap<String, String> {
        self.files
            .iter()
            .map(|f| (f.relative_path.clone(), f.render()))
            .collect()
    }

    /// Write the drafts to a directory, preserving the v-Drive structure. Never touches the
    /// keyboard.
    pub fn export_to(&self, directory: &Path) -> CoreResult<Vec<PathBuf>> {
        fs::create_dir_all(directory).map_err(|source| CoreError::CreateDir {
            path: directory.to_path_buf(),
            source,
        })?;
        let mut written = Vec::new();
        for f in &self.files {
            let target = directory.join(&f.relative_path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|source| CoreError::CreateDir {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            writer::write_verified(&target, &f.render())?;
            written.push(target);
        }
        let manifest = self.draft_manifest();
        let path = directory.join("adv360-draft.json");
        writer::write_verified(&path, &manifest)?;
        written.push(path);
        Ok(written)
    }

    fn draft_manifest(&self) -> String {
        let manifest = serde_json::json!({
            "tool": "adv360-config",
            "kind": "adv360-draft",
            "created_at": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            "device_root": self.device.root.to_string_lossy(),
            "keyboard_layout": self.layout.layout,
            "profiles_with_layout": self.available_profiles().iter().map(|p| p.0).collect::<Vec<_>>(),
            "undo_depth": self.undo_stack.len(),
            "undo_history": self.undo_descriptions(),
            "changed_lines": self.diff().changed_line_count(),
        });
        serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string())
    }

    /// Replace the draft with the contents of an exported directory.
    ///
    /// Only files that exist both in the export and in the session are loaded, and the roles come
    /// from the session, so an export can never widen what may be written.
    pub fn import_from(&mut self, directory: &Path) -> CoreResult<Vec<String>> {
        let mut loaded = Vec::new();
        for i in 0..self.files.len() {
            let relative = self.files[i].relative_path.clone();
            let path = directory.join(&relative);
            if !path.exists() {
                continue;
            }
            let role = self.files[i].role;
            let parsed = parser::read_file(&path, &relative, role)?;
            self.files[i] = parsed;
            self.files[i].dirty = true;
            loaded.push(relative);
        }
        self.undo_stack.clear();
        self.redo_stack.clear();
        Ok(loaded)
    }

    // ---------------------------------------------------------------------------------------
    // Backup and write
    // ---------------------------------------------------------------------------------------

    /// Back up the original content of the given profiles' files.
    ///
    /// This reads from the *v-Drive*, not from the draft, so the backup is always the original.
    pub fn backup(&mut self, profiles: &[ProfileId]) -> CoreResult<BackupSet> {
        let mut paths: Vec<String> = Vec::new();
        for p in profiles {
            if let Some(f) = self.device.layout_file(*p) {
                paths.push(f.relative_path.clone());
            }
            if let Some(f) = self
                .device
                .lighting_files()
                .into_iter()
                .find(|f| f.profile == Some(*p))
            {
                paths.push(f.relative_path.clone());
            }
        }
        if paths.is_empty() {
            // Back up settings.txt too, so the operator always has a full picture of the drive.
            if let Some(s) = self.device.settings_file() {
                paths.push(s.relative_path.clone());
            }
        }
        paths.sort();
        paths.dedup();
        let scope = if profiles.len() == 1 {
            format!("profile{}", profiles[0].0)
        } else {
            "profiles".to_string()
        };
        let set = backup::create(&self.device.root, &paths, &self.backup_root, &scope)?;
        backup::verify(&set)?;
        self.last_backup = Some(set.clone());
        Ok(set)
    }

    /// Write one profile's layout file to the v-Drive, atomically and verified.
    ///
    /// Refuses unless every one of these holds:
    /// * the profile is programmable (not profile 0);
    /// * the target file is a `layout<N>.txt` (the only kind this tool writes);
    /// * a dated, verified backup exists for this session;
    /// * validation reports no errors;
    /// * the operator passed `confirmed_by_operator: true`, having seen the change count.
    pub fn write_profile(&mut self, confirmation: WriteConfirmation) -> CoreResult<WriteReport> {
        let profile = confirmation.profile;
        if !profile.is_editable() {
            return Err(CoreError::ProfileZeroNotEditable);
        }
        if !confirmation.confirmed_by_operator {
            return Err(CoreError::RefusedWrite {
                reason: "no explicit confirmation was given; this tool never writes to the \
                         keyboard's v-Drive without one"
                    .to_string(),
            });
        }
        let file_index =
            self.profile_file_index(profile)
                .ok_or_else(|| CoreError::ReadOnlyFile {
                    path: PathBuf::from(profile.layout_file_name()),
                    role: "no layout file for this profile exists on the v-Drive",
                })?;
        let role = self.files[file_index].role;
        if !role.is_writable() {
            return Err(CoreError::ReadOnlyFile {
                path: PathBuf::from(&self.files[file_index].relative_path),
                role: role.label(),
            });
        }

        let absolute = self.device.root.join(&self.files[file_index].relative_path);
        if !absolute.exists() {
            return Err(CoreError::Disconnected {
                detail: format!(
                    "{} is gone; the keyboard may have been unplugged or the v-Drive closed \n                     after the session was opened",
                    absolute.display()
                ),
            });
        }

        let report = validate::validate_file(&self.files[file_index]);
        if report.blocks_write() {
            let first = report
                .errors()
                .next()
                .map(|f| {
                    format!(
                        "line {}{}",
                        f.line.map(|l| l.to_string()).unwrap_or_else(|| "-".into()),
                        f.hint
                            .as_ref()
                            .map(|h| format!(" ({h})"))
                            .unwrap_or_default()
                    )
                })
                .unwrap_or_else(|| "no detail available".to_string());
            return Err(CoreError::ValidationFailed {
                errors: report.error_count(),
                first,
            });
        }

        let backup_set = self
            .last_backup
            .clone()
            .ok_or_else(|| CoreError::BackupRequired {
                detail: "no backup has been taken in this session; run the backup step first"
                    .to_string(),
            })?;
        backup::verify(&backup_set)?;

        let plan = writer::plan(
            &self.files[file_index],
            &self.device.root.join(&self.files[file_index].relative_path),
        )?;

        if plan.changed_lines.len() != confirmation.acknowledged_changed_lines && !plan.is_noop() {
            return Err(CoreError::RefusedWrite {
                reason: format!(
                    "the operator acknowledged {} changed line(s) but the file now has {}. Re-open \
                     the change list and confirm again.",
                    confirmation.acknowledged_changed_lines,
                    plan.changed_lines.len()
                ),
            });
        }
        if plan.is_noop() {
            return Err(CoreError::RefusedWrite {
                reason: "the draft is identical to what is already on the v-Drive".to_string(),
            });
        }

        let outcome = writer::write_verified(&plan.absolute_path, &plan.after)?;

        // Accept the new content as the new baseline, so the diff goes back to empty and a second
        // write cannot silently repeat.
        self.original[file_index] = self.files[file_index].clone();
        self.files[file_index].dirty = false;

        // Confirm the drive is still readable after the write; if it vanished, say so loudly.
        if !self.device.root.exists() {
            return Err(CoreError::Disconnected {
                detail: format!(
                    "{} disappeared right after the write; the backup at {} still has the \
                     original content",
                    self.device.root.display(),
                    backup_set.directory.display()
                ),
            });
        }

        let mut next_steps = vec![
            format!(
                "Verified written file: {} ({} bytes)",
                outcome.relative_path, outcome.bytes_written
            ),
            "The keyboard re-reads its configuration when the v-Drive is closed.".to_string(),
            "Mode changes made from this tool are not visible to the keyboard until then."
                .to_string(),
        ];
        if self.files[file_index].has_mixed_eol() {
            next_steps.push(
                "Note: this file mixes LF and CRLF endings; each line kept the ending it had."
                    .to_string(),
            );
        }

        Ok(WriteReport {
            profile,
            relative_path: self.files[file_index].relative_path.clone(),
            bytes_written: outcome.bytes_written,
            verified: outcome.verified,
            changed_lines: plan.changed_lines,
            backup_directory: backup_set.directory.clone(),
            backup_files: backup_set
                .files
                .iter()
                .map(|f| f.relative_path.clone())
                .collect(),
            eject_instructions: device::eject_instructions(&self.device.root),
            next_steps,
        })
    }

    /// Restore the given relative paths from a backup set.
    pub fn restore(&self, set: &BackupSet, relative_paths: &[String]) -> CoreResult<Vec<String>> {
        backup::restore(set, &self.device.root, relative_paths)
    }
}

fn layer_of(line: &Line) -> LayerId {
    match &line.kind {
        LineKind::LayerHeader(l) => *l,
        _ => line.layer.unwrap_or(LayerId::Base),
    }
}

/// Per-profile status, for both interfaces' profile selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub profile: ProfileId,
    /// Profile 0 is never editable.
    pub editable: bool,
    pub has_layout_file: bool,
    pub has_lighting_file: bool,
    pub relative_path: Option<String>,
    pub pending_changed_lines: usize,
    pub validation_errors: usize,
    pub led_color_hint: String,
}

/// Per-file status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSummary {
    pub relative_path: String,
    pub role: FileRole,
    pub profile: Option<ProfileId>,
    pub lines: usize,
    pub bytes: usize,
}

/// A whole-session summary, used by the CLI and by the "about this session" panels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub device_root: PathBuf,
    pub evidence: Vec<String>,
    pub files: Vec<FileSummary>,
    pub profiles: Vec<ProfileSummary>,
    pub changed_lines: usize,
    pub key_changes: usize,
    pub errors: usize,
    pub warnings: usize,
    pub layout: KeyboardLayout,
    pub undo_depth: usize,
    pub redo_depth: usize,
    pub skipped_files: Vec<(String, String)>,
}

/// Severity helpers used by both interfaces when grouping findings.
pub fn worst_severity(findings: &[Finding]) -> Option<Severity> {
    findings.iter().map(|f| f.severity).min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets;
    use std::fs;

    fn fixture_drive() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ADV360");
        fs::create_dir_all(root.join("layouts")).unwrap();
        fs::create_dir_all(root.join("lighting")).unwrap();
        fs::write(root.join("settings.txt"), "never touch\n").unwrap();
        let headers: String = LayerId::all()
            .iter()
            .map(|l| format!("{}\n", l.header()))
            .collect();
        for p in ProfileId::programmable() {
            fs::write(
                root.join(format!("layouts/layout{}.txt", p.0)),
                headers.clone(),
            )
            .unwrap();
        }
        fs::write(
            root.join("lighting/led1.txt"),
            "[IND1]>[caps][255][000][000]\n",
        )
        .unwrap();
        dir
    }

    fn open(dir: &tempfile::TempDir) -> Session {
        Session::open_at(
            &dir.path().join("ADV360"),
            SessionOptions {
                backup_root: dir.path().join("backups"),
                layout_override: Some(("us".to_string(), None)),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn opening_a_drive_reads_every_profile() {
        let dir = fixture_drive();
        let s = open(&dir);
        assert_eq!(s.available_profiles().len(), 9);
        assert!(s.skipped_files().is_empty());
        assert_eq!(s.profile_summary().len(), 10);
        assert!(
            !s.profile_summary()[0].editable,
            "profile 0 must not be editable"
        );
        for p in s.profile_summary().iter().skip(1) {
            assert!(p.editable);
            assert!(p.has_layout_file);
        }
    }

    #[test]
    fn a_key_starts_at_its_documented_factory_action() {
        let dir = fixture_drive();
        let s = open(&dir);
        let ks = s.key_state(ProfileId(1), LayerId::Base, "esc").unwrap();
        assert!(!ks.is_custom());
        assert_eq!(ks.default.action, "esc");
        assert!(ks.default.is_documented());
        assert!(!ks.is_default_uncertain());

        let hk = s.key_state(ProfileId(1), LayerId::Base, "hk1").unwrap();
        assert_eq!(hk.default.action, "null");
        assert!(hk.effective_description().contains("blank by default"));

        let fn_key = s.key_state(ProfileId(1), LayerId::Base, "lfn").unwrap();
        assert_eq!(fn_key.default.action, "fn1s");
    }

    #[test]
    fn a_foot_pedal_default_is_reported_as_uncertain_not_guessed() {
        let dir = fixture_drive();
        let s = open(&dir);
        let ks = s.key_state(ProfileId(1), LayerId::Base, "lpd1").unwrap();
        assert!(ks.is_default_uncertain());
        assert!(ks.effective_description().contains("unknown"));
    }

    #[test]
    fn setting_a_binding_creates_a_line_and_is_undoable() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(1),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();

        let ks = s.key_state(ProfileId(1), LayerId::Base, "esc").unwrap();
        assert!(ks.is_custom());
        assert_eq!(ks.lines.len(), 1);
        let Binding::Remap(r) = ks.binding.unwrap() else {
            panic!()
        };
        assert_eq!(r.render(), "[esc]>[caps]");
        let report = s.validation();
        assert_eq!(
            report.error_count(),
            0,
            "unexpected errors: {:#?}",
            report.errors().collect::<Vec<_>>()
        );

        assert!(s.can_undo());
        s.undo().unwrap();
        let ks = s.key_state(ProfileId(1), LayerId::Base, "esc").unwrap();
        assert!(!ks.is_custom());
        assert!(!s.can_undo());
        assert!(s.can_redo());
        s.redo().unwrap();
        assert!(s
            .key_state(ProfileId(1), LayerId::Base, "esc")
            .unwrap()
            .is_custom());
    }

    #[test]
    fn the_edited_line_goes_into_the_right_layer_block() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(2),
            LayerId::Keypad,
            "spc",
            Some(Assignment::Remap {
                position: "spc".into(),
                action: "kp0".into(),
            }),
        )
        .unwrap();
        let f = s.file(s.profile_file_index(ProfileId(2)).unwrap()).unwrap();
        let text = f.render();
        let keypad_pos = text.find("<keypad>").unwrap();
        let f1_pos = text.find("<function1>").unwrap();
        let line_pos = text.find("[spc]>[kp0]").unwrap();
        assert!(keypad_pos < line_pos && line_pos < f1_pos, "{text}");
    }

    #[test]
    fn a_second_edit_replaces_the_same_line_instead_of_appending() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let set = |s: &mut Session, action: &str| {
            s.set_binding(
                ProfileId(1),
                LayerId::Base,
                "esc",
                Some(Assignment::Remap {
                    position: "esc".into(),
                    action: action.into(),
                }),
            )
            .unwrap()
        };
        set(&mut s, "caps");
        set(&mut s, "q");
        let ks = s.key_state(ProfileId(1), LayerId::Base, "esc").unwrap();
        assert_eq!(ks.lines.len(), 1, "a re-edit must not add a duplicate line");
        let text = s
            .file(s.profile_file_index(ProfileId(1)).unwrap())
            .unwrap()
            .render();
        assert!(text.contains("[esc]>[q]"));
        assert!(!text.contains("[esc]>[caps]"));
    }

    #[test]
    fn removing_a_remap_switches_the_line_off_instead_of_deleting_it() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(1),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();
        s.set_binding(ProfileId(1), LayerId::Base, "esc", None)
            .unwrap();

        let ks = s.key_state(ProfileId(1), LayerId::Base, "esc").unwrap();
        assert!(!ks.is_custom(), "the factory action must be back in effect");
        assert!(ks.has_disabled_lines);
        let text = s
            .file(s.profile_file_index(ProfileId(1)).unwrap())
            .unwrap()
            .render();
        assert!(
            text.contains("*[esc]>[caps]"),
            "the original text must survive: {text}"
        );

        // And removing again is a clear error rather than a silent no-op.
        let err = s
            .set_binding(ProfileId(1), LayerId::Base, "esc", None)
            .unwrap_err();
        assert!(err.to_string().contains("nothing to remove"));
    }

    #[test]
    fn a_different_binding_kind_disables_the_previous_line_rather_than_shadowing_it() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        // First a plain remap making `a` produce `b`.
        s.set_binding(
            ProfileId(1),
            LayerId::Base,
            "a",
            Some(Assignment::Remap {
                position: "a".into(),
                action: "b".into(),
            }),
        )
        .unwrap();
        // Now a macro whose trigger must be the action `a` produces in this layer, i.e. `a`.
        s.set_binding(
            ProfileId(1),
            LayerId::Base,
            "a",
            Some(Assignment::Macro {
                position: "a".into(),
                macro_line: MacroLine {
                    trigger: "a".into(),
                    modifiers: vec![],
                    body: vec![crate::model::MacroAtom::Action {
                        stroke: crate::model::Stroke::Press,
                        token: "z".into(),
                    }],
                },
            }),
        )
        .unwrap();
        let ks = s.key_state(ProfileId(1), LayerId::Base, "a").unwrap();
        assert!(matches!(ks.binding, Some(Binding::Macro(_))));
        // The single existing line for this key is rewritten in place, so the layer does not
        // accumulate a shadowed duplicate.
        assert_eq!(ks.lines.len(), 1, "{:?}", ks.lines);
        let text = s
            .file(s.profile_file_index(ProfileId(1)).unwrap())
            .unwrap()
            .render();
        assert!(text.contains("{a}>{z}"), "{text}");
        assert!(!text.contains("[a]>[b]"), "{text}");
    }

    #[test]
    fn profile_zero_can_never_be_edited() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let err = s
            .set_binding(
                ProfileId::ZERO,
                LayerId::Base,
                "esc",
                Some(Assignment::Remap {
                    position: "esc".into(),
                    action: "caps".into(),
                }),
            )
            .unwrap_err();
        assert_eq!(err.code(), "profile_zero_not_editable");
    }

    #[test]
    fn the_smartset_key_can_never_be_edited() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let err = s
            .set_binding(
                ProfileId(1),
                LayerId::Base,
                "ss",
                Some(Assignment::Remap {
                    position: "ss".into(),
                    action: "q".into(),
                }),
            )
            .unwrap_err();
        assert!(err.to_string().contains("SmartSet"));
    }

    #[test]
    fn an_unknown_position_is_rejected() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let err = s
            .set_binding(
                ProfileId(1),
                LayerId::Base,
                "nope",
                Some(Assignment::Remap {
                    position: "nope".into(),
                    action: "q".into(),
                }),
            )
            .unwrap_err();
        assert_eq!(err.code(), "unknown_position_token");
    }

    #[test]
    fn applying_the_symbol_and_navigation_presets_validates_cleanly() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let draft = presets::build_draft();
        let n = s.apply_preset(ProfileId(3), &draft.symbols).unwrap();
        assert_eq!(n, draft.symbols.len());
        s.apply_preset(ProfileId(3), &draft.navigation).unwrap();
        s.apply_access_key(ProfileId(3), &draft.access_key_options[0])
            .unwrap();

        let report = s.validation_for_write(ProfileId(3));
        assert_eq!(
            report.error_count(),
            0,
            "{:#?}",
            report.errors().collect::<Vec<_>>()
        );

        // And the changes are visible as a semantic diff.
        let diff = s.diff();
        assert!(diff.key_changes.len() >= draft.symbols.len() + draft.navigation.len());
        assert!(diff.changed_line_count() >= draft.symbols.len());
        // The Fn1 number row is untouched, so F1-F12 survive.
        let text = s
            .file(s.profile_file_index(ProfileId(3)).unwrap())
            .unwrap()
            .render();
        assert!(!text.contains("[eql]>"), "{text}");
        assert!(!text.contains("[hyph]>"), "{text}");
    }

    #[test]
    fn undo_and_redo_walk_the_whole_preset_application() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let draft = presets::build_draft();
        let depth = draft.symbols.len();
        s.apply_preset(ProfileId(4), &draft.symbols).unwrap();
        assert_eq!(s.undo_descriptions().len(), depth);
        for _ in 0..depth {
            s.undo();
        }
        assert!(!s.can_undo());
        assert!(s
            .key_state(ProfileId(4), LayerId::Function1, "a")
            .unwrap()
            .binding
            .is_none());
        for _ in 0..depth {
            s.redo();
        }
        assert!(s
            .key_state(ProfileId(4), LayerId::Function1, "a")
            .unwrap()
            .binding
            .is_some());
    }

    #[test]
    fn a_write_is_refused_without_confirmation_a_backup_and_clean_validation() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(1),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();
        let changed = s.file_change_count(s.profile_file_index(ProfileId(1)).unwrap());

        // 1. no confirmation
        let err = s
            .write_profile(WriteConfirmation {
                profile: ProfileId(1),
                confirmed_by_operator: false,
                acknowledged_changed_lines: changed,
            })
            .unwrap_err();
        assert!(err.to_string().contains("never writes"));

        // 2. confirmed but no backup
        let err = s
            .write_profile(WriteConfirmation {
                profile: ProfileId(1),
                confirmed_by_operator: true,
                acknowledged_changed_lines: changed,
            })
            .unwrap_err();
        assert_eq!(err.code(), "backup_required");

        // 3. backup, but the change count does not match what was acknowledged
        s.backup(&[ProfileId(1)]).unwrap();
        let err = s
            .write_profile(WriteConfirmation {
                profile: ProfileId(1),
                confirmed_by_operator: true,
                acknowledged_changed_lines: changed + 5,
            })
            .unwrap_err();
        assert!(err.to_string().contains("acknowledged"));

        // 4. with everything in place it succeeds and verifies.
        let report = s
            .write_profile(WriteConfirmation {
                profile: ProfileId(1),
                confirmed_by_operator: true,
                acknowledged_changed_lines: changed,
            })
            .unwrap();
        assert!(report.verified);
        assert_eq!(report.relative_path, "layouts/layout1.txt");
        assert!(report.backup_directory.exists());
        assert!(report
            .eject_instructions
            .iter()
            .any(|i| i.contains("udisksctl")));
        // The v-Drive really has the new line.
        let on_disk = fs::read_to_string(dir.path().join("ADV360/layouts/layout1.txt")).unwrap();
        assert!(on_disk.contains("[esc]>[caps]"), "{on_disk}");
        // Nothing else changed: settings.txt is untouched and the other profiles are untouched.
        assert_eq!(
            fs::read_to_string(dir.path().join("ADV360/settings.txt")).unwrap(),
            "never touch\n"
        );
        assert!(
            !fs::read_to_string(dir.path().join("ADV360/layouts/layout2.txt"))
                .unwrap()
                .contains("[esc]")
        );
        // After writing, the baseline is accepted so the diff is empty again.
        assert!(s.diff().key_changes.is_empty());
    }

    #[test]
    fn writing_is_refused_while_validation_reports_an_error() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let idx = s.profile_file_index(ProfileId(1)).unwrap();
        s.append_line_raw(idx, LayerId::Base, "[esc]>[".to_string())
            .unwrap();
        s.backup(&[ProfileId(1)]).unwrap();
        let err = s
            .write_profile(WriteConfirmation {
                profile: ProfileId(1),
                confirmed_by_operator: true,
                acknowledged_changed_lines: 1,
            })
            .unwrap_err();
        assert_eq!(err.code(), "validation_failed");
        assert!(!err.recovery_hints().is_empty());
        // The v-Drive is untouched.
        let on_disk = fs::read_to_string(dir.path().join("ADV360/layouts/layout1.txt")).unwrap();
        assert!(!on_disk.contains("[esc]"));
    }

    #[test]
    fn writing_an_unchanged_draft_is_refused() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.backup(&[ProfileId(1)]).unwrap();
        let err = s
            .write_profile(WriteConfirmation {
                profile: ProfileId(1),
                confirmed_by_operator: true,
                acknowledged_changed_lines: 0,
            })
            .unwrap_err();
        assert!(err.to_string().contains("identical"));
    }

    #[test]
    fn settings_txt_is_never_writable_through_the_session() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let idx = s
            .draft_files()
            .iter()
            .position(|f| f.role == FileRole::Settings)
            .unwrap();
        s.set_line_raw(idx, 0, "hacked=1".to_string()).unwrap();
        assert!(s.draft_files()[idx].render().contains("hacked=1"));
        // It still cannot be written: no profile maps to it.
        let err = s
            .write_profile(WriteConfirmation {
                profile: ProfileId(1),
                confirmed_by_operator: true,
                acknowledged_changed_lines: 0,
            })
            .unwrap_err();
        assert!(err.code() == "refused_write" || err.code() == "backup_required");
        assert_eq!(
            fs::read_to_string(dir.path().join("ADV360/settings.txt")).unwrap(),
            "never touch\n"
        );
    }

    #[test]
    fn a_simulated_disconnect_during_the_write_keeps_the_backup_and_reports_recovery() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(1),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();
        let changed = s.file_change_count(s.profile_file_index(ProfileId(1)).unwrap());
        let backup = s.backup(&[ProfileId(1)]).unwrap();
        let original_on_disk =
            fs::read_to_string(dir.path().join("ADV360/layouts/layout1.txt")).unwrap();

        // Simulate the drive going away: remove the whole v-Drive directory.
        fs::remove_dir_all(dir.path().join("ADV360")).unwrap();

        let err = s
            .write_profile(WriteConfirmation {
                profile: ProfileId(1),
                confirmed_by_operator: true,
                acknowledged_changed_lines: changed,
            })
            .unwrap_err();
        assert!(
            matches!(
                err,
                CoreError::Write { .. } | CoreError::Disconnected { .. }
            ),
            "{err:?}"
        );
        // The backup is intact and holds the original bytes.
        backup::verify(&backup).unwrap();
        let saved = fs::read_to_string(backup.directory.join("layouts/layout1.txt")).unwrap();
        assert_eq!(saved, original_on_disk);
        // And recovery hints are concrete.
        assert!(err.recovery_hints().iter().any(|h| h.contains("backup")));
    }

    #[test]
    fn a_backup_can_restore_the_drive_after_a_bad_write() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let before = fs::read_to_string(dir.path().join("ADV360/layouts/layout1.txt")).unwrap();
        let set = s.backup(&[ProfileId(1)]).unwrap();
        fs::write(dir.path().join("ADV360/layouts/layout1.txt"), "corrupted").unwrap();
        let restored = s
            .restore(&set, &["layouts/layout1.txt".to_string()])
            .unwrap();
        assert_eq!(restored, vec!["layouts/layout1.txt"]);
        let after = fs::read_to_string(dir.path().join("ADV360/layouts/layout1.txt")).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn export_and_import_round_trip_the_draft_without_touching_the_drive() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(5),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();
        let export_dir = dir.path().join("export");
        let written = s.export_to(&export_dir).unwrap();
        assert!(written.iter().any(|p| p.ends_with("adv360-draft.json")));
        assert!(export_dir.join("layouts/layout5.txt").exists());
        // The v-Drive itself is untouched.
        assert!(
            !fs::read_to_string(dir.path().join("ADV360/layouts/layout5.txt"))
                .unwrap()
                .contains("[esc]")
        );

        // Import it back into a fresh session and confirm the draft matches.
        let mut s2 = open(&dir);
        let loaded = s2.import_from(&export_dir).unwrap();
        assert!(loaded.iter().any(|p| p == "layouts/layout5.txt"));
        assert!(s2
            .key_state(ProfileId(5), LayerId::Base, "esc")
            .unwrap()
            .is_custom());
    }

    #[test]
    fn import_never_widens_which_files_may_be_written() {
        // A stray file in the export directory must not become part of the session.
        let dir = fixture_drive();
        let mut s = open(&dir);
        let export_dir = dir.path().join("export");
        s.export_to(&export_dir).unwrap();
        fs::write(export_dir.join("layouts/layout0.txt"), "[esc]>[q]\n").unwrap();
        fs::write(export_dir.join("settings.txt"), "evil\n").unwrap();
        s.import_from(&export_dir).unwrap();
        assert!(s.profile_file_index(ProfileId::ZERO).is_none());
        assert_eq!(
            fs::read_to_string(dir.path().join("ADV360/settings.txt")).unwrap(),
            "never touch\n"
        );
    }

    #[test]
    fn the_pending_change_list_describes_each_edit() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(1),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();
        let pending = s.pending_changes();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].description.contains("esc"));
        assert!(pending[0].description.contains("caps"));
        assert!(!pending[0].file.is_empty());
    }

    #[test]
    fn validation_findings_are_attached_to_the_key_state() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let idx = s.profile_file_index(ProfileId(1)).unwrap();
        s.append_line_raw(idx, LayerId::Base, "[q]>[w][e]".to_string())
            .unwrap();
        let ks = s.key_state(ProfileId(1), LayerId::Base, "q").unwrap();
        assert!(ks.findings.iter().any(|f| f.code == "multi-action-remap"));
        assert_eq!(worst_severity(&ks.findings), Some(Severity::Warning));
    }

    #[test]
    fn the_session_summary_reflects_the_draft() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(1),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();
        let summary = s.summary();
        assert_eq!(summary.profiles.len(), 10);
        assert!(summary.key_changes >= 1);
        assert!(summary.changed_lines >= 1);
        assert_eq!(summary.undo_depth, 1);
        assert!(summary
            .files
            .iter()
            .any(|f| f.relative_path == "settings.txt"));
        assert_eq!(summary.layout.layout, "us");
    }

    #[test]
    fn a_crlf_file_keeps_its_endings_when_lines_are_appended() {
        let dir = fixture_drive();
        let path = dir.path().join("ADV360/layouts/layout6.txt");
        fs::write(&path, "<base>\r\n<keypad>\r\n").unwrap();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(6),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();
        let text = s
            .file(s.profile_file_index(ProfileId(6)).unwrap())
            .unwrap()
            .render();
        assert!(text.contains("<base>\r\n[esc]>[caps]\r\n"), "{text:?}");
        assert!(!s
            .file(s.profile_file_index(ProfileId(6)).unwrap())
            .unwrap()
            .has_mixed_eol());
    }

    #[test]
    fn a_line_header_cannot_be_edited_as_a_plain_line() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let idx = s.profile_file_index(ProfileId(1)).unwrap();
        let err = s
            .set_line_raw(idx, 0, "<nonsense>".to_string())
            .unwrap_err();
        assert!(err.to_string().contains("structural"));
    }

    #[test]
    fn unknown_lines_in_a_real_file_survive_editing_and_writing() {
        let dir = fixture_drive();
        let path = dir.path().join("ADV360/layouts/layout7.txt");
        fs::write(
            &path,
            "# my own note\r\n<base>\r\nVENDOREXT something\r\n<keypad>\r\n<function1>\r\n<function2>\r\n<function3>\r\n",
        )
        .unwrap();
        let mut s = open(&dir);
        s.set_binding(
            ProfileId(7),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();
        let rendered = s
            .file(s.profile_file_index(ProfileId(7)).unwrap())
            .unwrap()
            .render();
        assert!(rendered.contains("# my own note"));
        assert!(rendered.contains("VENDOREXT something"));
        assert!(rendered.contains("[esc]>[caps]"));
        assert!(rendered.contains("\r\n"), "CRLF must be preserved");

        let changed = s.file_change_count(s.profile_file_index(ProfileId(7)).unwrap());
        s.backup(&[ProfileId(7)]).unwrap();
        s.write_profile(WriteConfirmation {
            profile: ProfileId(7),
            confirmed_by_operator: true,
            acknowledged_changed_lines: changed,
        })
        .unwrap();
        let on_disk = fs::read_to_string(&path).unwrap();
        assert_eq!(on_disk, rendered);
        assert!(on_disk.contains("VENDOREXT something"));
    }

    #[test]
    fn led_lines_can_be_set_and_are_written_only_to_the_lighting_file() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        s.set_led(
            ProfileId(1),
            Indicator(1),
            LedFunction::CapsLock,
            vec![Rgb { r: 0, g: 255, b: 0 }],
        )
        .unwrap();
        let text = fs::read_to_string(dir.path().join("ADV360/lighting/led1.txt")).unwrap();
        // Not written yet: the draft only.
        assert!(text.contains("[255][000][000]"), "{text}");
        let rendered = s.render_all();
        assert!(rendered["lighting/led1.txt"].contains("[IND1]>[caps][000][255][000]"));
    }

    #[test]
    fn a_layout_draft_can_be_pasted_into_an_empty_profile() {
        let dir = fixture_drive();
        let draft = presets::build_draft();
        let text = presets::render_layout_draft(&draft, Some(&draft.access_key_options[0]));
        let path = dir.path().join("ADV360/layouts/layout9.txt");
        fs::write(&path, &text).unwrap();
        let s2 = open(&dir);
        let idx = s2.profile_file_index(ProfileId(9)).unwrap();
        assert!(!s2.file(idx).unwrap().lines.is_empty());
        assert_eq!(
            validate::validate_file(s2.file(idx).unwrap()).error_count(),
            0
        );
        let ks = s2.key_state(ProfileId(9), LayerId::Function1, "a").unwrap();
        assert!(ks.is_custom());
    }

    #[test]
    fn a_line_edit_cannot_introduce_a_layer_header() {
        let dir = fixture_drive();
        let mut s = open(&dir);
        let idx = s.profile_file_index(ProfileId(1)).unwrap();
        let n = s
            .append_line_raw(idx, LayerId::Base, "[esc]>[caps]".to_string())
            .unwrap();
        let err = s
            .set_line_raw(idx, n, "<function3>".to_string())
            .unwrap_err();
        assert!(err.to_string().contains("structural"));
        let err = s
            .append_line_raw(idx, LayerId::Base, "<function3>".to_string())
            .unwrap_err();
        assert!(err.to_string().contains("structural"));
    }
}
