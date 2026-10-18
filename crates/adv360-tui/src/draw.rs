//! Rendering for [`App`].
//!
//! Nothing here mutates the session or the filesystem: every line is built from state the core
//! already computed. Terminal width and height are treated as hostile — every layout degrades to a
//! readable list or drops optional panels instead of panicking.

use adv360_core::prelude::*;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Panel};

impl App {
    /// Render the whole interface. Called from `Terminal::draw`.
    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        if area.width == 0 || area.height == 0 {
            return;
        }
        if area.width < 56 || area.height < 16 {
            self.draw_compact(frame, area);
        } else {
            self.draw_full(frame, area);
        }
        self.draw_error(frame, area);
    }

    fn draw_full(&self, frame: &mut Frame, area: Rect) {
        let header_h = 2.min(area.height);
        let footer_h = 2.min(area.height.saturating_sub(header_h));
        let body_h = area.height.saturating_sub(header_h + footer_h);

        let header_area = Rect::new(area.x, area.y, area.width, header_h);
        frame.render_widget(Paragraph::new(Text::from(self.header_lines())), header_area);

        if body_h > 0 {
            let body_area = Rect::new(area.x, area.y + header_h, area.width, body_h);
            let columns =
                Layout::horizontal([Constraint::Percentage(54), Constraint::Percentage(46)])
                    .split(body_area);
            let map_area = columns[0];
            let panel_area = columns[1];

            let map_lines = self.map_lines(map_area.width);
            if !map_lines.is_empty() && map_lines.len() <= body_h as usize && map_area.width >= 30 {
                frame.render_widget(Paragraph::new(Text::from(map_lines)), map_area);
            } else {
                let list = self.map_list_lines(body_h as usize);
                frame.render_widget(
                    Paragraph::new(Text::from(list))
                        .block(Block::bordered().title("Map (list fallback)")),
                    map_area,
                );
            }

            let lines = if self.panel == Panel::Map {
                self.inspector_lines()
            } else {
                self.panel_lines()
            };
            frame.render_widget(
                Paragraph::new(Text::from(lines))
                    .block(Block::bordered().title(self.panel.title()))
                    .wrap(Wrap { trim: false }),
                panel_area,
            );
        }

        let footer_area = Rect::new(area.x, area.y + header_h + body_h, area.width, footer_h);
        frame.render_widget(Paragraph::new(Text::from(self.footer_lines())), footer_area);
    }

    fn draw_compact(&self, frame: &mut Frame, area: Rect) {
        let header_h = 1.min(area.height);
        let footer_h = 1.min(area.height.saturating_sub(header_h));
        let body_h = area.height.saturating_sub(header_h + footer_h);

        frame.render_widget(
            Paragraph::new(Text::from(vec![self.compact_header_line()])),
            Rect::new(area.x, area.y, area.width, header_h),
        );

        if body_h > 0 {
            let body = if self.panel == Panel::Map {
                self.map_list_lines(body_h as usize)
            } else {
                self.panel_lines()
            };
            frame.render_widget(
                Paragraph::new(Text::from(body)).wrap(Wrap { trim: false }),
                Rect::new(area.x, area.y + header_h, area.width, body_h),
            );
        }

        frame.render_widget(
            Paragraph::new(Text::from(self.footer_lines())),
            Rect::new(area.x, area.y + header_h + body_h, area.width, footer_h),
        );
    }

    /// A full-width box with the error and every recovery hint the core gave.
    fn draw_error(&self, frame: &mut Frame, area: Rect) {
        let Some(error) = &self.error else {
            return;
        };
        let height = (area.height / 2).max(5).min(area.height);
        let width = area.width.saturating_sub(4).max(10).min(area.width);
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let rect = Rect::new(x, y, width, height);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(error.as_str())
                .block(Block::bordered().title("Error — recovery hints (Esc dismisses)"))
                .wrap(Wrap { trim: false }),
            rect,
        );
    }

    // -------------------------------------------------------------------------------------
    // Header, footer, inspector
    // -------------------------------------------------------------------------------------

    pub(crate) fn header_lines(&self) -> Vec<Line<'static>> {
        let layout = self.session.layout();
        let diff = self.session.diff();
        let validation = self.session.validation();
        let state = match self
            .session
            .key_state(self.profile, self.layer, &self.selected_position)
        {
            Some(key) if key.is_custom() => "custom",
            Some(key) if key.is_default_uncertain() => "factory (uncertain)",
            Some(_) => "factory",
            None => "unknown",
        };
        vec![
            Line::from(format!(
                "adv360-config {} | v-Drive: {}",
                adv360_core::VERSION,
                self.session.device().root.display()
            )),
            Line::from(format!(
                "Profile {} | Layer {} | Key: {} [{}] | Layout {} ({}) | changes {} | errors {} warnings {} | undo {} redo {}",
                self.profile,
                self.layer.short_label(),
                self.selected_position,
                state,
                layout.layout,
                layout.source.label(),
                diff.changed_line_count(),
                validation.error_count(),
                validation.warning_count(),
                self.session.undo_descriptions().len(),
                self.session.redo_descriptions().len(),
            )),
        ]
    }

    fn compact_header_line(&self) -> Line<'static> {
        Line::from(format!(
            "P{} {} {} | {} | E{} W{}",
            self.profile.0,
            self.layer.short_label(),
            self.selected_position,
            self.panel.title(),
            self.session.validation().error_count(),
            self.session.validation().warning_count(),
        ))
    }

    pub(crate) fn footer_lines(&self) -> Vec<Line<'static>> {
        vec![
            Line::from(self.status.clone()),
            Line::from(
                // Kept short enough to fit a normal terminal; the full list is in the help panel.
                "? help | q quit | Tab panels | p/P profile | l/L layer | hjkl move | / search | \
                 Enter edit | u undo | d diff | v validation | w write | b backup | e export",
            ),
        ]
    }

    pub(crate) fn inspector_lines(&self) -> Vec<Line<'static>> {
        let Some(key) = self
            .session
            .key_state(self.profile, self.layer, &self.selected_position)
        else {
            return vec![Line::from("No key selected.")];
        };
        let mut lines = Vec::new();
        lines.push(Line::from(format!(
            "{}  (keycap {}, map {}, {} / {})",
            key.position.token,
            key.position.keycap,
            key.position.map_label,
            key.position.half.label(),
            block_label(key.position.block),
        )));
        lines.push(Line::from(format!(
            "Layer {} ({})",
            key.layer.short_label(),
            key.layer.led_color_hint()
        )));
        lines.push(Line::from(format!("Now: {}", key.effective_description())));
        lines.push(Line::from(format!(
            "State: {}",
            if key.is_custom() {
                "custom line active"
            } else if key.is_default_uncertain() {
                "factory action, but Kinesis does not publish it"
            } else {
                "factory action (documented)"
            }
        )));
        lines.push(Line::from(format!(
            "Factory source: {}",
            default_source_label(key.default.source)
        )));
        if key.is_default_uncertain() {
            lines.push(styled(
                "This value is uncertain and is never presented as documented.",
                Modifier::UNDERLINED,
            ));
        }
        lines.push(Line::from(format!(
            "Config lines: {}",
            if key.lines.is_empty() {
                "none".to_string()
            } else {
                format_numbers(&key.lines)
            }
        )));
        if key.has_disabled_lines {
            lines.push(Line::from(
                "Some lines for this key are switched off with '*'.",
            ));
        }
        lines.push(Line::from(""));

        let action_tokens = binding_tokens(key.binding.as_ref());
        if action_tokens.is_empty() {
            lines.push(preview_line(key.default.action, self.session.layout()));
        } else {
            for token in action_tokens {
                lines.push(preview_line(&token, self.session.layout()));
            }
        }
        for finding in &key.findings {
            lines.push(Line::from(format!(
                "[{}] line {:?}: {}",
                finding.severity.label(),
                finding.line,
                finding.message
            )));
        }
        lines
    }

    // -------------------------------------------------------------------------------------
    // The physical map
    // -------------------------------------------------------------------------------------

    fn map_lines(&self, width: u16) -> Vec<Line<'static>> {
        let cell_w = ((width.saturating_sub(1)) / 7) as usize;
        if cell_w < 4 {
            return Vec::new();
        }
        let cell_w = cell_w.min(7);
        let keys = self.session.layer_keys(self.profile, self.layer);
        let mut lines = Vec::new();
        for half in [Half::Left, Half::Right] {
            lines.push(Line::from(format!(
                "{} (main keywell and thumb cluster)",
                half.label()
            )));
            lines.extend(self.block_lines(half, KeyBlock::Main, cell_w, &keys));
            lines.extend(self.block_lines(half, KeyBlock::Thumb, cell_w, &keys));
        }
        lines.push(Line::from(
            "FS360 foot pedals (factory action not published) | * = custom, ! = locked (SmartSet key)",
        ));
        lines.extend(self.block_lines(Half::Pedal, KeyBlock::Pedal, cell_w, &keys));
        lines
    }

    fn block_lines(
        &self,
        half: Half,
        block: KeyBlock,
        cell_w: usize,
        keys: &[KeyState],
    ) -> Vec<Line<'static>> {
        let items = positions::positions_in(half, block);
        if items.is_empty() {
            return Vec::new();
        }
        let max_x = items.iter().map(|p| p.x).max().unwrap_or(0);
        let max_y = items.iter().map(|p| p.y).max().unwrap_or(0);
        let mut lines = Vec::new();
        for y in 0..=max_y {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for x in 0..=max_x {
                let item = items.iter().find(|p| p.x == x && p.y == y);
                match item {
                    Some(position) => {
                        let selected = position.token.eq_ignore_ascii_case(&self.selected_position);
                        let key = keys
                            .iter()
                            .find(|k| k.position.token.eq_ignore_ascii_case(position.token));
                        let custom = key.is_some_and(|k| k.is_custom());
                        let uncertain = key.is_some_and(|k| k.is_default_uncertain());
                        let label = if selected && cell_w >= 6 {
                            format!("[{}]", position.token)
                        } else if selected {
                            format!(">{}", position.token)
                        } else if !position.programmable {
                            format!("{}!", position.token)
                        } else if custom {
                            format!("{}*", position.token)
                        } else {
                            position.token.to_string()
                        };
                        let shown = truncate_chars(&label, cell_w);
                        let padded = format!("{shown:<cell_w$}");
                        let mut style = Style::default();
                        if selected {
                            style = style.add_modifier(Modifier::REVERSED);
                        } else if custom {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                        if uncertain {
                            style = style.add_modifier(Modifier::UNDERLINED);
                        }
                        spans.push(Span::styled(padded, style));
                    }
                    None => spans.push(Span::raw(" ".repeat(cell_w))),
                }
            }
            lines.push(Line::from(spans));
        }
        lines
    }

    fn map_list_lines(&self, limit: usize) -> Vec<Line<'static>> {
        let keys = self.session.layer_keys(self.profile, self.layer);
        let selected = keys
            .iter()
            .position(|k| {
                k.position
                    .token
                    .eq_ignore_ascii_case(&self.selected_position)
            })
            .unwrap_or(0);
        let visible = limit.saturating_sub(2).max(1);
        let start = selected.saturating_sub(visible / 2);
        let mut lines = vec![Line::from(format!(
            "Terminal too small for the map: readable key list ({} keys).",
            keys.len()
        ))];
        for key in keys.iter().skip(start).take(visible) {
            let marker = if key
                .position
                .token
                .eq_ignore_ascii_case(&self.selected_position)
            {
                ">"
            } else {
                " "
            };
            let state = if key.is_custom() {
                "custom"
            } else if key.is_default_uncertain() {
                "factory?"
            } else {
                "factory"
            };
            lines.push(Line::from(format!(
                "{marker} {:<5} {:<34} ({state})",
                key.position.token,
                key.effective_description()
            )));
        }
        lines
    }

    // -------------------------------------------------------------------------------------
    // The other panels
    // -------------------------------------------------------------------------------------

    pub(crate) fn panel_lines(&self) -> Vec<Line<'static>> {
        match self.panel {
            Panel::Map => self.inspector_lines(),
            Panel::Catalogue => self.catalogue_lines(),
            Panel::Changes => self.changes_lines(),
            Panel::Validation => self.validation_lines(),
            Panel::Diff => self.diff_lines(),
            Panel::Presets => self.preset_lines(),
            Panel::Layout => self.layout_lines(),
            Panel::Write => self.write_lines(),
            Panel::Profiles => self.profile_lines(),
            Panel::Help => self.help_lines(),
        }
    }

    fn catalogue_lines(&self) -> Vec<Line<'static>> {
        let results = tokens::search(&self.search);
        let mut lines = vec![Line::from(format!(
            "Search: '{}'  ({} matching actions)",
            self.search,
            results.len()
        ))];
        let selected = self.catalogue_index.min(results.len().saturating_sub(1));
        for (index, token) in results.iter().enumerate() {
            let marker = if index == selected { ">" } else { " " };
            let text = format!(
                "{marker} {:<6} {:<18} {} (usage {})",
                token.id,
                token.category.label(),
                token.name,
                token.usage
            );
            if index == selected {
                lines.push(styled_owned(text, Modifier::REVERSED));
            } else {
                lines.push(Line::from(text));
            }
        }
        if let Some(token) = results.get(selected) {
            lines.push(Line::from(""));
            lines.push(preview_line(token.id, self.session.layout()));
        }
        lines.push(Line::from(
            "Enter assigns a remap | t assigns Tap-and-Hold (hold=esc) | m assigns a macro | / searches again",
        ));
        lines
    }

    fn changes_lines(&self) -> Vec<Line<'static>> {
        let changes = self.session.pending_changes();
        if changes.is_empty() {
            return vec![Line::from(
                "No pending changes: the draft matches the v-Drive.",
            )];
        }
        let mut lines = vec![Line::from(format!("{} pending change(s):", changes.len()))];
        for change in &changes {
            lines.push(Line::from(format!(
                " - {}  [{}]",
                change.description, change.file
            )));
        }
        lines
    }

    fn validation_lines(&self) -> Vec<Line<'static>> {
        let report = self.session.validation().sorted();
        if report.findings.is_empty() {
            return vec![Line::from("No validation findings.")];
        }
        let mut lines = vec![Line::from(format!(
            "{} error(s), {} warning(s), {} info finding(s). Errors block writing.",
            report.error_count(),
            report.warning_count(),
            report.info_count()
        ))];
        for finding in &report.findings {
            let line = finding
                .line
                .map(|l| l.to_string())
                .unwrap_or_else(|| "-".to_string());
            lines.push(Line::from(format!(
                "[{}] {}:{} {} ({})",
                finding.severity.label(),
                finding.relative_path,
                line,
                finding.message,
                finding.code
            )));
            if let Some(hint) = &finding.hint {
                lines.push(Line::from(format!("      hint: {hint}")));
            }
        }
        lines
    }

    fn diff_lines(&self) -> Vec<Line<'static>> {
        let summary = self.session.diff();
        let mut lines = Vec::new();
        if summary.key_changes.is_empty() && summary.changed_line_count() == 0 {
            lines.push(Line::from("No differences from the v-Drive."));
            return lines;
        }
        lines.push(Line::from(format!(
            "{} key change(s), {} changed line(s).",
            summary.key_changes.len(),
            summary.changed_line_count()
        )));
        for change in &summary.key_changes {
            lines.push(Line::from(format!(
                " - {} [{}]",
                change.describe(),
                change.kind.label()
            )));
        }
        lines.push(Line::from(""));
        for file in &summary.files {
            if file.changed_lines.is_empty() {
                continue;
            }
            lines.push(Line::from(format!(
                "--- {} (lines {})",
                file.relative_path,
                format_numbers(&file.changed_lines)
            )));
            for text in file.unified.lines().take(120) {
                lines.push(Line::from(text.to_string()));
            }
        }
        lines
    }

    fn preset_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let symbols_len = self.draft.symbols.assignments.len();
        let navigation_len = self.draft.navigation.assignments.len();
        lines.push(Line::from(
            "Review drafts only. Nothing here is applied until you press a or Enter.",
        ));
        lines.push(Line::from(""));
        lines.push(Line::from(format!("== {} ==", self.draft.symbols.title)));
        lines.push(Line::from(format!(
            "Rationale: {}",
            self.draft.symbols.rationale
        )));
        for (offset, row) in self.draft.symbols.table().into_iter().enumerate() {
            lines.push(preset_row_line(&row, offset == self.preset_index));
        }
        lines.push(Line::from(format!("== {} ==", self.draft.navigation.title)));
        for (offset, row) in self.draft.navigation.table().into_iter().enumerate() {
            let global = symbols_len + offset;
            lines.push(preset_row_line(&row, global == self.preset_index));
        }
        lines.push(Line::from("== Access keys for the navigation layer =="));
        for (offset, option) in self.draft.access_key_options.iter().enumerate() {
            let index = symbols_len + navigation_len + offset;
            let marker = if index == self.preset_index { ">" } else { " " };
            let text = format!(
                "{marker} {:<6} {:<28} {}{}",
                option.position,
                option.keycap,
                option.line,
                option
                    .cost
                    .as_ref()
                    .map(|c| format!("  (cost: {c})"))
                    .unwrap_or_else(|| "  (costs nothing)".to_string())
            );
            if index == self.preset_index {
                lines.push(styled_owned(text, Modifier::REVERSED));
            } else {
                lines.push(Line::from(text));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from("Caveats from the draft:"));
        for caveat in self
            .draft
            .symbols
            .caveats
            .iter()
            .chain(self.draft.navigation.caveats.iter())
        {
            lines.push(Line::from(format!(" - {caveat}")));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Press a or Enter to apply the highlighted proposal as reviewable draft edits.",
        ));
        lines
    }

    fn layout_lines(&self) -> Vec<Line<'static>> {
        let layout = self.session.layout();
        let mut lines = vec![
            Line::from(format!("XKB layout: {}", layout.layout)),
            Line::from(format!(
                "Variant: {}",
                layout.variant.clone().unwrap_or_else(|| "none".to_string())
            )),
            Line::from(format!("Source: {}", layout.source.label())),
            Line::from(format!("Detail: {}", layout.detail)),
            Line::from(format!(
                "Character preview reliable: {}",
                if layout.preview_is_reliable() {
                    "yes"
                } else {
                    "no, characters are marked uncertain"
                }
            )),
            Line::from(format!(
                "Supported preview layouts: {}",
                kbd_layout::supported_layouts().join(", ")
            )),
        ];
        if let Some(warning) = layout.reliability_warning() {
            lines.push(styled_owned(warning, Modifier::UNDERLINED));
        }
        lines.push(Line::from(
            "The token is what the keyboard sends; the character is what Linux produces from it. \
             They are shown separately and are never assumed equal.",
        ));
        lines.push(Line::from(
            "Press c or Enter to correct the layout manually, then type the XKB name and press Enter.",
        ));
        lines
    }

    fn write_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from("WRITE CONFIRMATION (nothing is written yet)")];
        match self.target_relative_path() {
            Some(path) => lines.push(Line::from(format!("Target file: {path}"))),
            None => lines.push(Line::from(
                "Target file: this profile has no layout file on the v-Drive.",
            )),
        }
        let changed = self.target_changed_lines();
        lines.push(Line::from(format!(
            "Changed line numbers ({}): {}",
            changed.len(),
            format_numbers(&changed)
        )));
        lines.push(Line::from("Pending change list:"));
        for change in self.session.pending_changes().iter().take(40) {
            lines.push(Line::from(format!(
                " - {}  [{}]",
                change.description, change.file
            )));
        }
        lines.push(Line::from(""));
        match self.session.last_backup() {
            Some(set) => lines.push(Line::from(format!(
                "Backup that will be taken: already taken at {} ({} file(s), {}).",
                set.directory.display(),
                set.files.len(),
                set.created_at
            ))),
            None => lines.push(Line::from(
                "Backup that will be taken: none yet. Press b here to take a dated, verified backup before writing.",
            )),
        }
        lines.push(Line::from("Eject instructions for after the write:"));
        for instruction in device::eject_instructions(&self.session.device().root) {
            lines.push(Line::from(format!(" - {instruction}")));
        }
        lines.push(Line::from(""));
        match &self.write_report {
            Some(report) => {
                lines.push(styled_owned(
                    format!(
                        "WRITE VERIFIED: {} ({} bytes, {} changed line(s)).",
                        report.relative_path,
                        report.bytes_written,
                        report.changed_lines.len()
                    ),
                    Modifier::BOLD,
                ));
                lines.push(Line::from(format!(
                    "Backup location: {}",
                    report.backup_directory.display()
                )));
                lines.push(Line::from("Next steps:"));
                for step in &report.next_steps {
                    lines.push(Line::from(format!(" - {step}")));
                }
                lines.push(Line::from("Eject now:"));
                for step in &report.eject_instructions {
                    lines.push(Line::from(format!(" - {step}")));
                }
            }
            None => {
                lines.push(Line::from(
                    "Press Enter, then type the word WRITE exactly, then Enter again.",
                ));
                lines.push(Line::from(format!("Confirmation: {}", self.write_input)));
                lines.push(Line::from(
                    "The count above is the only count that will be accepted by the core.",
                ));
            }
        }
        lines
    }

    fn profile_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from(
            "Profiles 1-9 are programmable. Profile 0 is listed but never editable.",
        )];
        for summary in self.session.profile_summary() {
            let marker = if summary.profile.0 == self.profile_cursor {
                ">"
            } else {
                " "
            };
            let editable = if summary.editable {
                "editable"
            } else {
                "LOCKED"
            };
            lines.push(Line::from(format!(
                "{marker} Profile {} ({editable}) | file {} | changes {} | errors {} | {}",
                summary.profile.0,
                summary.relative_path.as_deref().unwrap_or("(none)"),
                summary.pending_changed_lines,
                summary.validation_errors,
                summary.led_color_hint
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "j/k moves the cursor; Enter selects an editable profile. Profile 0 shows the locked message.",
        ));
        lines
    }

    fn help_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from("Keys:"),
            Line::from("  q quit | ? help | Esc close/dismiss | Tab / Shift-Tab cycle panels"),
            Line::from("  p / Shift-P next / previous profile (1-9; profile 0 is locked)"),
            Line::from("  l / Shift-L next / previous layer (base, keypad, fn1, fn2, fn3)"),
            Line::from("  arrows / h j k move the key cursor | Enter open or edit"),
            Line::from("  / search the action catalogue | u undo | Ctrl-R redo"),
            Line::from("  d differences | v validation | g preset drafts | y keyboard layout"),
            Line::from("  w write flow | b backup | e export | i import"),
            Line::from(""),
            Line::from("Documentation this build was written against:"),
        ];
        for (name, detail) in adv360_core::SOURCES {
            lines.push(Line::from(format!("  {name}: {detail}")));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("How to mount the v-Drive:"));
        for step in device::mount_instructions() {
            lines.push(Line::from(format!(" - {step}")));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Every core error is shown with CoreError::recovery_hints(); none are swallowed.",
        ));
        lines
    }
}

