//! The pure state machine behind the terminal interface.
//!
//! [`App`] owns the [`Session`] and every piece of interface state. It never touches a terminal
//! itself, so it can be driven by [`crate::run_headless`] in tests with a scripted [`Msg`] list.
//! All configuration work goes through the core session: this module does not parse, validate,
//! back up or write anything on its own.

use std::path::PathBuf;

use adv360_core::prelude::*;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Which panel the operator is looking at. `Tab`/`Shift-Tab` cycles through all of them, so every
/// required function is reachable from the keyboard even on a narrow terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Map,
    Catalogue,
    Changes,
    Validation,
    Diff,
    Presets,
    Layout,
    Write,
    Profiles,
    Help,
}

impl Panel {
    /// Every panel, in the order `Tab` visits them.
    pub fn all() -> &'static [Panel] {
        &[
            Panel::Map,
            Panel::Catalogue,
            Panel::Changes,
            Panel::Validation,
            Panel::Diff,
            Panel::Presets,
            Panel::Layout,
            Panel::Write,
            Panel::Profiles,
            Panel::Help,
        ]
    }

    /// A short title shown in the panel border.
    pub fn title(self) -> &'static str {
        match self {
            Panel::Map => "Physical map",
            Panel::Catalogue => "Action catalogue",
            Panel::Changes => "Pending changes",
            Panel::Validation => "Validation",
            Panel::Diff => "Differences",
            Panel::Presets => "Preset drafts (review only)",
            Panel::Layout => "Keyboard layout",
            Panel::Write => "Write confirmation",
            Panel::Profiles => "Profiles",
            Panel::Help => "Help and sources",
        }
    }

    /// A stable position in [`Panel::all`], used by `Tab`.
    fn index(self) -> usize {
        Panel::all().iter().position(|p| *p == self).unwrap_or(0)
    }

    /// The next panel, wrapping.
    fn next(self) -> Panel {
        let all = Panel::all();
        all[(self.index() + 1) % all.len()]
    }

    /// The previous panel, wrapping.
    fn prev(self) -> Panel {
        let all = Panel::all();
        all[(self.index() + all.len() - 1) % all.len()]
    }
}

/// One message the interface can process.
///
/// `Msg::Key` carries a real Crossterm key press, so the keyboard path exercised by `run` is the
/// same one `run_headless` scripts.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A key press.
    Key(KeyEvent),
    /// The terminal was resized.
    Resize(u16, u16),
    /// Time passed with no input; used to keep the interface responsive.
    Tick,
    /// Ask the event loop to stop.
    Quit,
}

impl Msg {
    /// A plain key press with no modifiers.
    pub fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// A printable character.
    pub fn text(c: char) -> Msg {
        Msg::key(KeyCode::Char(c))
    }

    /// A control chord, e.g. `Ctrl-r`.
    pub fn ctrl(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }
}

/// What the interface is collecting text for. `Normal` is the navigation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Normal,
    Search,
    LayoutName,
    WriteConfirm,
    ExportPath { import: bool },
}

/// What a catalogue selection is turned into when `Enter`, `t` or `m` is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssignKind {
    Remap,
    TapAndHold,
    Macro,
}

/// The whole terminal interface state.
///
/// Every field is plain data plus the shared [`Session`], which is the only thing that reads or
/// writes configuration.
pub struct App {
    pub(crate) session: Session,
    pub(crate) profile: ProfileId,
    pub(crate) layer: LayerId,
    pub(crate) selected_position: String,
    pub(crate) panel: Panel,
    pub(crate) search: String,
    pub(crate) catalogue_index: usize,
    pub(crate) preset_index: usize,
    pub(crate) profile_cursor: u8,
    pub(crate) draft: presets::Draft,
    pub(crate) write_input: String,
    pub(crate) write_report: Option<WriteReport>,
    pub(crate) path_input: String,
    pub(crate) layout_input: String,
    pub(crate) mode: Mode,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
}

impl App {
    /// Build an app around an already opened session.
    pub fn new(session: Session) -> App {
        let profile = session
            .available_profiles()
            .first()
            .copied()
            .unwrap_or(ProfileId(1));
        let selected_position = positions::programmable()
            .next()
            .map_or_else(String::new, |p| p.token.to_string());
        App {
            session,
            profile,
            layer: LayerId::Base,
            selected_position,
            panel: Panel::Map,
            search: String::new(),
            catalogue_index: 0,
            preset_index: 0,
            profile_cursor: profile.0,
            draft: presets::build_draft(),
            write_input: String::new(),
            write_report: None,
            path_input: String::new(),
            layout_input: String::new(),
            mode: Mode::Normal,
            status: "Ready. Press ? for help, / to search actions, w to write.".to_string(),
            error: None,
        }
    }

