//! Headless tests for the pure view model and the key-map geometry.
//!
//! These tests never initialise GTK and never require a display.

mod common;

use adv360_core::layer::{LayerId, ProfileId};
use adv360_core::positions::{Half, KeyBlock};
use adv360_core::presets::Assignment;
use adv360_core::tokens::TokenCategory;
use adv360_gui::view::{build_view_model, render_key_map, ViewState};

fn default_view() -> ViewState {
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

#[test]
fn the_view_model_reports_the_device_profiles_and_layers() {
    let drive = common::TempDrive::new("view-model");
    let session = common::open(&drive);
    let view_model = build_view_model(&session, &default_view());

    assert!(view_model.device.available);
    assert!(view_model.device.root.contains("ADV360"));
    assert_eq!(view_model.profiles.len(), 10);
    assert!(
        !view_model.profiles[0].editable,
        "profile 0 must be marked non-editable"
    );
    assert!(view_model
        .profiles
        .iter()
        .skip(1)
        .all(|profile| profile.editable));
    assert_eq!(view_model.layers.len(), 5);
    assert_eq!(view_model.layers[0].layer, LayerId::Base);
    assert_eq!(view_model.layers[4].layer, LayerId::Function3);
    assert_eq!(view_model.selected_profile, ProfileId(1));
    assert_eq!(view_model.selected_layer, LayerId::Base);
    assert!(!view_model.catalogue.entries.is_empty());
    assert!(view_model.key("esc").is_some());
}

#[test]
fn render_key_map_covers_both_modules_and_the_pedal_row_in_physical_order() {
    let drive = common::TempDrive::new("key-map");
    let session = common::open(&drive);
    let view_model = build_view_model(&session, &default_view());

    let left_main = render_key_map(Half::Left, KeyBlock::Main, &view_model);
    let left_thumb = render_key_map(Half::Left, KeyBlock::Thumb, &view_model);
    let right_main = render_key_map(Half::Right, KeyBlock::Main, &view_model);
    let right_thumb = render_key_map(Half::Right, KeyBlock::Thumb, &view_model);
    let pedals = render_key_map(Half::Pedal, KeyBlock::Pedal, &view_model);

    assert_eq!(left_main.len(), 32);
    assert_eq!(left_thumb.len(), 6);
    assert_eq!(right_main.len(), 32);
    assert_eq!(right_thumb.len(), 6);
    assert_eq!(pedals.len(), 4);

    // Physical order: sorted by row then column.
    assert_eq!(left_main.first().unwrap().position.token, "eql");
    assert_eq!(left_main.last().unwrap().position.token, "rght");
    assert_eq!(right_main.first().unwrap().position.token, "ss");
    assert_eq!(pedals.first().unwrap().position.token, "lpd1");
    assert_eq!(pedals.last().unwrap().position.token, "rpd2");

    // The non-programmable SmartSet key is drawn but never presented as editable.
    let smart_set = right_main
        .iter()
        .find(|cell| cell.position.token == "ss")
        .unwrap();
    assert!(!smart_set.position.programmable);
    assert!(!smart_set.custom);
}

#[test]
fn a_custom_binding_appears_in_the_key_map_and_the_inspector() {
    let drive = common::TempDrive::new("custom");
    let mut session = common::open(&drive);
    session
        .set_binding(
            ProfileId(1),
            LayerId::Base,
            "q",
            Some(Assignment::Remap {
                position: "q".into(),
                action: "obrk".into(),
            }),
        )
        .unwrap();

    let mut view = default_view();
    view.position = Some("q".into());
    let view_model = build_view_model(&session, &view);

    let left_main = render_key_map(Half::Left, KeyBlock::Main, &view_model);
    let q_cell = left_main
        .iter()
        .find(|cell| cell.position.token == "q")
        .unwrap();
    assert!(q_cell.custom);
    assert!(q_cell.selected);
    assert!(q_cell.sub_label.contains("obrk"), "{}", q_cell.sub_label);

    let inspector = view_model.inspector.as_ref().unwrap();
    assert!(inspector.custom);
    assert_eq!(inspector.factory_action, "q");
    assert!(!inspector.factory_uncertain);
    let preview = inspector.preview.as_ref().unwrap();
    assert!(preview.confident);
    assert_eq!(preview.character.as_deref(), Some("["));
    assert_eq!(preview.shifted.as_deref(), Some("{"));
}

#[test]
fn the_inspector_marks_an_unpublished_factory_action_as_uncertain() {
    let drive = common::TempDrive::new("uncertain");
    let session = common::open(&drive);
    let mut view = default_view();
    view.position = Some("lpd1".into());
    let view_model = build_view_model(&session, &view);
    let inspector = view_model.inspector.as_ref().unwrap();
    assert!(inspector.factory_uncertain);
    assert!(inspector.factory_source.starts_with("uncertain"));
}

#[test]
fn the_catalogue_can_be_searched_and_filtered() {
    let drive = common::TempDrive::new("catalogue");
    let session = common::open(&drive);

    let mut view = default_view();
    view.search = "volume".to_string();
    let searched = build_view_model(&session, &view);
    assert!(searched
        .catalogue
        .entries
        .iter()
        .any(|entry| entry.token == "vol+"));

    let mut view = default_view();
    view.category = Some(TokenCategory::Letters);
    let filtered = build_view_model(&session, &view);
    assert!(!filtered.catalogue.entries.is_empty());
    assert!(filtered
        .catalogue
        .entries
        .iter()
        .all(|entry| entry.category == "Letters"));
}

#[test]
fn the_write_preview_shows_the_target_changed_lines_backup_and_eject_steps() {
    let drive = common::TempDrive::new("write-preview");
    let mut session = common::open(&drive);
    session
        .set_binding(
            ProfileId(1),
            LayerId::Base,
            "esc",
            Some(Assignment::Remap {
                position: "esc".into(),
                action: "caps".into(),
            }),
        )
        .unwrap();

    let view_model = build_view_model(&session, &default_view());
    let write = &view_model.write;
    assert!(write.editable);
    assert!(write.has_file);
    assert_eq!(write.target_file.as_deref(), Some("layouts/layout1.txt"));
    assert_eq!(write.changed_line_count, 1);
    assert!(!write.pending.is_empty());
    assert_eq!(write.backup_scope, "profile1");
    assert!(!write.backup_root.is_empty());
    assert!(write
        .eject_instructions
        .iter()
        .any(|line| line.contains("udisksctl")));
    assert!(write.can_write());
}

#[test]
fn presets_are_only_described_never_applied_by_the_view_model() {
    let drive = common::TempDrive::new("presets");
    let session = common::open(&drive);
    assert!(session.diff().is_empty());

    let view_model = build_view_model(&session, &default_view());
    assert!(!view_model.presets.symbols.rows.is_empty());
    assert!(!view_model.presets.navigation.rows.is_empty());
    assert!(view_model.presets.exact_lines.contains("<function1>"));
    assert!(view_model.presets.exact_lines.contains("<function2>"));
    assert!(!view_model.presets.access_keys.is_empty());

    // Building the review must not touch the draft.
    assert!(
        session.diff().is_empty(),
        "the view model must never apply a preset"
    );
}

#[test]
fn profile_zero_is_reported_as_non_editable_and_unwritable() {
    let drive = common::TempDrive::new("profile-zero");
    let session = common::open(&drive);
    let mut view = default_view();
    view.profile = ProfileId::ZERO;
    let view_model = build_view_model(&session, &view);
    assert!(!view_model.write.editable);
    assert!(!view_model.write.can_write());
    assert!(!view_model.selected_profile.is_editable());
}
