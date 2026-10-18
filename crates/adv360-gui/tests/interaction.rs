//! Interaction tests for the GTK4 widget layer.
//!
//! These drive the real widget tree — clicking key buttons, typing into the action field, pressing
//! assign and undo, and opening the write confirmation — so the claim "the GUI is not a mock-up"
//! is backed by observed widget state rather than by reading the source.
//!
//! GTK is not thread-safe and Rust runs test functions on separate threads, so everything lives in
//! one `#[test]`. When no display is available the test prints why and returns, which keeps the
//! suite green on a headless machine.

mod common;

use adv360_gui::Ui;
use gtk4::prelude::*;

/// Find a widget by its stable name anywhere in the window.
fn widget<T: IsA<gtk4::Widget>>(ui: &Ui, name: &str) -> T {
    let found = ui
        .find_widget(name)
        .unwrap_or_else(|| panic!("the window must contain a widget named '{name}'"));
    found
        .downcast::<T>()
        .unwrap_or_else(|_| panic!("'{name}' has an unexpected widget type"))
}

/// Find a widget by name across every open window, which is how a dialog is reached.
fn in_dialogs<T: IsA<gtk4::Widget>>(name: &str) -> Option<T> {
    for window in gtk4::Window::list_toplevels() {
        if let Some(found) = find_by_name(&window, name) {
            return found.downcast::<T>().ok();
        }
    }
    None
}

