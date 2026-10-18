//! GTK4 desktop interface for the Kinesis Advantage360 SmartSet USB keyboard.
//!
//! This crate is a thin client of `adv360-core`: it renders a pure [`view::ViewModel`], forwards
//! operator actions to [`adv360_core::session::Session`], and never reads, parses, validates,
//! backs up or writes a configuration file itself.
//!
//! The split is deliberate:
//!
//! * [`view`] is GTK-free and headlessly testable. [`view::build_view_model`] turns a session
//!   plus the interface's selection into plain data, and [`view::render_key_map`] returns one
//!   cell per physical key in physical order, so the geometry can be asserted without a display.
//! * [`ui`] builds the widgets, wires the signals, runs backup/write/export/import on a worker
//!   thread and delivers the result back to the main loop.
//!
//! Entry points:
//!
//! * [`run`] builds and runs the application.
//! * [`build_ui`] builds the widgets for a loaded session and returns the window plus accessors.

pub mod ui;
pub mod view;

pub use ui::{build_ui, run, Ui};
pub use view::{
    build_view_model, render_key_map, AccessKeyView, CatalogueEntry, CatalogueView, DeviceView,
    FileDiffView, FindingView, InspectorView, KeyCell, KeyRender, LayerTab, LayoutView,
    PendingChangeView, PresetReview, PresetRowView, PresetTableView, PreviewView, ProfileOption,
    ViewModel, ViewState, WritePreview,
};
