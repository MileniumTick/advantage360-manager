//! The GTK4 widget layer.
//!
//! This module is deliberately thin: it renders the pure [`crate::view::ViewModel`], forwards
//! operator actions to the [`Session`] facade, and never reads, parses, validates or writes a
//! configuration file itself. Backups, writes, exports and imports touch the filesystem, so they
//! run on a worker thread and their result is delivered back to the main loop over a channel.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use adv360_core::backup::BackupSet;
use adv360_core::error::{CoreError, CoreResult};
use adv360_core::layer::{LayerId, ProfileId};
use adv360_core::model::{Eol, LineKind};
use adv360_core::positions::{Half, KeyBlock};
use adv360_core::presets::{self, Assignment};
use adv360_core::session::{Session, SessionOptions, WriteConfirmation, WriteReport};
use adv360_core::tokens::{self, TokenCategory};
use adv360_core::{APP_NAME, SOURCES, VERSION};
use gtk4::glib::object::{Cast, IsA};
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, DropDown, Entry,
    Frame, Grid, Label, Notebook, Orientation, Paned, ScrolledWindow, SearchEntry, Spinner,
    TextView, ToggleButton, Window,
};

use crate::view::{build_view_model, render_key_map, KeyCell, ViewModel, ViewState};

type SharedState = Rc<RefCell<AppState>>;

/// The interface's runtime state, kept alive by the widget signal handlers.
struct AppState {
    session: Option<Session>,
    view: ViewState,
    options: SessionOptions,
    status: String,
    busy: bool,
}

/// A result produced on a worker thread and delivered to the UI thread.
enum WorkOutput {
    Backup(BackupSet),
    Written(Box<WriteReport>),
    Exported(Vec<PathBuf>),
    Imported(Vec<String>),
}

/// Every widget the interface updates or a test looks up.
struct Widgets {
    window: ApplicationWindow,
    header_device: Label,
    banner: GtkBox,
    banner_label: Label,
    mount_view: TextView,
    manual_path: Entry,
    open_folder: Button,
    retry_button: Button,
    profile_combo: DropDown,
    layer_tabs: GtkBox,
    layer_buttons: Vec<ToggleButton>,
    key_well_left: GtkBox,
    key_well_right: GtkBox,
    key_well_pedal: GtkBox,
    inspector_page: GtkBox,
    inspector_current: Label,
    inspector_factory: Label,
    inspector_preview: Label,
    inspector_lines: Label,
    inspector_findings: Label,
    inspector_action: Entry,
    assign_remap: Button,
    inspector_tap: Entry,
    inspector_hold: Entry,
    inspector_delay: Entry,
    assign_taphold: Button,
    macro_body: Entry,
    assign_macro: Button,
    remove_binding: Button,
    catalogue_search: SearchEntry,
    catalogue_category: DropDown,
    catalogue_count: Label,
    catalogue_list: GtkBox,
    pending_panel: GtkBox,
    diff_files: GtkBox,
    diff_view: TextView,
    validation_panel: GtkBox,
    preset_review: TextView,
    preset_apply_symbols: Button,
    preset_apply_navigation: Button,
    preset_access_combo: DropDown,
    preset_apply_access: Button,
    layout_info: Label,
    layout_override: Entry,
    layout_apply: Button,
    export_entry: Entry,
    undo_button: Button,
    redo_button: Button,
    backup_button: Button,
    export_button: Button,
    import_button: Button,
    write_button: Button,
    about_button: Button,
    status_label: Label,
    spinner: Spinner,
}

/// The built interface: the window plus accessors for the widgets a test may assert on.
pub struct Ui {
    window: ApplicationWindow,
    app: SharedState,
    widgets: Rc<Widgets>,
}

impl Ui {
    /// The top-level window.
    pub fn window(&self) -> &ApplicationWindow {
        &self.window
    }

    /// True when a session is loaded (false while the v-Drive is absent).
    pub fn session_available(&self) -> bool {
        self.app.borrow().session.is_some()
    }

    /// The profile selector.
    pub fn profile_selector(&self) -> &DropDown {
        &self.widgets.profile_combo
    }

    /// The container holding the five layer tabs.
    pub fn layer_tabs(&self) -> &GtkBox {
        &self.widgets.layer_tabs
    }

    /// The left key well.
    pub fn key_well_left(&self) -> &GtkBox {
        &self.widgets.key_well_left
    }

    /// The right key well.
    pub fn key_well_right(&self) -> &GtkBox {
        &self.widgets.key_well_right
    }

    /// The foot-pedal key well.
    pub fn key_well_pedal(&self) -> &GtkBox {
        &self.widgets.key_well_pedal
    }

    /// The key inspector page.
    pub fn inspector(&self) -> &GtkBox {
        &self.widgets.inspector_page
    }

    /// The searchable action catalogue's search box.
    pub fn catalogue_search(&self) -> &SearchEntry {
        &self.widgets.catalogue_search
    }

    /// The pending-changes panel.
    pub fn pending_panel(&self) -> &GtkBox {
        &self.widgets.pending_panel
    }

    /// The diff text view.
    pub fn diff_view(&self) -> &TextView {
        &self.widgets.diff_view
    }

    /// The validation panel.
    pub fn validation_panel(&self) -> &GtkBox {
        &self.widgets.validation_panel
    }

    /// The preset review text view.
    pub fn preset_review(&self) -> &TextView {
        &self.widgets.preset_review
    }

    pub fn undo_button(&self) -> &Button {
        &self.widgets.undo_button
    }

    pub fn redo_button(&self) -> &Button {
        &self.widgets.redo_button
    }

    pub fn backup_button(&self) -> &Button {
        &self.widgets.backup_button
    }

    pub fn export_button(&self) -> &Button {
        &self.widgets.export_button
    }

    pub fn import_button(&self) -> &Button {
        &self.widgets.import_button
    }

    pub fn write_button(&self) -> &Button {
        &self.widgets.write_button
    }

    pub fn about_button(&self) -> &Button {
        &self.widgets.about_button
    }

    /// Find a widget anywhere in the window by its stable widget name.
    pub fn find_widget(&self, name: &str) -> Option<gtk4::Widget> {
        find_by_name(&self.window, name)
    }
}

/// Build the whole interface for a loaded session.
pub fn build_ui(session: Session) -> Ui {
    build_ui_inner(Some(session), None, SessionOptions::default(), None)
}

/// Build and run the GTK4 application.
///
/// The command's own arguments are deliberately *not* handed to GTK: `adv360-config` passes the
/// v-Drive path and the keyboard layout in `options`, and GTK would reject them as unknown options
/// if it saw them. Running with an empty argument list is what keeps `adv360-config --device PATH
/// gui` working.
pub fn run(options: SessionOptions) -> Result<(), Box<dyn std::error::Error>> {
    let application = Application::builder()
        .application_id("org.adv360.config")
        .build();
    let opts = options.clone();
    application.connect_activate(move |app| activate(app, opts.clone()));
    application.run_with_args::<&str>(&[]);
    Ok(())
}

fn activate(application: &Application, options: SessionOptions) {
    let (session, disconnected) = match Session::open(options.clone()) {
        Ok(s) => (Some(s), None),
        Err(e) => (None, Some(e)),
    };
    let ui = build_ui_inner(session, disconnected, options, Some(application));
    ui.window.present();
    // The window and its signal handlers own the state; keep this wrapper alive for the run.
    std::mem::forget(ui);
}

fn build_ui_inner(
    session: Option<Session>,
    disconnected: Option<CoreError>,
    options: SessionOptions,
    application: Option<&Application>,
) -> Ui {
    let window = ApplicationWindow::builder()
        .title("adv360-config")
        .default_width(1480)
        .default_height(880)
        .build();
    if let Some(app) = application {
        window.set_application(Some(app));
    }
    let widgets = Rc::new(Widgets::build(&window));
    widgets.export_entry.set_text(&default_export_dir());
    let app = Rc::new(RefCell::new(AppState {
        session,
        view: ViewState::default(),
        options,
        status: String::new(),
        busy: false,
    }));
    wire(&app, &widgets);
    refresh(&app, &widgets);
    if let Some(error) = disconnected {
        widgets.banner.set_visible(true);
        widgets
            .banner_label
            .set_text(&format!("The ADV360 v-Drive is not available: {error}"));
        let mut text = String::new();
        for hint in error.recovery_hints() {
            text.push_str(&hint);
            text.push('\n');
        }
        text.push('\n');
        for line in adv360_core::device::mount_instructions() {
            text.push_str(&line);
            text.push('\n');
        }
        widgets.mount_view.buffer().set_text(&text);
    }
    Ui {
        window,
        app,
        widgets,
    }
}