fn find_by_name(root: &gtk4::Widget, name: &str) -> Option<gtk4::Widget> {
    if root.widget_name() == name {
        return Some(root.clone());
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

fn click(ui: &Ui, name: &str) {
    widget::<gtk4::Button>(ui, name).emit_clicked();
}

fn text_of(buffer: &gtk4::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

#[test]
fn the_widget_layer_responds_to_real_interactions() {
    if let Err(error) = gtk4::init() {
        eprintln!(
            "skipping the widget interaction test: no display available (gtk4::init failed: \
             {error}). Run under a display or gtk4-broadwayd to exercise the widget layer."
        );
        return;
    }

    let drive = common::TempDrive::new("gui-interaction");
    let session = common::open(&drive);
    let ui = adv360_gui::build_ui(session);

    // ---------------------------------------------------------------------------------------
    // A key can be selected with the mouse and appears in the inspector.
    // ---------------------------------------------------------------------------------------
    let inspector_current = widget::<gtk4::Label>(&ui, "inspector-current");
    assert!(
        inspector_current.text().contains("Select a key"),
        "the inspector starts empty: {}",
        inspector_current.text()
    );

    click(&ui, "key-esc");
    let after_click = inspector_current.text().to_string();
    assert!(
        after_click.contains("esc"),
        "clicking the Esc key must open it in the inspector, got: {after_click}"
    );
    // The factory action and its provenance are both shown.
    let factory = widget::<gtk4::Label>(&ui, "inspector-factory");
    assert!(
        factory.text().contains("documented"),
        "the factory action's source must be stated: {}",
        factory.text()
    );

    // ---------------------------------------------------------------------------------------
    // The catalogue can be searched and filtered.
    // ---------------------------------------------------------------------------------------
    let search = widget::<gtk4::SearchEntry>(&ui, "action-search");
    search.set_text("volume");
    let catalogue = widget::<gtk4::Box>(&ui, "action-catalogue");
    let catalogue_text = all_text(&catalogue);
    assert!(
        catalogue_text.to_ascii_lowercase().contains("volume"),
        "searching for 'volume' must offer candidates, got: {catalogue_text}"
    );
    let count_label = widget::<gtk4::Label>(&ui, "catalogue-count");
    assert!(
        count_label.text().contains("documented action tokens"),
        "the catalogue must say how many tokens exist: {}",
        count_label.text()
    );
    search.set_text("");

    // ---------------------------------------------------------------------------------------
    // Assigning a remap updates the pending list, the diff and the undo button.
    // ---------------------------------------------------------------------------------------
    let pending = widget::<gtk4::Box>(&ui, "pending-panel");
    assert!(
        all_text(&pending).contains("No pending changes"),
        "before any edit the panel must say so: {}",
        all_text(&pending)
    );

    let action_entry = widget::<gtk4::Entry>(&ui, "inspector-action");
    action_entry.set_text("caps");
    click(&ui, "assign-remap");

    let pending_text = all_text(&pending);
    assert!(
        pending_text.contains("esc") && pending_text.contains("caps"),
        "the pending list must describe the assignment in words, got: {pending_text}"
    );
    let diff_after_assign = text_of(&widget::<gtk4::TextView>(&ui, "diff-view").buffer());
    assert!(
        diff_after_assign.contains("[esc]>[caps]"),
        "the diff must show the new line, got: {diff_after_assign}"
    );
    assert!(
        widget::<gtk4::Button>(&ui, "undo-button").is_sensitive(),
        "undo must become available after an edit"
    );
    let status = widget::<gtk4::Label>(&ui, "status-line");
    assert!(
        !status.text().is_empty(),
        "every action reports its outcome on the status line"
    );

    // ---------------------------------------------------------------------------------------
    // Undo puts it back.
    // ---------------------------------------------------------------------------------------
    click(&ui, "undo-button");
    let diff_after_undo = text_of(&widget::<gtk4::TextView>(&ui, "diff-view").buffer());
    assert!(
        !diff_after_undo.contains("[esc]>"),
        "undo must remove the assignment, got: {diff_after_undo}"
    );

    // Redo brings it back, so the operator cannot lose work by exploring.
    click(&ui, "redo-button");
    let diff_after_redo = text_of(&widget::<gtk4::TextView>(&ui, "diff-view").buffer());
    assert!(
        diff_after_redo.contains("[esc]>[caps]"),
        "redo must restore the assignment, got: {diff_after_redo}"
    );

    // ---------------------------------------------------------------------------------------
    // The write flow demands an explicit confirmation, and the acknowledged count is the one on
    // screen.
    // ---------------------------------------------------------------------------------------
    click(&ui, "write-button");
    let checkbox = in_dialogs::<gtk4::CheckButton>("confirm-write-checkbox")
        .expect("the write confirmation must open");
    let confirm = in_dialogs::<gtk4::Button>("confirm-write-button")
        .expect("the confirmation must have a write button");
    assert!(
        !checkbox.is_active(),
        "the confirmation must never be pre-filled"
    );
    assert!(
        !confirm.is_sensitive(),
        "the write button must stay disabled until the operator confirms"
    );
    let preview = in_dialogs::<gtk4::TextView>("write-preview")
        .expect("the confirmation must show what will change");
    let preview_text = text_of(&preview.buffer());
    assert!(
        preview_text.contains("layouts/layout1.txt"),
        "the preview must name the target file: {preview_text}"
    );
    assert!(
        preview_text.contains("udisksctl") || preview_text.contains("Eject"),
        "the preview must include the eject instructions: {preview_text}"
    );
    checkbox.set_active(true);
    assert!(
        confirm.is_sensitive(),
        "once the operator confirms, the write button becomes available"
    );

    // Close the dialog again; nothing is written by this test.
    for toplevel in gtk4::Window::list_toplevels() {
        if let Ok(window) = toplevel.downcast::<gtk4::Window>() {
            window.close();
        }
    }
    ui.window().close();
}

/// Every label text inside a widget subtree, so an assertion can be about meaning rather than
/// about how many child widgets happen to exist.
fn all_text<T: IsA<gtk4::Widget>>(root: &T) -> String {
    let root = root.upcast_ref::<gtk4::Widget>();
    let mut out = String::new();
    if let Ok(label) = root.clone().downcast::<gtk4::Label>() {
        out.push_str(&label.text());
        out.push('\n');
    }
    let mut child = root.first_child();
    while let Some(current) = child {
        out.push_str(&all_text(&current));
        child = current.next_sibling();
    }
    out
}