fn block_label(block: KeyBlock) -> &'static str {
    match block {
        KeyBlock::Main => "main keywell",
        KeyBlock::Thumb => "thumb cluster",
        KeyBlock::Pedal => "foot pedal row",
    }
}

fn default_source_label(source: DefaultSource) -> &'static str {
    match source {
        DefaultSource::Documented => "documented for this key and layer",
        DefaultSource::DocumentedNoAction => "documented: blank by default",
        DefaultSource::DocumentedSharedFunctionMap => "documented shared Fn map",
        DefaultSource::InferredFromBase => "inferred from the base layer (uncertain)",
        DefaultSource::Uncertain => "not published by Kinesis (uncertain)",
    }
}

fn binding_tokens(binding: Option<&Binding>) -> Vec<String> {
    match binding {
        None => Vec::new(),
        Some(Binding::Remap(remap)) => remap
            .tap_actions()
            .iter()
            .chain(remap.hold_actions().iter())
            .map(|token| token.to_string())
            .collect(),
        Some(Binding::Macro(macro_line)) => macro_line
            .body
            .iter()
            .filter_map(|atom| match atom {
                MacroAtom::Action { token, .. } => Some(token.clone()),
                _ => None,
            })
            .collect(),
    }
}

fn preview_line(token: &str, layout: &KeyboardLayout) -> Line<'static> {
    let preview = kbd_layout::preview(token, layout);
    let character = match preview.character {
        Some(character) => {
            let shifted = preview
                .shifted_character
                .map_or_else(String::new, |s| format!(" (shift '{}')", s));
            format!("'{}'{}", character, shifted)
        }
        None => "no printable character".to_string(),
    };
    let confidence = if preview.confident {
        "confident".to_string()
    } else {
        format!(
            "UNCERTAIN: {}",
            preview
                .note
                .clone()
                .unwrap_or_else(|| "not computed from an encoded mapping".to_string())
        )
    };
    Line::from(format!(
        "Token '{}' ({}) -> character {} [{}]",
        preview.token, preview.usb_label, character, confidence
    ))
}

fn preset_row_line(row: &presets::PresetRow, selected: bool) -> Line<'static> {
    let marker = if selected { ">" } else { " " };
    let text = format!(
        "{marker} {:<5} {:<12} {:<30} {}",
        row.position, row.keycap, row.produces, row.line
    );
    if selected {
        styled_owned(text, Modifier::REVERSED)
    } else {
        Line::from(text)
    }
}

fn styled(text: &str, modifier: Modifier) -> Line<'static> {
    styled_owned(text.to_string(), modifier)
}

fn styled_owned(text: String, modifier: Modifier) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().add_modifier(modifier)))
}

fn format_numbers(numbers: &[usize]) -> String {
    if numbers.is_empty() {
        return "-".to_string();
    }
    numbers
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<String>>()
        .join(", ")
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}