impl Widgets {
    fn build(window: &ApplicationWindow) -> Widgets {
        let header = gtk4::HeaderBar::new();
        let header_device = Label::new(Some("(no v-Drive)"));
        header_device.set_widget_name("device-path");
        header_device.set_max_width_chars(48);
        header.set_title_widget(Some(&header_device));

        let profile_labels: Vec<String> = ProfileId::all()
            .map(|p| {
                format!(
                    "Profile {} - {}",
                    p.0,
                    if p.is_editable() {
                        "editable"
                    } else {
                        "non-editable"
                    }
                )
            })
            .collect();
        let profile_combo = dropdown(&profile_labels);
        profile_combo.set_widget_name("profile-selector");
        profile_combo.set_selected(1);
        let profile_row = GtkBox::new(Orientation::Horizontal, 4);
        profile_row.append(&Label::new(Some("Profile")));
        profile_row.append(&profile_combo);
        header.pack_start(&profile_row);

        let layer_tabs = GtkBox::new(Orientation::Horizontal, 0);
        layer_tabs.set_widget_name("layer-tabs");
        let mut layer_buttons: Vec<ToggleButton> = Vec::new();
        let mut first: Option<ToggleButton> = None;
        for layer in LayerId::all() {
            let button = ToggleButton::with_label(layer.short_label());
            button.set_widget_name(&format!("layer-tab-{}", layer.name()));
            match &first {
                Some(group) => button.set_group(Some(group)),
                None => {
                    button.set_active(true);
                    first = Some(button.clone());
                }
            }
            layer_tabs.append(&button);
            layer_buttons.push(button);
        }
        header.pack_start(&layer_tabs);

        let about_button = Button::with_label("About / Help");
        about_button.set_widget_name("about-button");
        header.pack_end(&about_button);

        let spinner = Spinner::new();
        spinner.set_widget_name("busy-spinner");
        spinner.set_visible(false);
        header.pack_end(&spinner);

        window.set_titlebar(Some(&header));

        let root = GtkBox::new(Orientation::Vertical, 6);

        let banner = GtkBox::new(Orientation::Vertical, 6);
        banner.set_widget_name("device-banner");
        banner.set_visible(true);
        let banner_label = Label::new(Some("The ADV360 v-Drive is not available."));
        banner_label.set_xalign(0.0);
        banner_label.set_wrap(true);
        banner_label.set_margin_top(6);
        banner_label.set_margin_start(6);
        banner_label.set_margin_end(6);
        banner.append(&banner_label);

        let manual_row = GtkBox::new(Orientation::Horizontal, 4);
        let manual_path = Entry::new();
        manual_path.set_widget_name("manual-device-path");
        manual_path.set_placeholder_text(Some("/run/media/$USER/ADV360"));
        manual_path.set_hexpand(true);
        let open_folder = Button::with_label("Open this folder");
        open_folder.set_widget_name("open-folder-button");
        let retry_button = Button::with_label("Retry detection");
        retry_button.set_widget_name("retry-detect-button");
        manual_row.append(&manual_path);
        manual_row.append(&open_folder);
        manual_row.append(&retry_button);
        banner.append(&manual_row);

        let mount_view = TextView::new();
        mount_view.set_editable(false);
        mount_view.set_monospace(true);
        mount_view.set_widget_name("mount-instructions");
        let mount_scroll = ScrolledWindow::builder()
            .min_content_height(120)
            .child(&mount_view)
            .build();
        banner.append(&mount_scroll);
        root.append(&banner);

        let content = GtkBox::new(Orientation::Horizontal, 8);
        content.set_vexpand(true);

        let key_well_left = GtkBox::new(Orientation::Vertical, 4);
        key_well_left.set_widget_name("key-well-left");
        let key_well_right = GtkBox::new(Orientation::Vertical, 4);
        key_well_right.set_widget_name("key-well-right");
        let key_well_pedal = GtkBox::new(Orientation::Vertical, 4);
        key_well_pedal.set_widget_name("key-well-pedal");
        // The two modules wrap onto separate lines when the window is too narrow to show both,
        // which happens constantly under a tiling compositor. A `FlowBox` does that from the real
        // allocated width, so there is no threshold to guess.
        let modules = gtk4::FlowBox::new();
        modules.set_widget_name("key-modules");
        modules.set_selection_mode(gtk4::SelectionMode::None);
        modules.set_max_children_per_line(2);
        modules.set_min_children_per_line(1);
        modules.set_column_spacing(16);
        modules.set_row_spacing(8);
        modules.set_halign(Align::Start);
        modules.insert(&key_well_left, -1);
        modules.insert(&key_well_right, -1);
        let map_box = GtkBox::new(Orientation::Vertical, 8);
        map_box.append(&modules);
        map_box.append(&key_well_pedal);
        let map_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .min_content_width(300)
            .child(&map_box)
            .build();

        let notebook = Notebook::new();
        notebook.set_widget_name("side-panels");
        notebook.set_hexpand(true);

        // --- Inspector -------------------------------------------------------------
        let inspector_page = GtkBox::new(Orientation::Vertical, 6);
        inspector_page.set_widget_name("key-inspector");
        inspector_page.set_margin_top(8);
        inspector_page.set_margin_start(8);
        inspector_page.set_margin_end(8);
        let inspector_current = info_label("inspector-current");
        let inspector_factory = info_label("inspector-factory");
        let inspector_preview = info_label("inspector-preview");
        let inspector_lines = info_label("inspector-lines");
        let inspector_findings = info_label("inspector-findings");
        inspector_page.append(&inspector_current);
        inspector_page.append(&inspector_factory);
        inspector_page.append(&inspector_preview);
        inspector_page.append(&inspector_lines);
        inspector_page.append(&inspector_findings);

        let inspector_action = Entry::new();
        inspector_action.set_widget_name("inspector-action");
        inspector_action.set_placeholder_text(Some("action token, e.g. caps"));
        let assign_remap = Button::with_label("Assign remap to the selected key");
        assign_remap.set_widget_name("assign-remap");
        inspector_page.append(&inspector_action);
        inspector_page.append(&assign_remap);

        let inspector_tap = Entry::new();
        inspector_tap.set_widget_name("inspector-tap");
        inspector_tap.set_placeholder_text(Some("tap action, e.g. caps"));
        let inspector_hold = Entry::new();
        inspector_hold.set_widget_name("inspector-hold");
        inspector_hold.set_placeholder_text(Some("hold action, e.g. esc"));
        let inspector_delay = Entry::new();
        inspector_delay.set_widget_name("inspector-delay");
        inspector_delay.set_placeholder_text(Some("delay ms, e.g. 250"));
        let assign_taphold = Button::with_label("Assign Tap-and-Hold");
        assign_taphold.set_widget_name("assign-taphold");
        inspector_page.append(&inspector_tap);
        inspector_page.append(&inspector_hold);
        inspector_page.append(&inspector_delay);
        inspector_page.append(&assign_taphold);

        let macro_body = Entry::new();
        macro_body.set_widget_name("macro-body");
        macro_body.set_placeholder_text(Some("macro line, e.g. {a}>{-lshf}{h}{+lshf}"));
        let assign_macro = Button::with_label("Assign macro (the trigger must be the position)");
        assign_macro.set_widget_name("assign-macro");
        inspector_page.append(&macro_body);
        inspector_page.append(&assign_macro);

        let remove_binding = Button::with_label("Remove custom binding (restore factory action)");
        remove_binding.set_widget_name("remove-binding");
        inspector_page.append(&remove_binding);

        // --- Catalogue -------------------------------------------------------------
        let catalogue_page = GtkBox::new(Orientation::Vertical, 6);
        catalogue_page.set_widget_name("catalogue-page");
        catalogue_page.set_margin_top(8);
        catalogue_page.set_margin_start(8);
        catalogue_page.set_margin_end(8);
        let catalogue_search = SearchEntry::new();
        catalogue_search.set_widget_name("action-search");
        catalogue_search.set_placeholder_text(Some("Search actions by name, token or usage id"));
        let mut category_labels = vec!["All categories".to_string()];
        category_labels.extend(TokenCategory::all().iter().map(|c| c.label().to_string()));
        let catalogue_category = dropdown(&category_labels);
        catalogue_category.set_widget_name("action-category");
        catalogue_category.set_selected(0);
        let catalogue_count = Label::new(None);
        catalogue_count.set_xalign(0.0);
        catalogue_count.set_widget_name("catalogue-count");
        catalogue_page.append(&catalogue_search);
        catalogue_page.append(&catalogue_category);
        catalogue_page.append(&catalogue_count);
        let catalogue_list = GtkBox::new(Orientation::Vertical, 2);
        catalogue_list.set_widget_name("action-catalogue");
        let catalogue_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .child(&catalogue_list)
            .build();
        catalogue_page.append(&catalogue_scroll);

        // --- Pending ---------------------------------------------------------------
        let pending_panel = GtkBox::new(Orientation::Vertical, 4);
        pending_panel.set_widget_name("pending-panel");
        pending_panel.set_margin_top(8);
        pending_panel.set_margin_start(8);
        pending_panel.set_margin_end(8);
        let pending_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .child(&pending_panel)
            .build();

        // --- Diff ------------------------------------------------------------------
        let diff_page = GtkBox::new(Orientation::Vertical, 6);
        let diff_files = GtkBox::new(Orientation::Vertical, 2);
        diff_files.set_widget_name("diff-file-list");
        let diff_view = TextView::new();
        diff_view.set_widget_name("diff-view");
        diff_view.set_editable(false);
        diff_view.set_monospace(true);
        let diff_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .child(&diff_view)
            .build();
        diff_page.append(&diff_files);
        diff_page.append(&diff_scroll);

        // --- Validation ------------------------------------------------------------
        let validation_panel = GtkBox::new(Orientation::Vertical, 4);
        validation_panel.set_widget_name("validation-panel");
        validation_panel.set_margin_top(8);
        validation_panel.set_margin_start(8);
        validation_panel.set_margin_end(8);
        let validation_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .child(&validation_panel)
            .build();

        // --- Presets ---------------------------------------------------------------
        let preset_page = GtkBox::new(Orientation::Vertical, 6);
        preset_page.set_margin_top(8);
        preset_page.set_margin_start(8);
        preset_page.set_margin_end(8);
        let preset_note = Label::new(Some(
            "Presets are drafts. Review the table and the exact lines, then apply them to the \
             draft with a separate action. Nothing is written to the keyboard here.",
        ));
        preset_note.set_xalign(0.0);
        preset_note.set_wrap(true);
        preset_page.append(&preset_note);
        let preset_buttons = GtkBox::new(Orientation::Horizontal, 6);
        let preset_apply_symbols = Button::with_label("Apply symbol draft to the draft");
        preset_apply_symbols.set_widget_name("preset-apply-symbols");
        let preset_apply_navigation = Button::with_label("Apply navigation draft to the draft");
        preset_apply_navigation.set_widget_name("preset-apply-navigation");
        preset_buttons.append(&preset_apply_symbols);
        preset_buttons.append(&preset_apply_navigation);
        preset_page.append(&preset_buttons);
        let access_row = GtkBox::new(Orientation::Horizontal, 6);
        let access_labels: Vec<String> = presets::build_draft()
            .access_key_options
            .iter()
            .map(|option| format!("{}  {}", option.keycap, option.line))
            .collect();
        let preset_access_combo = dropdown(&access_labels);
        preset_access_combo.set_widget_name("preset-access-key");
        preset_access_combo.set_selected(0);
        let preset_apply_access = Button::with_label("Apply selected access key to the draft");
        preset_apply_access.set_widget_name("preset-apply-access-key");
        access_row.append(&preset_access_combo);
        access_row.append(&preset_apply_access);
        preset_page.append(&access_row);
        let preset_review = TextView::new();
        preset_review.set_widget_name("preset-review");
        preset_review.set_editable(false);
        preset_review.set_monospace(true);
        let preset_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .child(&preset_review)
            .build();
        preset_page.append(&preset_scroll);

        // --- Layout / drafts -------------------------------------------------------
        let layout_page = GtkBox::new(Orientation::Vertical, 6);
        layout_page.set_margin_top(8);
        layout_page.set_margin_start(8);
        layout_page.set_margin_end(8);
        let layout_info = info_label("layout-info");
        layout_page.append(&layout_info);
        let override_row = GtkBox::new(Orientation::Horizontal, 4);
        let layout_override = Entry::new();
        layout_override.set_widget_name("layout-override");
        layout_override.set_placeholder_text(Some("XKB layout, e.g. us"));
        let layout_apply = Button::with_label("Set layout manually");
        layout_apply.set_widget_name("layout-apply");
        override_row.append(&layout_override);
        override_row.append(&layout_apply);
        layout_page.append(&override_row);

        let draft_note = Label::new(Some(
            "Export the draft to a folder, or import a previously exported draft. Neither \
             touches the keyboard.",
        ));
        draft_note.set_xalign(0.0);
        draft_note.set_wrap(true);
        layout_page.append(&draft_note);
        let export_entry = Entry::new();
        export_entry.set_widget_name("export-path");
        layout_page.append(&export_entry);
        let draft_row = GtkBox::new(Orientation::Horizontal, 6);
        let export_button = Button::with_label("Export draft");
        export_button.set_widget_name("export-button");
        let import_button = Button::with_label("Import draft");
        import_button.set_widget_name("import-button");
        draft_row.append(&export_button);
        draft_row.append(&import_button);
        layout_page.append(&draft_row);

        // Each page is added to the notebook exactly once, through the widget that actually owns
        // it. Adding a widget that already has a parent is what breaks the tree (GTK warns and the
        // page becomes unreachable), so the panels are reached through their scrolled windows.
        let pages: [(&str, gtk4::Widget); 6] = [
            ("Inspector", inspector_page.clone().upcast()),
            ("Catalogue", catalogue_page.clone().upcast()),
            ("Pending", pending_scroll.clone().upcast()),
            ("Layout & drafts", layout_page.clone().upcast()),
            ("Presets", preset_page.clone().upcast()),
            ("Validation", validation_scroll.clone().upcast()),
        ];
        for (title, child) in &pages {
            notebook.append_page(child, Some(&Label::new(Some(title))));
        }
        notebook.append_page(&diff_page, Some(&Label::new(Some("Diff"))));

        notebook.set_size_request(320, -1);
        // A paned split, not a box: the operator chooses how much room the keyboard map gets, and
        // it keeps a usable share even when the side panels want to be wide.
        let paned = Paned::new(Orientation::Horizontal);
        paned.set_widget_name("content-panes");
        paned.set_start_child(Some(&map_scroll));
        paned.set_end_child(Some(&notebook));
        // A starting split that leaves one whole module visible and the panels usable on a
        // window as narrow as 1000px; the operator can drag it.
        paned.set_position(660);
        paned.set_resize_start_child(true);
        paned.set_shrink_start_child(false);
        paned.set_vexpand(true);
        paned.set_hexpand(true);
        content.append(&paned);
        root.append(&content);

        let bottom = GtkBox::new(Orientation::Horizontal, 8);
        bottom.set_margin_top(6);
        bottom.set_margin_bottom(6);
        bottom.set_margin_start(6);
        bottom.set_margin_end(6);
        let undo_button = Button::with_label("Undo");
        undo_button.set_widget_name("undo-button");
        let redo_button = Button::with_label("Redo");
        redo_button.set_widget_name("redo-button");
        let backup_button = Button::with_label("Backup");
        backup_button.set_widget_name("backup-button");
        let write_button = Button::with_label("Write to keyboard");
        write_button.set_widget_name("write-button");
        write_button.add_css_class("destructive-action");
        let status_label = Label::new(Some("Ready."));
        status_label.set_widget_name("status-line");
        status_label.set_xalign(0.0);
        status_label.set_hexpand(true);
        bottom.append(&undo_button);
        bottom.append(&redo_button);
        bottom.append(&backup_button);
        bottom.append(&write_button);
        bottom.append(&status_label);
        root.append(&bottom);

        window.set_child(Some(&root));

        Widgets {
            window: window.clone(),
            header_device,
            banner,
            banner_label,
            mount_view,
            manual_path,
            open_folder,
            retry_button,
            profile_combo,
            layer_tabs,
            layer_buttons,
            key_well_left,
            key_well_right,
            key_well_pedal,
            inspector_page,
            inspector_current,
            inspector_factory,
            inspector_preview,
            inspector_lines,
            inspector_findings,
            inspector_action,
            assign_remap,
            inspector_tap,
            inspector_hold,
            inspector_delay,
            assign_taphold,
            macro_body,
            assign_macro,
            remove_binding,
            catalogue_search,
            catalogue_category,
            catalogue_count,
            catalogue_list,
            pending_panel,
            diff_files,
            diff_view,
            validation_panel,
            preset_review,
            preset_apply_symbols,
            preset_apply_navigation,
            preset_access_combo,
            preset_apply_access,
            layout_info,
            layout_override,
            layout_apply,
            export_entry,
            undo_button,
            redo_button,
            backup_button,
            export_button,
            import_button,
            write_button,
            about_button,
            status_label,
            spinner,
        }
    }
}

