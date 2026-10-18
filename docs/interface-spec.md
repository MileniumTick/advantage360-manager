# Interface specification: the TUI and the GTK4 GUI

Both interfaces are thin clients of `adv360-core`. Read `crates/adv360-core/src/session.rs`
first: `Session` is the only thing either interface mutates, and it already provides loading,
key state with documented factory defaults, editing, undo/redo, the diff, validation, drafts,
export/import, backups and the guarded write path.

## The one rule that is not negotiable

An interface **must not** parse a configuration file, validate one, render one into text, take a
backup, or write to the v-Drive by itself. If an interface needs something the core does not
expose, extend the core and use it from both interfaces. Two parsers or two writers means the
protection rules only hold in one of them.

Concretely, these core calls cover everything both interfaces need:

| Need | Core call |
|---|---|
| open the keyboard (or a copy of it) | `Session::open(options)` / `Session::open_at(path, options)` |
| what is on the v-Drive | `session.device()`, `session.summary()`, `session.profile_summary()` |
| the active keyboard layout | `session.layout()`, `session.set_layout_override(layout, variant)` |
| current function of a key | `session.key_state(profile, layer, position)` |
| every key of a layer | `session.layer_keys(profile, layer)` |
| a searchable action catalogue | `adv360_core::tokens::search(needle)`, `tokens::by_category(category)` |
| what an action will type | `adv360_core::kbd_layout::preview(token, session.layout())` |
| assign / remove | `session.set_binding(profile, layer, position, Some(assignment))` / `None` |
| draft a whole layer | `adv360_core::presets::build_draft()`, `session.apply_preset(..)`, `session.apply_access_key(..)` |
| pending differences | `session.diff()`, `session.pending_changes()`, `session.file_diff(index)` |
| problems | `session.validation()`, `session.validation_for_write(profile)` |
| undo / redo | `session.can_undo()`, `session.undo()`, `session.can_redo()`, `session.redo()` |
| save a draft / export | `session.export_to(dir)`, `session.import_from(dir)` |
| backup then write | `session.backup(&[profile])`, then `session.write_profile(WriteConfirmation { .. })` |
| restore | `session.restore(&backup_set, &paths)` |
| how to mount / eject | `adv360_core::device::mount_instructions()`, `device::eject_instructions(root)` |

`WriteConfirmation` is a struct with `profile`, `confirmed_by_operator: true` and
`acknowledged_changed_lines` (the count the operator actually saw). The core refuses the write if
the count does not match, if there is no verified backup, or if validation reports any error. The
interfaces must therefore show the count before asking, and must never pre-fill `true` without an
explicit operator action.

## Required functions, in both interfaces

The two interfaces must reach parity. A feature present in only one of them is a defect.

1. Open the v-Drive, or select a folder manually; show `device::mount_instructions()` when it is
   not available. Never run as root.
2. Choose a profile 1–9. Profile 0 is listed, clearly marked non-editable, and cannot be selected
   for editing.
3. Choose one of the five layers (`base`, `keypad`, `function1`, `function2`, `function3`).
4. Show both modules using the physical map (`positions::positions_in(half, block)`), including
   the non-programmable SmartSet key and the foot-pedal positions.
   * GUI: select keys with the mouse.
   * TUI: navigate with the keyboard, search, and a readable list/map. No mouse required.
5. Show the key's current function, whether it is custom or the factory action, and what the
   factory action is *with its source* (`DefaultSource`): never present an inferred or uncertain
   value as documented. `KeyState::is_default_uncertain()` and
   `KeyState::effective_description()` already say this in words.
6. Assign another function from a searchable catalogue. Support plain remaps, modifier and
   layer-access actions, macros, and Tap-and-Hold where the firmware allows it
   (`RemapLine::tap_and_hold`). Preview the character the action will type, and mark it uncertain
   whenever `CharPreview::confident` is false.
7. Offer the programming-symbol and navigation presets as **drafts to review**. Show the review
   table (`LayerPreset::table()`) and the exact lines. Never apply without a separate, explicit
   operator action, and never on profile 0.
8. Show the keyboard layout, allow correcting it manually, and distinguish the token the keyboard
   sends from the character Linux produces.
9. Show pending differences, validation findings (error/warning/info with line numbers and hints),
   contextual help and key shortcuts, undo and redo.
10. Save drafts locally and export configurations; import them back.
11. Before writing: show the target file, the changed line numbers, the pending change list, the
    backup that will be taken, and the eject instructions. Require explicit confirmation.