    /// The profile currently selected for editing (never profile 0).
    pub fn profile(&self) -> ProfileId {
        self.profile
    }

    /// The layer currently selected.
    pub fn layer(&self) -> LayerId {
        self.layer
    }

    /// The position token the key cursor is on.
    pub fn selected_position(&self) -> &str {
        &self.selected_position
    }

    /// The currently open panel.
    pub fn panel(&self) -> Panel {
        self.panel
    }

    /// Handle one message. Returns `false` when the interface should stop.
    pub fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Quit => false,
            Msg::Resize(_, _) | Msg::Tick => true,
            Msg::Key(key) => self.handle_key(key),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return true;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => return false,
                KeyCode::Char('r') => {
                    self.redo();
                    return true;
                }
                _ => {}
            }
        }

        match self.mode {
            Mode::Search => return self.handle_search(key),
            Mode::LayoutName => return self.handle_layout_input(key),
            Mode::WriteConfirm => return self.handle_write_input(key),
            Mode::ExportPath { import } => return self.handle_path_input(key, import),
            Mode::Normal => {}
        }

        if key.code == KeyCode::Esc {
            if self.error.is_some() {
                self.error = None;
                self.status = "Error dismissed.".to_string();
            } else {
                self.panel = Panel::Map;
            }
            return true;
        }

        if self.handle_panel_keys(&key) {
            return true;
        }

        match key.code {
            KeyCode::Tab => {
                self.panel = self.panel.next();
                self.status = format!("{}: {}", self.panel.title(), self.panel_hint());
                true
            }
            KeyCode::BackTab => {
                self.panel = self.panel.prev();
                self.status = format!("{}: {}", self.panel.title(), self.panel_hint());
                true
            }
            _ => match printable(&key) {
                Some(c) => self.handle_global_char(c),
                None => true,
            },
        }
    }

    fn handle_panel_keys(&mut self, key: &KeyEvent) -> bool {
        match self.panel {
            Panel::Map => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.move_cursor(-1);
                    true
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.move_cursor(1);
                    true
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    self.move_cursor(-1);
                    true
                }
                KeyCode::Right => {
                    self.move_cursor(1);
                    true
                }
                KeyCode::Enter => {
                    self.panel = Panel::Catalogue;
                    self.status = "Catalogue: type / to search, Enter to assign.".to_string();
                    true
                }
                _ => false,
            },
            Panel::Catalogue => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.catalogue_move(-1);
                    true
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.catalogue_move(1);
                    true
                }
                KeyCode::Enter => {
                    self.assign_selected(AssignKind::Remap);
                    true
                }
                KeyCode::Char('t') => {
                    self.assign_selected(AssignKind::TapAndHold);
                    true
                }
                KeyCode::Char('m') => {
                    self.assign_selected(AssignKind::Macro);
                    true
                }
                _ => false,
            },
            Panel::Presets => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.preset_move(-1);
                    true
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.preset_move(1);
                    true
                }
                KeyCode::Enter | KeyCode::Char('a') => {
                    self.apply_preset_selected();
                    true
                }
                _ => false,
            },
            Panel::Profiles => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.profile_cursor = self.profile_cursor.saturating_sub(1);
                    true
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.profile_cursor = (self.profile_cursor + 1).min(9);
                    true
                }
                KeyCode::Enter => {
                    self.select_profile_cursor();
                    true
                }
                _ => false,
            },
            Panel::Write => match key.code {
                KeyCode::Char('b') => {
                    self.do_backup();
                    true
                }
                KeyCode::Enter => {
                    self.write_input.clear();
                    self.mode = Mode::WriteConfirm;
                    self.status = "Type the word WRITE exactly, then Enter, to confirm the write."
                        .to_string();
                    true
                }
                _ => false,
            },
            Panel::Layout => {
                if matches!(key.code, KeyCode::Enter | KeyCode::Char('c')) {
                    self.layout_input = self.session.layout().layout.clone();
                    self.mode = Mode::LayoutName;
                    self.status = "Type the XKB layout name, then Enter.".to_string();
                    return true;
                }
                false
            }
            Panel::Changes | Panel::Validation | Panel::Diff | Panel::Help => false,
        }
    }

    fn handle_global_char(&mut self, c: char) -> bool {
        match c {
            'q' => return false,
            '?' => {
                self.panel = Panel::Help;
                self.status = "Help. Press q to quit, Esc to return to the map.".to_string();
            }
            '/' => {
                self.panel = Panel::Catalogue;
                self.search.clear();
                self.catalogue_index = 0;
                self.mode = Mode::Search;
                self.status = "Catalogue search: type and press Enter.".to_string();
            }
            'p' => self.next_profile(false),
            'P' => self.next_profile(true),
            'l' => self.next_layer(false),
            'L' => self.next_layer(true),
            'u' => self.undo(),
            'd' => {
                self.panel = Panel::Diff;
                self.status = "Differences between the draft and the v-Drive.".to_string();
            }
            'v' => {
                self.panel = Panel::Validation;
                self.status = "Validation findings.".to_string();
            }
            'g' => {
                self.panel = Panel::Presets;
                self.status = "Preset drafts. Nothing is applied until you press a on one of them."
                    .to_string();
            }
            'y' => {
                self.panel = Panel::Layout;
                self.status = "Keyboard layout. Press c to correct it manually.".to_string();
            }
            'w' => self.begin_write(),
            'b' => self.do_backup(),
            'e' => self.begin_path(false),
            'i' => self.begin_path(true),
            _ => {}
        }
        true
    }

    fn handle_search(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = "Search kept. Enter assigns the highlighted action.".to_string();
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.status = "Enter assigns the highlighted action.".to_string();
            }
            KeyCode::Backspace => {
                self.search.pop();
                self.catalogue_index = 0;
            }
            _ => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u') {
                    self.search.clear();
                    self.catalogue_index = 0;
                } else if let Some(c) = printable(&key) {
                    self.search.push(c);
                    self.catalogue_index = 0;
                }
            }
        }
        true
    }

    fn handle_layout_input(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = "Layout unchanged.".to_string();
            }
            KeyCode::Enter => {
                let layout = self.layout_input.trim().to_string();
                if layout.is_empty() {
                    self.status = "Layout name is empty; nothing changed.".to_string();
                } else {
                    let variant = self.session.layout().variant.clone();
                    self.session.set_layout_override(layout, variant);
                    self.status =
                        format!("Keyboard layout set to {}", self.session.layout().layout);
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.layout_input.pop();
            }
            _ => {
                if let Some(c) = printable(&key) {
                    self.layout_input.push(c);
                }
            }
        }
        true
    }

    fn handle_write_input(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.write_input.clear();
                self.status = "Write cancelled; nothing was written.".to_string();
            }
            KeyCode::Enter => {
                self.confirm_write();
            }
            KeyCode::Backspace => {
                self.write_input.pop();
            }
            _ => {
                if let Some(c) = printable(&key) {
                    self.write_input.push(c);
                }
            }
        }
        true
    }

    fn handle_path_input(&mut self, key: KeyEvent, import: bool) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = "Path entry cancelled.".to_string();
            }
            KeyCode::Enter => {
                let path = PathBuf::from(self.path_input.trim());
                self.mode = Mode::Normal;
                self.run_path_action(&path, import);
            }
            KeyCode::Backspace => {
                self.path_input.pop();
            }
            _ => {
                if let Some(c) = printable(&key) {
                    self.path_input.push(c);
                }
            }
        }
        true
    }

    // -------------------------------------------------------------------------------------
    // Sessions actions
    // -------------------------------------------------------------------------------------

    fn next_profile(&mut self, backwards: bool) {
        let profiles: Vec<ProfileId> = ProfileId::programmable().collect();
        let idx = profiles
            .iter()
            .position(|p| *p == self.profile)
            .unwrap_or(0);
        let len = profiles.len();
        let next = if backwards {
            profiles[(idx + len - 1) % len]
        } else {
            profiles[(idx + 1) % len]
        };
        self.profile = next;
        self.profile_cursor = next.0;
        self.clamp_selection();
        self.status = format!("Selected {next} (editable).");
    }

    fn next_layer(&mut self, backwards: bool) {
        let layers = LayerId::all();
        let idx = layers.iter().position(|l| *l == self.layer).unwrap_or(0);
        let len = layers.len();
        self.layer = if backwards {
            layers[(idx + len - 1) % len]
        } else {
            layers[(idx + 1) % len]
        };
        self.status = format!(
            "Layer {} ({}).",
            self.layer.short_label(),
            self.layer.led_color_hint()
        );
    }

    fn select_profile_cursor(&mut self) {
        if self.profile_cursor == 0 {
            self.status =
                "Profile 0 is listed for reference only and cannot be edited or selected."
                    .to_string();
            return;
        }
        self.profile = ProfileId(self.profile_cursor);
        self.clamp_selection();
        self.status = format!("Selected {} (editable).", self.profile);
    }

    fn clamp_selection(&mut self) {
        let positions: Vec<&PositionDef> = positions::programmable().collect();
        let exists = positions
            .iter()
            .any(|p| p.token.eq_ignore_ascii_case(&self.selected_position));
        if exists {
            return;
        }
        if let Some(first) = positions.first() {
            self.selected_position = first.token.to_string();
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let positions: Vec<&PositionDef> = positions::programmable().collect();
        if positions.is_empty() {
            return;
        }
        let idx = positions
            .iter()
            .position(|p| p.token.eq_ignore_ascii_case(&self.selected_position))
            .unwrap_or(0);
        let new = (idx as isize + delta).clamp(0, positions.len() as isize - 1) as usize;
        self.selected_position = positions[new].token.to_string();
        self.status = format!("Key cursor: {}.", self.selected_position);
    }

    fn catalogue_move(&mut self, delta: isize) {
        let results = tokens::search(&self.search).len();
        if results == 0 {
            return;
        }
        let new = (self.catalogue_index as isize + delta).clamp(0, results as isize - 1) as usize;
        self.catalogue_index = new;
    }

    fn preset_move(&mut self, delta: isize) {
        let total = self.preset_total();
        if total == 0 {
            return;
        }
        let new = (self.preset_index as isize + delta).clamp(0, total as isize - 1) as usize;
        self.preset_index = new;
    }

    fn preset_total(&self) -> usize {
        self.draft.symbols.assignments.len()
            + self.draft.navigation.assignments.len()
            + self.draft.access_key_options.len()
    }

    fn assign_selected(&mut self, kind: AssignKind) {
        if self.search.trim().is_empty() {
            self.status = "Search the catalogue first (/), then choose an action.".to_string();
            return;
        }
        let results = tokens::search(&self.search);
        let Some(token) = results.get(self.catalogue_index).copied() else {
            self.status = "No action selected.".to_string();
            return;
        };
        let position = self.selected_position.clone();
        let assignment = match kind {
            AssignKind::Remap => Assignment::Remap {
                position: position.clone(),
                action: token.id.to_string(),
            },
            AssignKind::TapAndHold => Assignment::TapAndHold {
                position: position.clone(),
                tap: token.id.to_string(),
                delay_ms: 500,
                hold: "esc".to_string(),
            },
            AssignKind::Macro => Assignment::Macro {
                position: position.clone(),
                macro_line: MacroLine {
                    trigger: position.clone(),
                    modifiers: Vec::new(),
                    body: vec![MacroAtom::Action {
                        stroke: Stroke::Press,
                        token: token.id.to_string(),
                    }],
                },
            },
        };
        match self
            .session
            .set_binding(self.profile, self.layer, &position, Some(assignment))
        {
            Ok(description) => {
                self.status = description;
                self.error = None;
            }
            Err(err) => self.fail(err),
        }
    }

    fn apply_preset_selected(&mut self) {
        let symbols = self.draft.symbols.assignments.len();
        let navigation = self.draft.navigation.assignments.len();
        let index = self.preset_index;
        let result = if index < symbols {
            let preset = presets::programming_symbols_us();
            self.session
                .apply_preset(self.profile, &preset)
                .map(|n| format!("Applied {n} draft assignments from the symbols preset."))
        } else if index < symbols + navigation {
            let preset = presets::navigation_layer();
            self.session
                .apply_preset(self.profile, &preset)
                .map(|n| format!("Applied {n} draft assignments from the navigation preset."))
        } else {
            let options = presets::access_key_options(self.draft.navigation_layer);
            let choice = index - symbols - navigation;
            match options.get(choice) {
                Some(option) => self
                    .session
                    .apply_access_key(self.profile, option)
                    .map(|()| format!("Applied access key line {} as a draft.", option.line)),
                None => Ok("No proposal selected.".to_string()),
            }
        };
        match result {
            Ok(message) => {
                self.status = message;
                self.error = None;
            }
            Err(err) => self.fail(err),
        }
    }

    fn undo(&mut self) {
        match self.session.undo() {
            Some(description) => {
                self.status = format!("Undone: {description}");
                self.error = None;
            }
            None => self.status = "Nothing to undo.".to_string(),
        }
    }

    fn redo(&mut self) {
        match self.session.redo() {
            Some(description) => {
                self.status = format!("Redone: {description}");
                self.error = None;
            }
            None => self.status = "Nothing to redo.".to_string(),
        }
    }

    fn begin_write(&mut self) {
        self.panel = Panel::Write;
        self.mode = Mode::Normal;
        self.write_input.clear();
        self.write_report = None;
        self.status = "Write flow: review the target, then press Enter and type WRITE to confirm."
            .to_string();
    }

    fn confirm_write(&mut self) {
        let profile = self.profile;
        if self.write_input.trim() != "WRITE" {
            self.mode = Mode::Normal;
            self.write_input.clear();
            self.status =
                "Confirmation not accepted: type the word WRITE exactly, then Enter.".to_string();
            return;
        }
        let acknowledged = self.target_changed_lines().len();
        let confirmation = WriteConfirmation {
            profile,
            confirmed_by_operator: true,
            acknowledged_changed_lines: acknowledged,
        };
        match self.session.write_profile(confirmation) {
            Ok(report) => {
                self.status = format!(
                    "Write verified: {} ({} line(s) changed). Backup location: {} ({} file(s)).",
                    report.relative_path,
                    report.changed_lines.len(),
                    report.backup_directory.display(),
                    report.backup_files.len()
                );
                self.write_report = Some(report);
                self.error = None;
            }
            Err(err) => self.fail(err),
        }
        self.mode = Mode::Normal;
        self.write_input.clear();
    }

    fn do_backup(&mut self) {
        let profile = self.profile;
        match self.session.backup(&[profile]) {
            Ok(set) => {
                self.status = format!(
                    "Backup taken at {} ({} file(s)).",
                    set.directory.display(),
                    set.files.len()
                );
                self.error = None;
            }
            Err(err) => self.fail(err),
        }
    }

    fn begin_path(&mut self, import: bool) {
        self.path_input = default_exchange_dir().to_string_lossy().to_string();
        self.mode = Mode::ExportPath { import };
        self.status = if import {
            "Import path: type a directory, then Enter.".to_string()
        } else {
            "Export path: type a directory, then Enter.".to_string()
        };
    }

    fn run_path_action(&mut self, path: &std::path::Path, import: bool) {
        let result = if import {
            self.session.import_from(path).map(|loaded| {
                format!(
                    "Imported {} file(s) from {}; the draft was replaced.",
                    loaded.len(),
                    path.display()
                )
            })
        } else {
            self.session
                .export_to(path)
                .map(|written| format!("Exported {} file(s) to {}.", written.len(), path.display()))
        };
        match result {
            Ok(message) => {
                self.status = message;
                self.error = None;
            }
            Err(err) => self.fail(err),
        }
    }

    /// The 1-based changed line numbers of the profile's layout file, as shown before a write.
    pub(crate) fn target_changed_lines(&self) -> Vec<usize> {
        match self.session.profile_file_index(self.profile) {
            Some(index) => match self.session.file_diff(index) {
                Some(diff) => diff.changed_lines,
                None => Vec::new(),
            },
            None => Vec::new(),
        }
    }

    /// The target file that will be written, relative to the v-Drive.
    pub(crate) fn target_relative_path(&self) -> Option<String> {
        self.session
            .profile_file_index(self.profile)
            .and_then(|index| self.session.file(index))
            .map(|file| file.relative_path.clone())
    }

    /// A short sentence describing what the highlighted panel lets the operator do.
    pub(crate) fn panel_hint(&self) -> &'static str {
        match self.panel {
            Panel::Map => "arrows/hjkl move the cursor; Enter opens the catalogue",
            Panel::Catalogue => "type to search, Enter assigns a remap, t tap-and-hold, m macro",
            Panel::Changes => "everything that would change on the v-Drive",
            Panel::Validation => "errors block writing; warnings and info do not",
            Panel::Diff => "the semantic and textual differences",
            Panel::Presets => "review the table, then press a to apply one proposal",
            Panel::Layout => "the detected XKB layout and the token/character distinction",
            Panel::Write => "review, press b for a backup, then Enter and type WRITE",
            Panel::Profiles => "j/k moves, Enter selects; profile 0 is locked",
            Panel::Help => "key shortcuts, documentation sources and recovery hints",
        }
    }

    fn fail(&mut self, err: CoreError) {
        let mut text = format!("{}: {err}", err.code());
        for hint in err.recovery_hints() {
            text.push_str("\n - ");
            text.push_str(&hint);
        }
        self.error = Some(text);
        self.status = format!(
            "Last action failed ({}). Esc dismisses the details.",
            err.code()
        );
    }
}

fn printable(key: &KeyEvent) -> Option<char> {
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Char(c) => Some(c),
        _ => None,
    }
}

/// Where `e`/`i` default to when the operator does not type a path.
pub fn default_exchange_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("adv360-export")
}