fn info_label(name: &str) -> Label {
    let label = Label::new(None);
    label.set_widget_name(name);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_selectable(true);
    label
}

fn default_export_dir() -> String {
    match std::env::var("HOME") {
        Ok(home) => format!("{home}/adv360-export"),
        Err(_) => "adv360-export".to_string(),
    }
}

/// A drop-down listing the given labels, with the first entry selected.
///
/// `ComboBoxText` was deprecated in GTK 4.10 in favour of `DropDown` with a `StringList` model, and
/// this crate targets 4.10 or newer, so the current pair is used throughout.
fn dropdown(labels: &[String]) -> DropDown {
    let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    DropDown::from_strings(&refs)
}

/// The selected index of a [`DropDown`], or `None` when nothing is selected.
///
/// GTK reports "no selection" as `u32::MAX`, so this is used instead of casting the raw value.
fn selected_index(dropdown: &DropDown) -> Option<u32> {
    let index = dropdown.selected();
    (index != u32::MAX).then_some(index)
}

// ---------------------------------------------------------------------------------------------
// Signal wiring
// ---------------------------------------------------------------------------------------------

fn wire(app: &SharedState, widgets: &Rc<Widgets>) {
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the field before connecting: the closure moves the `widgets` handle, so it must not
        // be borrowed by the receiver of the call.
        let profile_combo = widgets.profile_combo.clone();
        profile_combo.connect_selected_notify(move |combo| {
            let Some(index) = selected_index(combo) else {
                return;
            };
            let profile = ProfileId(index as u8);
            let changed = {
                let mut state = app.borrow_mut();
                if state.view.profile != profile {
                    state.view.profile = profile;
                    state.view.position = None;
                    true
                } else {
                    false
                }
            };
            if changed {
                refresh(&app, &widgets);
            }
        });
    }
    for (index, button) in widgets.layer_buttons.iter().enumerate() {
        let layer = LayerId::all()[index];
        let app = app.clone();
        let widgets = widgets.clone();
        button.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            let changed = {
                let mut state = app.borrow_mut();
                if state.view.layer != layer {
                    state.view.layer = layer;
                    state.view.position = None;
                    true
                } else {
                    false
                }
            };
            if changed {
                refresh(&app, &widgets);
            }
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let catalogue_search_w = widgets.catalogue_search.clone();
        catalogue_search_w.connect_search_changed(move |entry| {
            let text = entry.text().to_string();
            {
                let mut state = app.borrow_mut();
                state.view.search = text;
            }
            refresh(&app, &widgets);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let catalogue_category_w = widgets.catalogue_category.clone();
        catalogue_category_w.connect_selected_notify(move |combo| {
            let index = selected_index(combo).unwrap_or(0) as usize;
            let category = if index == 0 {
                None
            } else {
                TokenCategory::all().get(index - 1).copied()
            };
            let changed = {
                let mut state = app.borrow_mut();
                if state.view.category != category {
                    state.view.category = category;
                    true
                } else {
                    false
                }
            };
            if changed {
                refresh(&app, &widgets);
            }
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let undo_button_w = widgets.undo_button.clone();
        undo_button_w.connect_clicked(move |_| {
            {
                let mut state = app.borrow_mut();
                if let Some(session) = state.session.as_mut() {
                    session.undo();
                }
            }
            refresh(&app, &widgets);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let redo_button_w = widgets.redo_button.clone();
        redo_button_w.connect_clicked(move |_| {
            {
                let mut state = app.borrow_mut();
                if let Some(session) = state.session.as_mut() {
                    session.redo();
                }
            }
            refresh(&app, &widgets);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let backup_button_w = widgets.backup_button.clone();
        backup_button_w.connect_clicked(move |_| {
            let profile = app.borrow().view.profile;
            spawn_session_work(
                app.clone(),
                widgets.clone(),
                "Backing up the original files...",
                move |session| {
                    let set = session.backup(&[profile])?;
                    Ok(WorkOutput::Backup(set))
                },
            );
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let write_button_w = widgets.write_button.clone();
        write_button_w.connect_clicked(move |_| open_write_confirmation(&app, &widgets));
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let export_button_w = widgets.export_button.clone();
        export_button_w.connect_clicked(move |_| {
            let directory = PathBuf::from(widgets.export_entry.text().to_string());
            if directory.as_os_str().is_empty() {
                set_status(&app, &widgets, "Enter an export folder first");
                return;
            }
            spawn_session_work(
                app.clone(),
                widgets.clone(),
                "Exporting the draft...",
                move |session| {
                    let written = session.export_to(&directory)?;
                    Ok(WorkOutput::Exported(written))
                },
            );
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let import_button_w = widgets.import_button.clone();
        import_button_w.connect_clicked(move |_| {
            let directory = PathBuf::from(widgets.export_entry.text().to_string());
            if directory.as_os_str().is_empty() {
                set_status(&app, &widgets, "Enter an import folder first");
                return;
            }
            spawn_session_work(
                app.clone(),
                widgets.clone(),
                "Importing the draft...",
                move |session| {
                    let loaded = session.import_from(&directory)?;
                    Ok(WorkOutput::Imported(loaded))
                },
            );
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let layout_apply_w = widgets.layout_apply.clone();
        layout_apply_w.connect_clicked(move |_| {
            let text = widgets.layout_override.text().trim().to_string();
            if text.is_empty() {
                set_status(&app, &widgets, "Enter an XKB layout name first");
                return;
            }
            {
                let mut state = app.borrow_mut();
                if let Some(session) = state.session.as_mut() {
                    session.set_layout_override(text, None);
                }
            }
            refresh(&app, &widgets);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let about_button_w = widgets.about_button.clone();
        about_button_w.connect_clicked(move |_| {
            show_help(&widgets.window, &app);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let open_folder_w = widgets.open_folder.clone();
        open_folder_w.connect_clicked(move |_| {
            let path = PathBuf::from(widgets.manual_path.text().to_string());
            if path.as_os_str().is_empty() {
                set_status(&app, &widgets, "Enter the folder that holds the v-Drive");
                return;
            }
            open_session_at(&app, &widgets, path);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let retry_button_w = widgets.retry_button.clone();
        retry_button_w.connect_clicked(move |_| {
            let options = app.borrow().options.clone();
            match Session::open(options) {
                Ok(session) => {
                    {
                        let mut state = app.borrow_mut();
                        state.session = Some(session);
                        state.view = ViewState::default();
                    }
                    set_status(&app, &widgets, "v-Drive detected");
                    refresh(&app, &widgets);
                }
                Err(error) => show_error(&widgets.window, &error),
            }
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let assign_remap_w = widgets.assign_remap.clone();
        assign_remap_w.connect_clicked(move |_| {
            let action = widgets.inspector_action.text().trim().to_string();
            if action.is_empty() {
                set_status(&app, &widgets, "Type an action token first");
                return;
            }
            assign_remap(&app, &widgets, action);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let assign_taphold_w = widgets.assign_taphold.clone();
        assign_taphold_w.connect_clicked(move |_| assign_tap_hold(&app, &widgets));
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let assign_macro_w = widgets.assign_macro.clone();
        assign_macro_w.connect_clicked(move |_| assign_macro(&app, &widgets));
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let remove_binding_w = widgets.remove_binding.clone();
        remove_binding_w.connect_clicked(move |_| {
            let (profile, layer, position) = current_selection(&app);
            let Some(position) = position else {
                set_status(&app, &widgets, "Select a key first");
                return;
            };
            if !profile.is_editable() {
                show_info(
                    &widgets.window,
                    "Profile 0 is read-only",
                    "Profile 0 is non-programmable and cannot be edited.",
                );
                return;
            }
            let result = {
                let mut state = app.borrow_mut();
                let Some(session) = state.session.as_mut() else {
                    return;
                };
                session.set_binding(profile, layer, &position, None)
            };
            report_result(&app, &widgets, result);
            refresh(&app, &widgets);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let preset_apply_symbols_w = widgets.preset_apply_symbols.clone();
        preset_apply_symbols_w.connect_clicked(move |_| {
            apply_preset(&app, &widgets, PresetKind::Symbols);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let preset_apply_navigation_w = widgets.preset_apply_navigation.clone();
        preset_apply_navigation_w.connect_clicked(move |_| {
            apply_preset(&app, &widgets, PresetKind::Navigation);
        });
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        // Clone the receiver: the closure below moves the `widgets` handle.
        let preset_apply_access_w = widgets.preset_apply_access.clone();
        preset_apply_access_w.connect_clicked(move |_| {
            let index = selected_index(&widgets.preset_access_combo).unwrap_or(0) as usize;
            let profile = app.borrow().view.profile;
            if !profile.is_editable() {
                show_info(
                    &widgets.window,
                    "Profile 0 is read-only",
                    "Nothing is ever applied to profile 0.",
                );
                return;
            }
            let draft = presets::build_draft();
            let Some(option) = draft.access_key_options.get(index) else {
                return;
            };
            let result = {
                let mut state = app.borrow_mut();
                let Some(session) = state.session.as_mut() else {
                    return;
                };
                session.apply_access_key(profile, option)
            };
            report_result(
                &app,
                &widgets,
                result.map(|_| "access key applied to the draft".to_string()),
            );
            refresh(&app, &widgets);
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Refresh / apply view model
// ---------------------------------------------------------------------------------------------

fn refresh(app: &SharedState, widgets: &Rc<Widgets>) {
    let (view_model, status, busy) = {
        let state = app.borrow();
        let Some(session) = state.session.as_ref() else {
            return;
        };
        (
            build_view_model(session, &state.view),
            state.status.clone(),
            state.busy,
        )
    };
    apply_view_model(&view_model, app, widgets);
    widgets.status_label.set_text(&status);
    widgets.spinner.set_visible(busy);
    if busy {
        widgets.spinner.start();
    } else {
        widgets.spinner.stop();
    }
}

fn apply_view_model(view_model: &ViewModel, app: &SharedState, widgets: &Rc<Widgets>) {
    let (busy, diff_index) = {
        let state = app.borrow();
        (state.busy, state.view.diff_file)
    };

    widgets.header_device.set_text(&view_model.device.root);
    let show_banner = !view_model.device.available;
    widgets.banner.set_visible(show_banner);
    if show_banner {
        widgets.banner_label.set_text(
            "The ADV360 v-Drive is not available. Nothing can be written until it is mounted.",
        );
        let mut text = String::new();
        for line in &view_model.device.mount_instructions {
            text.push_str(line);
            text.push('\n');
        }
        widgets.mount_view.buffer().set_text(&text);
    }

    let profile_index = view_model
        .profiles
        .iter()
        .position(|p| p.profile == view_model.selected_profile)
        .unwrap_or(0) as u32;
    if selected_index(&widgets.profile_combo) != Some(profile_index) {
        widgets.profile_combo.set_selected(profile_index);
    }

    for (index, tab) in view_model.layers.iter().enumerate() {
        if let Some(button) = widgets.layer_buttons.get(index) {
            if button.is_active() != tab.selected {
                button.set_active(tab.selected);
            }
        }
    }

    clear(&widgets.key_well_left);
    append_key_block(
        &widgets.key_well_left,
        Half::Left,
        KeyBlock::Main,
        "Left module - main keywell",
        view_model,
        app,
        widgets,
    );
    append_key_block(
        &widgets.key_well_left,
        Half::Left,
        KeyBlock::Thumb,
        "Left thumb cluster",
        view_model,
        app,
        widgets,
    );
    clear(&widgets.key_well_right);
    append_key_block(
        &widgets.key_well_right,
        Half::Right,
        KeyBlock::Main,
        "Right module - main keywell",
        view_model,
        app,
        widgets,
    );
    append_key_block(
        &widgets.key_well_right,
        Half::Right,
        KeyBlock::Thumb,
        "Right thumb cluster",
        view_model,
        app,
        widgets,
    );
    clear(&widgets.key_well_pedal);
    append_key_block(
        &widgets.key_well_pedal,
        Half::Pedal,
        KeyBlock::Pedal,
        "FS360 foot pedal row",
        view_model,
        app,
        widgets,
    );

    apply_inspector(view_model, widgets, busy);
    apply_catalogue(view_model, app, widgets, busy);
    apply_pending(view_model, widgets);
    apply_diff(view_model, app, widgets, diff_index);
    apply_validation(view_model, widgets);
    apply_presets(view_model, widgets, busy);
    apply_layout(view_model, widgets);

    let editable = view_model.selected_profile.is_editable();
    widgets
        .undo_button
        .set_sensitive(!busy && view_model.can_undo);
    widgets
        .redo_button
        .set_sensitive(!busy && view_model.can_redo);
    widgets
        .backup_button
        .set_sensitive(!busy && editable && view_model.device.available);
    widgets
        .write_button
        .set_sensitive(!busy && view_model.write.can_write());
    widgets.write_button.set_label(&format!(
        "Write to keyboard ({} line(s))",
        view_model.write.changed_line_count
    ));
    widgets.export_button.set_sensitive(!busy && editable);
    widgets.import_button.set_sensitive(!busy && editable);
}

fn apply_inspector(view_model: &ViewModel, widgets: &Rc<Widgets>, busy: bool) {
    let editable =
        !busy && view_model.selected_profile.is_editable() && view_model.device.available;
    match &view_model.inspector {
        Some(inspector) => {
            widgets.inspector_current.set_text(&format!(
                "Current function: {} {}",
                inspector.current,
                if inspector.custom {
                    "(custom)"
                } else {
                    "(factory action)"
                }
            ));
            widgets.inspector_factory.set_text(&format!(
                "Factory action: [{}]{} - {}",
                inspector.factory_action,
                if inspector.factory_uncertain {
                    " (UNCERTAIN)"
                } else {
                    ""
                },
                inspector.factory_source
            ));
            let preview = match &inspector.preview {
                Some(p) => {
                    let mut text = format!("Sends token: {} ({})", p.token, p.usb_label);
                    if let Some(character) = &p.character {
                        text.push_str(&format!("  produces '{character}'"));
                    }
                    if let Some(shifted) = &p.shifted {
                        text.push_str(&format!("  shifted '{shifted}'"));
                    }
                    if !p.confident {
                        text.push_str("\nWARNING: the character preview is uncertain.");
                    }
                    if let Some(note) = &p.note {
                        text.push('\n');
                        text.push_str(note);
                    }
                    text
                }
                None => {
                    "No single-character preview: a macro sends a sequence of actions; read the \
                     exact line in the diff view."
                        .to_string()
                }
            };
            widgets.inspector_preview.set_text(&preview);
            widgets.inspector_lines.set_text(&format!(
                "Configuration line(s) in this layer: {:?}",
                inspector.lines
            ));
            let findings: Vec<String> = inspector.findings.iter().map(|f| f.one_line()).collect();
            widgets
                .inspector_findings
                .set_text(&if findings.is_empty() {
                    "No validation findings for this key.".to_string()
                } else {
                    findings.join("\n")
                });
            if widgets.inspector_action.text().is_empty() {
                widgets.inspector_action.set_text(&inspector.factory_action);
            }
        }
        None => {
            widgets
                .inspector_current
                .set_text("Select a key on either module to inspect it.");
            widgets.inspector_factory.set_text("");
            widgets.inspector_preview.set_text("");
            widgets.inspector_lines.set_text("");
            widgets.inspector_findings.set_text("");
        }
    }
    let has_selection = view_model.selected_position.is_some();
    let allow = editable && has_selection;
    widgets.assign_remap.set_sensitive(allow);
    widgets.assign_taphold.set_sensitive(allow);
    widgets.assign_macro.set_sensitive(allow);
    widgets
        .remove_binding
        .set_sensitive(allow && view_model.inspector.as_ref().is_some_and(|i| i.custom));
    widgets.inspector_action.set_sensitive(allow);
    widgets.inspector_tap.set_sensitive(allow);
    widgets.inspector_hold.set_sensitive(allow);
    widgets.inspector_delay.set_sensitive(allow);
    widgets.macro_body.set_sensitive(allow);
}

fn apply_catalogue(view_model: &ViewModel, app: &SharedState, widgets: &Rc<Widgets>, busy: bool) {
    let active_index = view_model
        .catalogue
        .selected_category
        .as_ref()
        .and_then(|label| {
            TokenCategory::all()
                .iter()
                .position(|c| c.label() == label)
                .map(|i| i as u32 + 1)
        })
        .unwrap_or(0);
    if selected_index(&widgets.catalogue_category) != Some(active_index) {
        widgets.catalogue_category.set_selected(active_index);
    }
    widgets.catalogue_count.set_text(&format!(
        "{} of {} documented action tokens",
        view_model.catalogue.total_matches,
        tokens::TOKENS.len()
    ));
    clear(&widgets.catalogue_list);
    let allow = !busy
        && view_model.selected_profile.is_editable()
        && view_model.selected_position.is_some();
    if view_model.catalogue.entries.is_empty() {
        widgets
            .catalogue_list
            .append(&Label::new(Some("No action token matches the search.")));
        return;
    }
    for entry in view_model.catalogue.entries.iter().take(400) {
        let button = Button::new();
        button.set_widget_name(&format!("catalogue-{}", entry.token));
        let label = Label::new(Some(&format!("{} - {}", entry.token, entry.name)));
        label.set_xalign(0.0);
        button.set_child(Some(&label));
        button.set_halign(Align::Start);
        button.set_sensitive(allow);
        button.set_tooltip_text(Some(&format!(
            "{} | {} | {}",
            entry.category, entry.group, entry.tap_hold
        )));
        let action = entry.token.clone();
        let app = app.clone();
        let widgets = widgets.clone();
        // Keep a separate handle for the container: the closure below moves `widgets`.
        let catalogue_list = widgets.catalogue_list.clone();
        button.connect_clicked(move |_| assign_remap(&app, &widgets, action.clone()));
        catalogue_list.append(&button);
    }
}

fn apply_pending(view_model: &ViewModel, widgets: &Rc<Widgets>) {
    clear(&widgets.pending_panel);
    if view_model.pending.is_empty() {
        widgets
            .pending_panel
            .append(&Label::new(Some("No pending changes.")));
        return;
    }
    for change in &view_model.pending {
        let label = Label::new(Some(&format!("{}  [{}]", change.description, change.file)));
        label.set_xalign(0.0);
        label.set_wrap(true);
        widgets.pending_panel.append(&label);
    }
}

fn apply_diff(view_model: &ViewModel, app: &SharedState, widgets: &Rc<Widgets>, diff_index: usize) {
    clear(&widgets.diff_files);
    if view_model.diffs.is_empty() {
        widgets
            .diff_view
            .buffer()
            .set_text("No files in this session.");
        return;
    }
    for (index, diff) in view_model.diffs.iter().enumerate() {
        let mut label = diff.relative_path.clone();
        if diff.identical {
            label.push_str("  (identical)");
        } else {
            label.push_str(&format!("  ({} changed line(s))", diff.changed_lines.len()));
        }
        let button = Button::with_label(&label);
        button.set_widget_name(&format!("diff-file-{index}"));
        button.set_halign(Align::Start);
        if index == diff_index {
            button.add_css_class("accent");
        }
        let app = app.clone();
        let widgets = widgets.clone();
        // Keep a separate handle for the container: the closure below moves `widgets`.
        let diff_files = widgets.diff_files.clone();
        button.connect_clicked(move |_| {
            {
                let mut state = app.borrow_mut();
                state.view.diff_file = index;
            }
            refresh(&app, &widgets);
        });
        diff_files.append(&button);
    }
    let selected = view_model
        .diffs
        .get(diff_index)
        .or_else(|| view_model.diffs.first());
    let text = match selected {
        Some(diff) if diff.unified.is_empty() => {
            format!("{}: no differences from the v-Drive.", diff.relative_path)
        }
        Some(diff) => diff.unified.clone(),
        None => String::new(),
    };
    widgets.diff_view.buffer().set_text(&text);
}

fn apply_validation(view_model: &ViewModel, widgets: &Rc<Widgets>) {
    clear(&widgets.validation_panel);
    if view_model.validation.is_empty() {
        widgets
            .validation_panel
            .append(&Label::new(Some("No validation findings.")));
        return;
    }
    for finding in &view_model.validation {
        let label = Label::new(Some(&finding.one_line()));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_selectable(true);
        widgets.validation_panel.append(&label);
    }
}

fn apply_presets(view_model: &ViewModel, widgets: &Rc<Widgets>, busy: bool) {
    let mut text = String::new();
    for table in [&view_model.presets.symbols, &view_model.presets.navigation] {
        text.push_str(&format!("== {} ({})\n", table.title, table.layer_header));
        text.push_str(&format!("{}\n\n", table.rationale));
        for row in &table.rows {
            text.push_str(&format!(
                "  {} ({}) -> {}    {}\n",
                row.position, row.keycap, row.produces, row.line
            ));
        }
        if !table.caveats.is_empty() {
            text.push_str("  Things to check:\n");
            for caveat in &table.caveats {
                text.push_str(&format!("    - {caveat}\n"));
            }
        }
        text.push('\n');
    }
    text.push_str("Making the navigation layer reachable (applied under the Base layer):\n");
    for option in &view_model.presets.access_keys {
        text.push_str(&format!(
            "  {}    {}    cost: {}\n",
            option.keycap,
            option.line,
            option.cost.clone().unwrap_or_else(|| "none".to_string())
        ));
    }
    text.push_str("\nExact lines of the complete draft layout file:\n\n");
    text.push_str(&view_model.presets.exact_lines);
    widgets.preset_review.buffer().set_text(&text);

    let allow = !busy && view_model.selected_profile.is_editable();
    widgets.preset_apply_symbols.set_sensitive(allow);
    widgets.preset_apply_navigation.set_sensitive(allow);
    widgets.preset_apply_access.set_sensitive(allow);
    widgets.preset_access_combo.set_sensitive(allow);
}

fn apply_layout(view_model: &ViewModel, widgets: &Rc<Widgets>) {
    let mut text = format!(
        "Active keyboard layout: {} ({})",
        view_model.layout.layout, view_model.layout.source
    );
    if let Some(variant) = &view_model.layout.variant {
        text.push_str(&format!(", variant '{variant}'"));
    }
    text.push_str(&format!("\n{}", view_model.layout.detail));
    text.push_str(&format!(
        "\nLayouts with an encoded character mapping in this build: {}",
        view_model.layout.supported.join(", ")
    ));
    if let Some(warning) = &view_model.layout.warning {
        text.push_str(&format!("\n\nWARNING: {warning}"));
    } else {
        text.push_str("\nCharacters are computed from the encoded 'us' mapping.");
    }
    widgets.layout_info.set_text(&text);
}

// ---------------------------------------------------------------------------------------------
// Key map rendering
// ---------------------------------------------------------------------------------------------

fn append_key_block(
    container: &GtkBox,
    half: Half,
    block: KeyBlock,
    title: &str,
    view_model: &ViewModel,
    app: &SharedState,
    widgets: &Rc<Widgets>,
) {
    let frame = Frame::new(Some(title));
    frame.set_widget_name(&format!(
        "key-block-{}-{}",
        half_name(half),
        block_name(block)
    ));
    let grid = Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(4);
    for cell in render_key_map(half, block, view_model) {
        let button = key_button(&cell, app, widgets);
        grid.attach(
            &button,
            cell.position.x as i32,
            cell.position.y as i32,
            cell.position.w as i32,
            cell.position.h as i32,
        );
    }
    frame.set_child(Some(&grid));
    container.append(&frame);
}

fn key_button(cell: &KeyCell, app: &SharedState, widgets: &Rc<Widgets>) -> Button {
    let button = Button::new();
    button.set_widget_name(&format!("key-{}", cell.position.token));
    // A fixed cell size keeps a seven-column module at a predictable width; the labels ellipsise
    // rather than forcing the module wider.
    button.set_size_request(62, 42);
    let content = GtkBox::new(Orientation::Vertical, 0);
    let label = Label::new(Some(&cell.label));
    label.set_max_width_chars(9);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    let sub = Label::new(Some(&cell.sub_label));
    sub.set_max_width_chars(10);
    sub.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    sub.add_css_class("dim-label");
    content.append(&label);
    content.append(&sub);
    if let Some(severity) = cell.worst_finding {
        let marker = Label::new(Some(severity_marker(severity)));
        marker.add_css_class("error");
        content.append(&marker);
    }
    button.set_child(Some(&content));
    button.set_tooltip_text(Some(&format!(
        "{} - {}",
        cell.position.map_label, cell.sub_label
    )));
    if cell.custom {
        button.add_css_class("suggested-action");
    }
    if cell.selected {
        button.add_css_class("accent");
    }
    if !cell.position.programmable {
        button.set_sensitive(false);
        button.set_tooltip_text(Some(cell.position.note));
    }
    let token = cell.position.token.to_string();
    let app = app.clone();
    let widgets = widgets.clone();
    button.connect_clicked(move |_| {
        {
            let mut state = app.borrow_mut();
            state.view.position = Some(token.clone());
        }
        refresh(&app, &widgets);
    });
    button
}

fn half_name(half: Half) -> &'static str {
    match half {
        Half::Left => "left",
        Half::Right => "right",
        Half::Pedal => "pedal",
    }
}

fn block_name(block: KeyBlock) -> &'static str {
    match block {
        KeyBlock::Main => "main",
        KeyBlock::Thumb => "thumb",
        KeyBlock::Pedal => "pedal",
    }
}

fn severity_marker(severity: adv360_core::validate::Severity) -> &'static str {
    match severity {
        adv360_core::validate::Severity::Error => "! error",
        adv360_core::validate::Severity::Warning => "! warning",
        adv360_core::validate::Severity::Info => "i info",
    }
}

fn clear(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        child.unparent();
    }
}

// ---------------------------------------------------------------------------------------------
// Operator actions
// ---------------------------------------------------------------------------------------------

fn current_selection(app: &SharedState) -> (ProfileId, LayerId, Option<String>) {
    let state = app.borrow();
    (
        state.view.profile,
        state.view.layer,
        state.view.position.clone(),
    )
}

fn assign_remap(app: &SharedState, widgets: &Rc<Widgets>, action: String) {
    let (profile, layer, position) = current_selection(app);
    let Some(position) = position else {
        set_status(app, widgets, "Select a key first");
        return;
    };
    if !profile.is_editable() {
        show_info(
            &widgets.window,
            "Profile 0 is read-only",
            "Profile 0 is non-programmable and has no layout file, so nothing can be assigned to \
             it. Choose profile 1-9.",
        );
        return;
    }
    let result = {
        let mut state = app.borrow_mut();
        let Some(session) = state.session.as_mut() else {
            return;
        };
        session.set_binding(
            profile,
            layer,
            &position,
            Some(Assignment::Remap {
                position: position.clone(),
                action,
            }),
        )
    };
    report_result(app, widgets, result);
    refresh(app, widgets);
}

fn assign_tap_hold(app: &SharedState, widgets: &Rc<Widgets>) {
    let (profile, layer, position) = current_selection(app);
    let Some(position) = position else {
        set_status(app, widgets, "Select a key first");
        return;
    };
    if !profile.is_editable() {
        show_info(
            &widgets.window,
            "Profile 0 is read-only",
            "Profile 0 is non-programmable and cannot be edited.",
        );
        return;
    }
    let tap = widgets.inspector_tap.text().trim().to_string();
    let hold = widgets.inspector_hold.text().trim().to_string();
    let delay_text = widgets.inspector_delay.text().trim().to_string();
    if tap.is_empty() || hold.is_empty() {
        show_info(
            &widgets.window,
            "Tap-and-Hold needs both halves",
            "Enter a tap action and a hold action, for example caps and esc.",
        );
        return;
    }
    let delay_ms: u16 = match delay_text.parse() {
        Ok(value) => value,
        Err(_) => {
            show_info(
                &widgets.window,
                "Invalid delay",
                "The delay must be a whole number of milliseconds, for example 250.",
            );
            return;
        }
    };
    let result = {
        let mut state = app.borrow_mut();
        let Some(session) = state.session.as_mut() else {
            return;
        };
        session.set_binding(
            profile,
            layer,
            &position,
            Some(Assignment::TapAndHold {
                position: position.clone(),
                tap,
                delay_ms,
                hold,
            }),
        )
    };
    report_result(app, widgets, result);
    refresh(app, widgets);
}

fn assign_macro(app: &SharedState, widgets: &Rc<Widgets>) {
    let (profile, layer, position) = current_selection(app);
    let Some(position) = position else {
        set_status(app, widgets, "Select a key first");
        return;
    };
    if !profile.is_editable() {
        show_info(
            &widgets.window,
            "Profile 0 is read-only",
            "Profile 0 is non-programmable and cannot be edited.",
        );
        return;
    }
    let body = widgets.macro_body.text().trim().to_string();
    if body.is_empty() {
        show_info(
            &widgets.window,
            "Empty macro",
            "Enter a macro line such as {a}>{-lshf}{h}{+lshf}.",
        );
        return;
    }
    // The core parser is the only parser: the interface never interprets the syntax itself.
    let parsed = adv360_core::parser::parse_line(body, Eol::Lf, Some(layer));
    let LineKind::Macro(macro_line) = parsed.kind else {
        show_info(
            &widgets.window,
            "Not a macro line",
            "That text does not parse as a documented macro. Example: {tab}>{-lshf}{h}{+lshf}.",
        );
        return;
    };
    let result = {
        let mut state = app.borrow_mut();
        let Some(session) = state.session.as_mut() else {
            return;
        };
        session.set_binding(
            profile,
            layer,
            &position,
            Some(Assignment::Macro {
                position: position.clone(),
                macro_line,
            }),
        )
    };
    report_result(app, widgets, result);
    refresh(app, widgets);
}

enum PresetKind {
    Symbols,
    Navigation,
}

fn apply_preset(app: &SharedState, widgets: &Rc<Widgets>, kind: PresetKind) {
    let profile = app.borrow().view.profile;
    if !profile.is_editable() {
        show_info(
            &widgets.window,
            "Profile 0 is read-only",
            "Presets are never applied to profile 0.",
        );
        return;
    }
    let draft = presets::build_draft();
    let result = {
        let mut state = app.borrow_mut();
        let Some(session) = state.session.as_mut() else {
            return;
        };
        match kind {
            PresetKind::Symbols => session.apply_preset(profile, &draft.symbols),
            PresetKind::Navigation => session.apply_preset(profile, &draft.navigation),
        }
    };
    match result {
        Ok(count) => set_status(
            app,
            widgets,
            &format!("Applied {count} preset assignment(s) to the draft; review the diff."),
        ),
        Err(error) => show_error(&widgets.window, &error),
    }
    refresh(app, widgets);
}

fn open_session_at(app: &SharedState, widgets: &Rc<Widgets>, root: PathBuf) {
    let options = app.borrow().options.clone();
    match Session::open_at(&root, options) {
        Ok(session) => {
            {
                let mut state = app.borrow_mut();
                state.session = Some(session);
                state.view = ViewState::default();
            }
            set_status(app, widgets, "v-Drive opened");
            refresh(app, widgets);
        }
        Err(error) => show_error(&widgets.window, &error),
    }
}

fn report_result<T>(app: &SharedState, widgets: &Rc<Widgets>, result: CoreResult<T>)
where
    T: std::fmt::Display,
{
    match result {
        Ok(value) => set_status(app, widgets, &value.to_string()),
        Err(error) => show_error(&widgets.window, &error),
    }
}

fn set_status(app: &SharedState, widgets: &Rc<Widgets>, message: &str) {
    app.borrow_mut().status = message.to_string();
    widgets.status_label.set_text(message);
}

fn set_busy(app: &SharedState, widgets: &Rc<Widgets>, busy: bool, message: &str) {
    {
        let mut state = app.borrow_mut();
        state.busy = busy;
        state.status = message.to_string();
    }
    widgets.status_label.set_text(message);
    widgets.spinner.set_visible(busy);
    if busy {
        widgets.spinner.start();
    } else {
        widgets.spinner.stop();
    }
    let buttons = [
        &widgets.undo_button,
        &widgets.redo_button,
        &widgets.backup_button,
        &widgets.export_button,
        &widgets.import_button,
        &widgets.write_button,
        &widgets.preset_apply_symbols,
        &widgets.preset_apply_navigation,
        &widgets.preset_apply_access,
    ];
    for button in buttons {
        button.set_sensitive(!busy);
    }
}

// ---------------------------------------------------------------------------------------------
// Worker thread integration
// ---------------------------------------------------------------------------------------------

fn spawn_session_work<F>(app: SharedState, widgets: Rc<Widgets>, label: &str, work: F)
where
    F: FnOnce(&mut Session) -> CoreResult<WorkOutput> + Send + 'static,
{
    let session = app.borrow_mut().session.take();
    let Some(mut session) = session else {
        return;
    };
    set_busy(&app, &widgets, true, label);
    let (sender, receiver) = mpsc::channel::<(Session, CoreResult<WorkOutput>)>();
    let _handle = std::thread::spawn(move || {
        let result = work(&mut session);
        let _ = sender.send((session, result));
    });
    let app = app.clone();
    let widgets = widgets.clone();
    let _ = gtk4::glib::timeout_add_local(Duration::from_millis(80), move || {
        match receiver.try_recv() {
            Ok((session, result)) => {
                {
                    let mut state = app.borrow_mut();
                    state.session = Some(session);
                    state.busy = false;
                }
                match result {
                    Ok(output) => handle_work_output(&app, &widgets, output),
                    Err(error) => {
                        set_status(&app, &widgets, "Operation failed; nothing was written");
                        show_error(&widgets.window, &error);
                    }
                }
                refresh(&app, &widgets);
                gtk4::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                {
                    let mut state = app.borrow_mut();
                    state.busy = false;
                    state.status =
                        "The worker stopped unexpectedly; no change was made.".to_string();
                }
                refresh(&app, &widgets);
                gtk4::glib::ControlFlow::Break
            }
        }
    });
}

fn handle_work_output(app: &SharedState, widgets: &Rc<Widgets>, output: WorkOutput) {
    match output {
        WorkOutput::Backup(set) => {
            let message = format!(
                "Backup complete: {} ({} file(s), verified)",
                set.directory.display(),
                set.files.len()
            );
            set_status(app, widgets, &message);
            show_info(&widgets.window, "Backup complete", &message);
        }
        WorkOutput::Written(report) => {
            set_status(
                app,
                widgets,
                &format!("Write complete: {}", report.relative_path),
            );
            show_write_report(&widgets.window, &report);
        }
        WorkOutput::Exported(paths) => {
            let mut body = format!("Wrote {} file(s):\n", paths.len());
            for path in &paths {
                body.push_str(&format!("  {}\n", path.display()));
            }
            set_status(app, widgets, "Draft exported; the v-Drive was not touched");
            show_info(&widgets.window, "Export complete", &body);
        }
        WorkOutput::Imported(files) => {
            let mut body = format!("Imported {} file(s):\n", files.len());
            for file in &files {
                body.push_str(&format!("  {file}\n"));
            }
            set_status(
                app,
                widgets,
                "Draft imported; review the diff before writing",
            );
            show_info(&widgets.window, "Import complete", &body);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------------------------

fn show_info(parent: &ApplicationWindow, title: &str, body: &str) {
    show_message(parent, title, body);
}

fn show_error(parent: &ApplicationWindow, error: &CoreError) {
    let mut body = error.to_string();
    let hints = error.recovery_hints();
    if !hints.is_empty() {
        body.push_str("\n\nRecovery steps:");
        for hint in &hints {
            body.push_str(&format!("\n  - {hint}"));
        }
    }
    show_message(parent, "Operation failed", &body);
}

/// A modal message window built from plain widgets, so no deprecated dialog API is involved.
/// Every error path shows the message plus `CoreError::recovery_hints()`.
fn show_message(parent: &ApplicationWindow, title: &str, body: &str) {
    let window = Window::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .default_width(700)
        .default_height(520)
        .build();
    let content = GtkBox::new(Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let heading = Label::new(Some(title));
    heading.set_xalign(0.0);
    content.append(&heading);
    let text_view = TextView::new();
    text_view.set_editable(false);
    text_view.set_monospace(true);
    text_view.buffer().set_text(body);
    let scroll = ScrolledWindow::builder()
        .vexpand(true)
        .child(&text_view)
        .build();
    content.append(&scroll);
    let close = Button::with_label("Close");
    close.set_halign(Align::End);
    let handle = window.clone();
    close.connect_clicked(move |_| handle.close());
    content.append(&close);
    window.set_child(Some(&content));
    window.present();
}

fn show_write_report(parent: &ApplicationWindow, report: &WriteReport) {
    let mut body = format!("Target file: {}\n", report.relative_path);
    body.push_str(&format!(
        "Verification: {}\n",
        if report.verified {
            "read back from disk and verified byte-for-byte"
        } else {
            "NOT VERIFIED - restore the backup"
        }
    ));
    body.push_str(&format!("Bytes written: {}\n", report.bytes_written));
    body.push_str(&format!("Changed lines: {:?}\n", report.changed_lines));
    body.push_str(&format!(
        "\nBackup taken before the write:\n  {}\n",
        report.backup_directory.display()
    ));
    for file in &report.backup_files {
        body.push_str(&format!("    {file}\n"));
    }
    body.push_str("\nNext steps:\n");
    for step in &report.next_steps {
        body.push_str(&format!("  - {step}\n"));
    }
    body.push_str("\nEject instructions:\n");
    for line in &report.eject_instructions {
        body.push_str(&format!("  {line}\n"));
    }
    show_info(parent, "Write complete", &body);
}

fn show_help(parent: &ApplicationWindow, app: &SharedState) {
    let window = Window::builder()
        .title("About / Help")
        .transient_for(parent)
        .modal(true)
        .default_width(720)
        .default_height(620)
        .build();
    let content = GtkBox::new(Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let mut text = format!("{APP_NAME} {VERSION}\n\n");
    text.push_str("Documentation this build was written against:\n");
    for (name, detail) in SOURCES {
        text.push_str(&format!("  - {name}\n      {detail}\n"));
    }
    text.push_str("\nHow to use this window:\n");
    text.push_str("  - Click a key on either module to inspect and edit it.\n");
    text.push_str("  - Pick a profile 1-9 and one of the five layers.\n");
    text.push_str(
        "  - Search the action catalogue and assign a plain remap to the selected key.\n",
    );
    text.push_str(
        "  - Presets are drafts: review the table and the exact lines, then apply them.\n",
    );
    text.push_str("  - Back up before writing; the write dialog lists every changed line.\n");
    text.push_str("  - Eject the v-Drive from the computer before closing it from the keyboard.\n");
    {
        let state = app.borrow();
        if let Some(session) = state.session.as_ref() {
            text.push_str(&format!("\nDevice: {}\n", session.device().root.display()));
            text.push_str(&format!(
                "Keyboard layout: {} ({})\n",
                session.layout().layout,
                session.layout().source.label()
            ));
            if let Some(warning) = session.layout().reliability_warning() {
                text.push_str(&format!("\nWARNING: {warning}\n"));
            }
        } else {
            text.push_str("\nNo v-Drive is loaded.\n");
        }
    }
    let text_view = TextView::new();
    text_view.set_editable(false);
    text_view.set_monospace(true);
    text_view.buffer().set_text(&text);
    let scroll = ScrolledWindow::builder()
        .vexpand(true)
        .child(&text_view)
        .build();
    content.append(&scroll);
    let close = Button::with_label("Close");
    close.set_halign(Align::End);
    let handle = window.clone();
    close.connect_clicked(move |_| handle.close());
    content.append(&close);
    window.set_child(Some(&content));
    window.present();
}

fn open_write_confirmation(app: &SharedState, widgets: &Rc<Widgets>) {
    let preview = {
        let state = app.borrow();
        let Some(session) = state.session.as_ref() else {
            return;
        };
        build_view_model(session, &state.view).write
    };
    if !preview.editable {
        show_info(
            &widgets.window,
            "Profile 0 is read-only",
            "Profile 0 is non-programmable and has no layout file, so it can never be edited or \
             written. Choose profile 1-9.",
        );
        return;
    }
    if !preview.has_file {
        show_info(
            &widgets.window,
            "No layout file",
            &format!(
                "{} has no layout file on the v-Drive, so there is nothing to write.",
                preview.profile
            ),
        );
        return;
    }
    if preview.changed_line_count == 0 {
        show_info(
            &widgets.window,
            "No changes",
            "The draft is identical to what is already on the v-Drive.",
        );
        return;
    }
    if preview.blocks_write {
        show_error(
            &widgets.window,
            &CoreError::ValidationFailed {
                errors: preview.validation_errors,
                first: "see the validation panel for the exact lines".to_string(),
            },
        );
        return;
    }

    let mut body = format!("Target file: {}\n", preview.target_display());
    body.push_str(&format!(
        "Changed line numbers ({}): {:?}\n\n",
        preview.changed_line_count, preview.changed_lines
    ));
    body.push_str("Pending changes:\n");
    if preview.pending.is_empty() {
        body.push_str("  (the file changed outside the key map; see the diff view)\n");
    }
    for change in &preview.pending {
        body.push_str(&format!("  - {} [{}]\n", change.description, change.file));
    }
    body.push_str(&format!(
        "\nBackup: a dated, verified copy of the original will be written under\n  {} (scope {})\n",
        preview.backup_root, preview.backup_scope
    ));
    body.push_str("\nEject instructions (run these after the write):\n");
    for line in &preview.eject_instructions {
        body.push_str(&format!("  {line}\n"));
    }

    let confirm_window = Window::builder()
        .title("Confirm write to the v-Drive")
        .transient_for(&widgets.window)
        .modal(true)
        .default_width(800)
        .default_height(660)
        .build();
    let content = GtkBox::new(Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let title = Label::new(Some("Review before writing to the keyboard"));
    title.set_xalign(0.0);
    content.append(&title);
    let text_view = TextView::new();
    text_view.set_editable(false);
    text_view.set_monospace(true);
    text_view.set_widget_name("write-preview");
    text_view.buffer().set_text(&body);
    let scroll = ScrolledWindow::builder()
        .vexpand(true)
        .child(&text_view)
        .build();
    content.append(&scroll);
    let checkbox = CheckButton::with_label(
        "I have read the target, the changed lines, the pending changes, the backup and the eject \
         instructions, and I authorise this write.",
    );
    checkbox.set_widget_name("confirm-write-checkbox");
    checkbox.set_active(false);
    content.append(&checkbox);
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_halign(Align::End);
    let cancel = Button::with_label("Cancel");
    let confirm = Button::with_label("Write to the v-Drive");
    confirm.set_widget_name("confirm-write-button");
    confirm.add_css_class("destructive-action");
    confirm.set_sensitive(false);
    row.append(&cancel);
    row.append(&confirm);
    content.append(&row);
    confirm_window.set_child(Some(&content));

    {
        let confirm = confirm.clone();
        checkbox.connect_toggled(move |checkbox| confirm.set_sensitive(checkbox.is_active()));
    }
    {
        let handle = confirm_window.clone();
        cancel.connect_clicked(move |_| handle.close());
    }
    {
        let app = app.clone();
        let widgets = widgets.clone();
        let handle = confirm_window.clone();
        let profile = preview.profile;
        let acknowledged = preview.changed_line_count;
        confirm.connect_clicked(move |_| {
            handle.close();
            spawn_session_work(
                app.clone(),
                widgets.clone(),
                "Backing up, then writing and verifying...",
                move |session| {
                    let current = session
                        .profile_file_index(profile)
                        .map_or(0, |index| session.file_change_count(index));
                    if current != acknowledged {
                        return Err(CoreError::RefusedWrite {
                            reason: format!(
                                "the file now has {current} changed line(s) but {acknowledged} \
                                 were displayed when you confirmed; re-open the write dialog"
                            ),
                        });
                    }
                    session.backup(&[profile])?;
                    let report = session.write_profile(WriteConfirmation {
                        profile,
                        confirmed_by_operator: true,
                        acknowledged_changed_lines: acknowledged,
                    })?;
                    Ok(WorkOutput::Written(Box::new(report)))
                },
            );
        });
    }
    confirm_window.present();
}

fn find_by_name<W: IsA<gtk4::Widget>>(root: &W, name: &str) -> Option<gtk4::Widget> {
    if root.widget_name().as_str() == name {
        return Some(root.upcast_ref::<gtk4::Widget>().clone());
    }
    let mut child = root.first_child();
    while let Some(current) = child {
        if let Some(found) = find_by_name(&current, name) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}