12. After writing: show the verification result, the backup location, the next steps and the eject
    instructions. On failure show `CoreError::recovery_hints()` verbatim.

## TUI requirements (`crates/adv360-tui`)

* Ratatui 0.30 + Crossterm 0.29, already in the manifest. Do not add another terminal backend.
* Separate a pure state machine from the terminal wiring:
  * `pub struct App` holding the `Session`, the selected profile and layer, the selected position,
    the catalogue search, the open overlay/panel, and a status/error line.
  * `pub enum Msg` covering a key press (`crossterm::event::KeyEvent`), resize and quit.
  * `impl App { pub fn handle(&mut self, msg: Msg) -> bool /* keep running */ }`
  * `pub fn draw(&self, frame: &mut ratatui::Frame)`
  * `pub fn run(options: SessionOptions) -> Result<(), Box<dyn std::error::Error>>` which wires
    crossterm: raw mode, alternate screen, bracketed paste, event loop, **and restores the terminal
    on normal exit, on error and on panic** (install a panic hook that restores and re-raises).
* `pub fn run_headless(session: Session, script: &[Msg], size: (u16, u16)) -> Vec<String>`: drive
  `App` with a scripted message list and render each step into a `ratatui::backend::TestBackend`,
  returning the frame contents as text. This is how the TUI is tested without a terminal, and it
  must cover: profile switching, layer switching, key selection, catalogue search, applying an
  assignment, undo, the diff view, the help view, the confirmation and the narrow-terminal layout.
* Keyboard-only operation, with at least: `?` help, `q` quit, `Tab`/`Shift-Tab` cycle panels,
  `p`/`Shift-P` profile, `l`/`Shift-L` layer, arrows/`hjkl` move the key cursor, `/` search,
  `Enter` open/edit, `Esc` close, `u`/`Ctrl-R` undo/redo, `d` diff, `w` write flow, `b` backup,
  `e` export. Show a key legend on screen so nothing has to be memorised.
* Adapt to narrow and short terminals: never panic on a tiny area, degrade to a list when the
  map does not fit, and drop panels in a documented priority order.
* Support terminals without 256 colours: use `Modifier` and simple attributes as a fallback.

## GUI requirements (`crates/adv360-gui`)

* GTK4 via the `gtk4` crate already in the manifest (`v4_10` feature).
* Same split: a testable view model plus the widget layer.
  * `pub struct ViewModel` built by
    `pub fn build_view_model(session: &Session, view: &ViewState) -> ViewModel`, where `ViewState`
    holds the selected profile, layer, position and search text.
  * `pub fn render_key_map(half: Half, block: KeyBlock, view: &ViewModel) -> Vec<KeyCell>` with
    `KeyCell { position: &'static PositionDef, label: String, sub_label: String, custom: bool,
    selected: bool, findings: Severity-ish }`, in physical order, so the widget layer only places
    cells and the geometry is testable.
  * `pub struct Ui` / `pub fn build_ui(session: Session) -> Ui` returning the
    `gtk4::ApplicationWindow` plus accessors for the widgets a test needs, with
    `set_widget_name()` on every widget a test asserts on.
  * `pub fn run(options: SessionOptions) -> Result<(), Box<dyn std::error::Error>>` calling
    `gtk4::Application::run`.
* Widgets: a header bar with the device path, profile selector and layer tabs; two key-well
  widgets (left and right) drawn from the key map with the thumb cluster and pedal row as separate
  blocks; a key inspector; a searchable action catalogue with category filtering; a pending-changes
  panel; a diff/text view; a validation panel; a help/`about` dialog listing
  `adv360_core::SOURCES`; undo/redo/backup/export/write buttons with confirmation dialogs.
* Never block the main loop on I/O: backups and writes touch the filesystem, so run them on a
  worker thread and deliver the result back with `glib::MainContext::channel` or
  `glib::spawn_future_local`. Show a spinner/progress while the work is in flight.
* Show every error with `CoreError::recovery_hints()` and never swallow one.

## Evidence required before either interface counts as done

* `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` and `cargo build --release --workspace` all clean.
* TUI: `run_headless` tests covering the list above, plus pasted frame output showing the map, the
  catalogue, the diff and the help.
* GUI: `build_view_model` and `render_key_map` tests, plus `build_ui` exercised under a real
  display. Broadway is available on this machine (`gtk4-broadwayd`), so the window can be checked
  in a real browser with the Playwright CLI.
* No interface may claim a feature the other lacks.
