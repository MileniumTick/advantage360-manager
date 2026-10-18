//! Display-dependent smoke test for the widget layer.
//!
//! When no display is available the test prints why and returns, so the suite stays green on a
//! headless machine. Under a real (or Broadway/Xvfb) display it builds the window and checks the
//! widgets a test relies on by their stable names.

mod common;

use gtk4::prelude::*;

#[test]
fn build_ui_constructs_the_window_and_its_named_widgets() {
    if let Err(error) = gtk4::init() {
        eprintln!(
            "skipping build_ui test: no display available (gtk4::init failed: {error}). \
             Run under a display or gtk4-broadwayd to exercise the widget layer."
        );
        return;
    }

    let drive = common::TempDrive::new("build-ui");
    let session = common::open(&drive);
    let ui = adv360_gui::build_ui(session);

    assert_eq!(
        ui.window().title().map(|t| t.to_string()).as_deref(),
        Some("adv360-config")
    );
    assert!(ui.session_available());

    let named = [
        "device-path",
        "profile-selector",
        "layer-tabs",
        "key-well-left",
        "key-well-right",
        "key-well-pedal",
        "key-inspector",
        "action-search",
        "action-category",
        "pending-panel",
        "diff-view",
        "validation-panel",
        "preset-review",
        "undo-button",
        "redo-button",
        "backup-button",
        "export-button",
        "import-button",
        "write-button",
        "about-button",
        "key-esc",
    ];
    for name in named {
        assert!(
            ui.find_widget(name).is_some(),
            "the window must contain the widget named '{name}'"
        );
    }

    assert_eq!(
        ui.profile_selector().widget_name().as_str(),
        "profile-selector"
    );

    // GTK4 removed gtk_widget_destroy; the window is closed instead.
    ui.window().close();
}
