//! `adv360-config` — the command line entry point.
//!
//! Every subcommand is a thin shell over [`adv360_core`]. The two interactive interfaces are
//! started through `gui` and `tui`; everything else here is the scriptable equivalent, which is
//! also how the protection rules can be checked without a graphical session.

use std::path::PathBuf;
use std::process::ExitCode;

use adv360_core::device;
use adv360_core::error::CoreError;
use adv360_core::glyph;
use adv360_core::kbd_layout;
use adv360_core::layer::{LayerId, ProfileId};
use adv360_core::positions::{self, KeyBlock};
use adv360_core::prelude::*;
use adv360_core::presets;
use adv360_core::session::{Session, SessionOptions};
use adv360_core::{APP_NAME, SOURCES, VERSION};

/// Exit codes, so scripts can tell the difference between "the check failed" and "the tool broke".
const EXIT_OK: u8 = 0;
const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&argv) {
        Ok(code) => ExitCode::from(code),
        Err(Failure::Usage(message)) => {
            eprintln!("{APP_NAME}: {message}");
            eprintln!("Try '{APP_NAME} --help' for the available commands.");
            ExitCode::from(EXIT_USAGE)
        }
        Err(Failure::Core(error)) => {
            report_error(&error);
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

enum Failure {
    Usage(String),
    Core(CoreError),
}

impl From<CoreError> for Failure {
    fn from(e: CoreError) -> Self {
        Failure::Core(e)
    }
}

fn report_error(error: &CoreError) {
    eprintln!("error [{}]: {error}", error.code());
    for hint in error.recovery_hints() {
        eprintln!("  - {hint}");
    }
}

// ---------------------------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Options {
    device: Option<PathBuf>,
    layout: Option<String>,
    variant: Option<String>,
    backup_dir: Option<PathBuf>,
    profile: Option<ProfileId>,
    layer: Option<LayerId>,
    draft: Option<PathBuf>,
    out: Option<PathBuf>,
    from_backup: Option<PathBuf>,
    access_key: Option<String>,
    preset: Option<String>,
    write: bool,
    yes: bool,
    json: bool,
}

enum Command {
    Help,
    About,
    Gui,
    Tui,
    Detect,
    Layout,
    Dump,
    Keys,
    Diff,
    Validate,
    Export,
    Backup,
    Backups,
    Presets,
    ApplyPreset,
    Restore,
    MountHelp,
    Fixture,
}

fn dispatch(argv: &[String]) -> Result<u8, Failure> {
    if argv.is_empty() {
        print_help();
        return Ok(EXIT_OK);
    }
    let (command, options) = parse(argv)?;
    match command {
        Command::Help => {
            print_help();
            Ok(EXIT_OK)
        }
        Command::About => {
            print_about();
            Ok(EXIT_OK)
        }
        Command::Gui => run_gui(&options),
        Command::Tui => run_tui(&options),
        Command::Detect => run_detect(&options),
        Command::Layout => run_layout(&options),
        Command::Dump => run_dump(&options),
        Command::Keys => run_keys(&options),
        Command::Validate => run_validate(&options),
        Command::Diff => run_diff(&options),
        Command::Export => run_export(&options),
        Command::Backup => run_backup(&options),
        Command::Backups => run_backups(&options),
        Command::Presets => run_presets(&options),
        Command::ApplyPreset => run_apply_preset(&options),
        Command::Restore => run_restore(&options),
        Command::MountHelp => run_mount_help(&options),
        Command::Fixture => run_fixture(&options),
    }
}

fn parse(argv: &[String]) -> Result<(Command, Options), Failure> {
    let mut options = Options::default();
    let mut command: Option<Command> = None;
    let mut i = 0usize;
    while i < argv.len() {
        let arg = argv[i].as_str();
        let take_value = |i: &mut usize| -> Result<String, Failure> {
            *i += 1;
            argv.get(*i)
                .cloned()
                .ok_or_else(|| Failure::Usage(format!("{arg} needs a value")))
        };
        match arg {
            "--help" | "-h" => command = Some(Command::Help),
            "--device" => options.device = Some(PathBuf::from(take_value(&mut i)?)),
            "--layout" => options.layout = Some(take_value(&mut i)?),
            "--variant" => options.variant = Some(take_value(&mut i)?),
            "--backup-dir" => options.backup_dir = Some(PathBuf::from(take_value(&mut i)?)),
            "--profile" => {
                let raw = take_value(&mut i)?;
                let n: u8 = raw
                    .parse()
                    .map_err(|_| Failure::Usage(format!("--profile expects 0-9, got '{raw}'")))?;
                if n > 9 {
                    return Err(Failure::Usage(format!("--profile expects 0-9, got '{n}'")));
                }
                options.profile = Some(ProfileId(n));
            }
            "--layer" => {
                let raw = take_value(&mut i)?;
                options.layer = Some(LayerId::parse(&raw).ok_or_else(|| {
                    Failure::Usage(format!(
                        "--layer expects one of base, keypad, function1, function2, function3; \
                         got '{raw}'"
                    ))
                })?);
            }
            "--draft" => options.draft = Some(PathBuf::from(take_value(&mut i)?)),
            "--out" => options.out = Some(PathBuf::from(take_value(&mut i)?)),
            "--from-backup" => options.from_backup = Some(PathBuf::from(take_value(&mut i)?)),
            "--access-key" => options.access_key = Some(take_value(&mut i)?),
            "--preset" => options.preset = Some(take_value(&mut i)?),
            "--write" => options.write = true,
            "--yes" => options.yes = true,
            "--json" => options.json = true,
            other if other.starts_with("--") => {
                return Err(Failure::Usage(format!("unknown option '{other}'")))
            }
            other => {
                if command.is_some() {
                    return Err(Failure::Usage(format!(
                        "unexpected extra argument '{other}'"
                    )));
                }
                command = Some(match other {
                    "gui" => Command::Gui,
                    "tui" => Command::Tui,
                    "detect" => Command::Detect,
                    "layout" => Command::Layout,
                    "dump" => Command::Dump,
                    "keys" => Command::Keys,
                    "validate" => Command::Validate,
                    "diff" => Command::Diff,
                    "export" => Command::Export,
                    "backup" => Command::Backup,
                    "backups" => Command::Backups,
                    "presets" => Command::Presets,
                    "apply-preset" => Command::ApplyPreset,
                    "restore" => Command::Restore,
                    "mount-help" => Command::MountHelp,
                    "fixture" => Command::Fixture,
                    "about" => Command::About,
                    "help" => Command::Help,
                    unknown => return Err(Failure::Usage(format!("unknown command '{unknown}'"))),
                });
            }
        }
        i += 1;
    }
    let command = command.ok_or_else(|| Failure::Usage("no command given".to_string()))?;
    Ok((command, options))
}

fn print_help() {
    println!(
        "{APP_NAME} {VERSION} — configure a Kinesis Advantage360 SmartSet USB keyboard

USAGE
    {APP_NAME} <command> [options]

INTERFACES
    gui                     Launch the GTK4 desktop interface
    tui                     Launch the terminal interface (works with no graphical session)

INSPECT
    detect                  Find the v-Drive and report what is on it
    layout                  Report the detected keyboard layout and the characters actions produce
    dump                    Print a summary of the session (files, profiles, validation)
    keys                    Print every key of one profile and layer with its current function
    validate                Validate the configuration; exit code 1 when there is an error
    diff                    Show what would change (compare the v-Drive with --draft)
    mount-help              Explain how to open and how to eject the v-Drive

TRY IT WITHOUT THE KEYBOARD
    fixture                 Create a synthetic v-Drive in --out DIR so both interfaces can be
                            explored and reviewed with no keyboard attached (never real data)

CHANGE (nothing touches the keyboard unless --write and --yes are both given)
    presets                 Generate the symbol and navigation drafts for review
    apply-preset            Build a draft for one profile (see --preset)
    export                  Write the drafts to a directory
    backup                  Take a dated, verified local backup
    restore                 Restore files from a backup set (--from-backup)
    backups                 List the local backup sets

OPTIONS
    --device PATH           Use this v-Drive root instead of searching for it
    --layout NAME           Override the detected keyboard layout (for example us)
    --variant NAME          XKB variant of that layout
    --backup-dir PATH       Where dated backups are written
    --profile N             Profile 0-9 (0 is shown but is never editable)
    --layer NAME            base | keypad | function1 | function2 | function3
    --preset NAME           symbols | navigation | both
    --access-key POS        hk1 | hk3 | caps | rwin | none
    --draft DIR             Read draft files from DIR
    --out DIR               Output directory
    --from-backup DIR       Backup set to restore from
    --write                 Write the draft to the v-Drive (requires --yes)
    --yes                   Confirm the action that needs confirmation
    --json                  Machine-readable output where supported
    --help, -h              This text
    about                   Version and the documentation this build was written against

SAFETY
    profile 0 is never editable; settings.txt is never modified; the firmware is never touched;
    every write is preceded by a dated verified backup and refuses to run when validation reports
    an error. Nothing is written without --write and --yes.
"
    );
}

fn print_about() {
    println!("{APP_NAME} {VERSION}");
    println!();
    println!("Written against these official documents:");
    for (name, detail) in SOURCES {
        println!("  * {name}");
        println!("    {detail}");
    }
    println!();
    println!(
        "Default backup root: {}",
        adv360_core::backup::default_backup_root().display()
    );
    println!();
    println!(
        "Layouts whose character mapping is encoded: {}",
        kbd_layout::supported_layouts().join(", ")
    );
    println!(
        "Position map: {} positions, {} of them programmable, plus the non-programmable SmartSet key",
        positions::POSITIONS.len(),
        positions::programmable().count()
    );
    let non_printable: Vec<&str> = positions::POSITIONS
        .iter()
        .filter(|p| p.block == KeyBlock::Pedal)
        .map(|p| p.token)
        .collect();
    println!("Foot pedal positions: {}", non_printable.join(", "));
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn session_options(options: &Options) -> SessionOptions {
    SessionOptions {
        device: options.device.clone(),
        layout_override: options
            .layout
            .as_ref()
            .map(|l| (l.clone(), options.variant.clone())),
        backup_root: options
            .backup_dir
            .clone()
            .unwrap_or_else(adv360_core::backup::default_backup_root),
    }
}

fn open_session(options: &Options) -> Result<Session, Failure> {
    let session = match &options.device {
        Some(path) => Session::open_at(path, session_options(options))?,
        None => Session::open(session_options(options))?,
    };
    Ok(session)
}

/// Open a session and, when `--draft` was given, replace its draft with the exported one.
fn open_session_with_draft(options: &Options) -> Result<Session, Failure> {
    let mut session = open_session(options)?;
    if let Some(dir) = &options.draft {
        let loaded = session.import_from(dir)?;
        eprintln!(
            "imported {} draft file(s) from {}",
            loaded.len(),
            dir.display()
        );
    }
    Ok(session)
}

fn profile_or_default(options: &Options, fallback: ProfileId) -> ProfileId {
    options.profile.unwrap_or(fallback)
}

fn cell(text: &str, width: usize) -> String {
    let t: String = text.chars().take(width).collect();
    format!("{t:<width$}", width = width)
}

/// A serialisable view of a validation finding.
///
/// The core type deliberately is not `Serialize`: its `code` is a static catalogue string, so the
/// machine-readable shape is defined here, at the boundary, where it can be kept stable.
fn finding_json(f: &Finding) -> serde_json::Value {
    serde_json::json!({
        "severity": f.severity.label(),
        "relative_path": f.relative_path,
        "line": f.line,
        "column": f.column,
        "code": f.code,
        "message": f.message,
        "hint": f.hint,
    })
}

fn print_table(headers: &[(&str, usize)], rows: &[Vec<String>]) {
    let mut line = String::new();
    for (title, width) in headers {
        line.push_str(&cell(title, *width));
        line.push(' ');
    }
    println!("{}", line.trim_end());
    let rule: String = headers
        .iter()
        .map(|(_, w)| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join(" ");
    println!("{rule}");
    for row in rows {
        let mut line = String::new();
        for (value, (_, width)) in row.iter().zip(headers) {
            line.push_str(&cell(value, *width));
            line.push(' ');
        }
        println!("{}", line.trim_end());
    }
}

// ---------------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------------

fn run_gui(options: &Options) -> Result<u8, Failure> {
    #[cfg(feature = "gui")]
    {
        adv360_gui::run(session_options(options)).map_err(|e| {
            Failure::Core(CoreError::Other(format!(
                "the GTK4 interface could not start: {e}"
            )))
        })?;
        Ok(EXIT_OK)
    }
    #[cfg(not(feature = "gui"))]
    {
        let _ = options;
        Err(Failure::Core(CoreError::Other(
            "this build was compiled without the 'gui' feature, so the GTK4 interface is not \
             available; use 'tui' or rebuild with --features gui"
                .to_string(),
        )))
    }
}

fn run_tui(options: &Options) -> Result<u8, Failure> {
    adv360_tui::run(session_options(options)).map_err(|e| {
        Failure::Core(CoreError::Other(format!(
            "the terminal interface could not start: {e}"
        )))
    })?;
    Ok(EXIT_OK)
}

fn run_detect(options: &Options) -> Result<u8, Failure> {
    let session = open_session(options)?;
    let device = session.device();
    let summary = session.summary();

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(EXIT_OK);
    }

    println!("v-Drive: {}", device.root.display());
    println!("Detected because:");
    for line in &device.evidence {
        println!("  - {line}");
    }
    if let Some(d) = &device.layouts_dir {
        println!("layouts folder found: {d}");
    }
    if let Some(d) = &device.lighting_dir {
        println!("lighting folder found: {d}");
    }
    println!();
    if device.files.is_empty() {
        println!("No files found on the drive.");
    } else {
        let rows: Vec<Vec<String>> = device
            .files
            .iter()
            .map(|f| {
                vec![
                    f.relative_path.clone(),
                    f.role.label().to_string(),
                    f.profile
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "-".into()),
                    f.bytes.to_string(),
                    if f.role.is_writable() {
                        "writable".into()
                    } else {
                        "read-only".into()
                    },
                ]
            })
            .collect();
        print_table(
            &[
                ("file", 34),
                ("role", 44),
                ("profile", 9),
                ("bytes", 8),
                ("access", 10),
            ],
            &rows,
        );
    }

    let layout = session.layout();
    println!();
    println!(
        "Keyboard layout: {} ({})",
        layout.layout,
        layout.source.label()
    );
    println!("  {}", layout.detail);
    if let Some(w) = layout.reliability_warning() {
        println!("  NOTE: {w}");
    }
    if !session.skipped_files().is_empty() {
        println!();
        println!("Files that could not be read:");
        for (path, why) in session.skipped_files() {
            println!("  {path}: {why}");
        }
    }
    Ok(EXIT_OK)
}

fn run_layout(options: &Options) -> Result<u8, Failure> {
    let session = open_session(options)?;
    let layout = session.layout();
    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(layout).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(EXIT_OK);
    }
    for (key, value) in kbd_layout::layout_summary(layout) {
        println!("{key:>16}: {value}");
    }
    if let Some(warning) = layout.reliability_warning() {
        println!();
        println!("UNCERTAIN: {warning}");
    }
    println!();
    println!(
        "The token is what the keyboard sends over USB; the character is what Linux produces."
    );
    println!();
    let mut rows = Vec::new();
    for token in [
        "q", "1", "hyph", "eql", "obrk", "cbrk", "bsls", "scol", "apos", "grav", "comm", "perd",
        "fsls", "spc", "ent", "tab",
    ] {
        let p = kbd_layout::preview(token, layout);
        rows.push(vec![
            token.to_string(),
            p.usb_label.clone(),
            match (p.character, p.confident) {
                (Some(' '), _) => "(space)".into(),
                (Some('\n'), _) => "\\n".into(),
                (Some('\t'), _) => "\\t".into(),
                (Some(c), true) => c.to_string(),
                (Some(c), false) => format!("{c} (uncertain)"),
                (None, _) => "-".into(),
            },
            match (p.shifted_character, p.confident) {
                (Some(c), true) if c == p.character.unwrap_or('\0') => "-".into(),
                (Some(c), true) => c.to_string(),
                _ => "-".into(),
            },
            if p.confident {
                "yes".into()
            } else {
                "no".into()
            },
        ]);
    }
    print_table(
        &[
            ("token", 7),
            ("what the keyboard sends", 32),
            ("character", 12),
            ("+Shift", 8),
            ("confident", 9),
        ],
        &rows,
    );
    Ok(EXIT_OK)
}

fn run_dump(options: &Options) -> Result<u8, Failure> {
    let session = open_session_with_draft(options)?;
    let summary = session.summary();
    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(EXIT_OK);
    }
    println!("v-Drive: {}", summary.device_root.display());
    for line in &summary.evidence {
        println!("  - {line}");
    }
    println!();
    println!(
        "keyboard layout: {} ({})",
        summary.layout.layout,
        summary.layout.source.label()
    );
    println!(
        "changed lines: {}   key changes: {}   errors: {}   warnings: {}",
        summary.changed_lines, summary.key_changes, summary.errors, summary.warnings
    );
    println!(
        "undo depth: {}   redo depth: {}",
        summary.undo_depth, summary.redo_depth
    );
    println!();
    let rows: Vec<Vec<String>> = summary
        .profiles
        .iter()
        .map(|p| {
            vec![
                p.profile.0.to_string(),
                if p.editable {
                    "yes"
                } else {
                    "NO (non-programmable)"
                }
                .to_string(),
                if p.has_layout_file { "yes" } else { "no" }.to_string(),
                if p.has_lighting_file { "yes" } else { "no" }.to_string(),
                p.relative_path.clone().unwrap_or_else(|| "-".into()),
                p.pending_changed_lines.to_string(),
                p.validation_errors.to_string(),
                p.led_color_hint.clone(),
            ]
        })
        .collect();
    print_table(
        &[
            ("profile", 7),
            ("editable", 24),
            ("layout", 6),
            ("led", 4),
            ("file", 26),
            ("pending", 7),
            ("errors", 6),
            ("factory LED colour", 16),
        ],
        &rows,
    );
    if !summary.skipped_files.is_empty() {
        println!();
        println!("unreadable files:");
        for (path, why) in &summary.skipped_files {
            println!("  {path}: {why}");
        }
    }
    Ok(EXIT_OK)
}

fn run_keys(options: &Options) -> Result<u8, Failure> {
    let session = open_session_with_draft(options)?;
    let profile = profile_or_default(options, ProfileId(1));
    let layer = options.layer.unwrap_or(LayerId::Base);
    let layout = session.layout();

    if options.json {
        let keys: Vec<serde_json::Value> = session
            .layer_keys(profile, layer)
            .iter()
            .map(|k| {
                let preview = k
                    .binding
                    .as_ref()
                    .and_then(|b| match b {
                        Binding::Remap(r) => r.tap_actions().first().map(|t| t.to_string()),
                        Binding::Macro(m) => Some(m.trigger.clone()),
                    })
                    .map(|t| kbd_layout::preview(&t, layout));
                serde_json::json!({
                    "position": k.position.token,
                    "keycap": k.position.keycap,
                    "half": k.position.half.label(),
                    "block": format!("{:?}", k.position.block),
                    "current": k.effective_description(),
                    "custom": k.is_custom(),
                    "factory_action": k.default.action,
                    "factory_source": format!("{:?}", k.default.source),
                    "factory_is_documented": k.default.is_documented(),
                    "uncertain": k.is_default_uncertain(),
                    "lines": k.lines,
                    "problems": k.findings.iter().map(|f| {
                        serde_json::json!({ "severity": f.severity.label(), "code": f.code, "message": f.message })
                    }).collect::<Vec<_>>(),
                    "character": preview.and_then(|p| p.character).map(|c| c.to_string()),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "profile": profile.0,
                "layer": layer.name(),
                "keyboard_layout": layout.layout,
                "keys": keys,
            }))
            .unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(EXIT_OK);
    }

    println!(
        "{} / {} / keyboard layout {}",
        profile,
        layer.header(),
        layout.layout
    );
    if let Some(w) = layout.reliability_warning() {
        println!("NOTE: {w}");
    }
    println!();
    // The foot pedals are their own `Half`, so they are listed once rather than per module.
    let groups: [(Half, KeyBlock); 5] = [
        (Half::Left, KeyBlock::Main),
        (Half::Left, KeyBlock::Thumb),
        (Half::Right, KeyBlock::Main),
        (Half::Right, KeyBlock::Thumb),
        (Half::Pedal, KeyBlock::Pedal),
    ];
    for (half, block) in groups {
        {
            let states: Vec<KeyState> = session
                .layer_keys(profile, layer)
                .into_iter()
                .filter(|k| k.position.half == half && k.position.block == block)
                .collect();
            if states.is_empty() {
                continue;
            }
            println!(
                "{} — {}",
                half.label(),
                match block {
                    KeyBlock::Main => "main keywell",
                    KeyBlock::Thumb => "thumb cluster",
                    KeyBlock::Pedal => "foot pedals (FS360, factory action not published)",
                }
            );
            let rows: Vec<Vec<String>> = states
                .iter()
                .map(|k| {
                    let (factory, mark) = if k.default.is_documented() {
                        (k.default.action.to_string(), "")
                    } else {
                        (format!("{} ?", k.default.action), " (uncertain)")
                    };
                    vec![
                        k.position.token.to_string(),
                        glyph::position_glyph(k.position.token),
                        glyph::glyph(
                            k.binding
                                .as_ref()
                                .map(|b| match b {
                                    Binding::Remap(r) => {
                                        r.tap_actions().first().copied().unwrap_or("").to_string()
                                    }
                                    Binding::Macro(m) => m.trigger.clone(),
                                })
                                .as_deref()
                                .unwrap_or(k.default.action),
                        ),
                        k.effective_description(),
                        format!("{factory}{mark}"),
                        format!("{:?}", k.default.source),
                        k.lines
                            .iter()
                            .map(|l| l.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    ]
                })
                .collect();
            print_table(
                &[
                    ("position", 9),
                    ("key", 14),
                    ("glyph", 10),
                    ("current function", 42),
                    ("factory action", 14),
                    ("factory source", 32),
                    ("lines", 7),
                ],
                &rows,
            );
            println!();
        }
    }
    Ok(EXIT_OK)
}

fn run_validate(options: &Options) -> Result<u8, Failure> {
    let session = open_session_with_draft(options)?;
    let report = match options.profile {
        Some(p) if p.is_editable() => session.validation_for_write(p),
        _ => session.validation(),
    };
    if options.json {
        let findings: Vec<serde_json::Value> = report.findings.iter().map(finding_json).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&findings).unwrap_or_else(|_| "[]".to_string())
        );
    } else {
        println!(
            "{} error(s), {} warning(s), {} note(s)",
            report.error_count(),
            report.warning_count(),
            report.info_count()
        );
        for f in &report.findings {
            if f.severity == Severity::Info {
                continue;
            }
            println!(
                "{}:{} [{}] {} {}",
                f.relative_path,
                f.line.map(|l| l.to_string()).unwrap_or_else(|| "-".into()),
                f.severity.label(),
                f.code,
                f.message
            );
            if let Some(h) = &f.hint {
                println!("      hint: {h}");
            }
        }
    }
    Ok(if report.blocks_write() {
        EXIT_FAILURE
    } else {
        EXIT_OK
    })
}

fn run_diff(options: &Options) -> Result<u8, Failure> {
    let session = open_session_with_draft(options)?;
    let diff = session.diff();
    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&diff).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(EXIT_OK);
    }
    if diff.is_empty() {
        println!("No differences: the draft matches the v-Drive.");
        return Ok(EXIT_OK);
    }
    println!("Key changes:");
    for change in &diff.key_changes {
        println!("  [{}] {}", change.kind.label(), change.describe());
    }
    if diff.key_changes.is_empty() {
        println!("  (none)");
    }
    println!();
    println!("Files:");
    for file in &diff.files {
        if file.changed_lines.is_empty() {
            println!("  {}: unchanged", file.relative_path);
        } else {
            println!(
                "  {}: line(s) {}",
                file.relative_path,
                file.changed_lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    for file in &diff.files {
        if !file.unified.is_empty() {
            println!();
            println!("{}", file.unified.trim_end());
        }
    }
    Ok(EXIT_OK)
}

fn run_export(options: &Options) -> Result<u8, Failure> {
    let session = open_session_with_draft(options)?;
    let out = options
        .out
        .clone()
        .ok_or_else(|| Failure::Usage("export needs --out DIR".to_string()))?;
    let written = session.export_to(&out)?;
    println!("Exported {} file(s) to {}:", written.len(), out.display());
    for path in &written {
        println!("  {}", path.display());
    }
    println!();
    println!("The keyboard was not touched.");
    Ok(EXIT_OK)
}

fn run_backup(options: &Options) -> Result<u8, Failure> {
    let mut session = open_session(options)?;
    let profiles: Vec<ProfileId> = match options.profile {
        Some(p) => vec![p],
        None => ProfileId::programmable().collect(),
    };
    let set = session.backup(&profiles)?;
    println!("Backup taken: {}", set.directory.display());
    println!("  created {}", set.created_at);
    for f in &set.files {
        println!(
            "  {} ({} bytes{})",
            f.relative_path,
            f.bytes,
            if f.existed { "" } else { ", did not exist" }
        );
    }
    println!();
    println!("Verified by re-reading every copy and comparing its SHA-256.");
    Ok(EXIT_OK)
}

fn run_backups(options: &Options) -> Result<u8, Failure> {
    let root = options
        .backup_dir
        .clone()
        .unwrap_or_else(adv360_core::backup::default_backup_root);
    let sets = adv360_core::backup::list(&root);
    if sets.is_empty() {
        println!("No backup sets under {}", root.display());
        return Ok(EXIT_OK);
    }
    println!("{} backup set(s) under {}:", sets.len(), root.display());
    for set in &sets {
        println!(
            "  {}  [{}]  {} file(s)",
            set.directory.display(),
            set.scope,
            set.files.len()
        );
    }
    Ok(EXIT_OK)
}

fn run_presets(options: &Options) -> Result<u8, Failure> {
    let draft = presets::build_draft();
    let layout_override = options
        .layout
        .as_ref()
        .map(|l| KeyboardLayout::manual(l.clone(), options.variant.clone()))
        .unwrap_or_else(kbd_layout::detect);
    let sheet = presets::render_review_sheet(&draft, &layout_override);

    let out = match &options.out {
        Some(dir) => dir.clone(),
        None => {
            // Never write anywhere surprising: default to a clearly named folder in the cwd.
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("adv360-presets")
        }
    };
    std::fs::create_dir_all(&out).map_err(|source| CoreError::CreateDir {
        path: out.clone(),
        source,
    })?;

    let access_key = select_access_key(&draft, options.access_key.as_deref())?;
    let layout_text = presets::render_layout_draft(&draft, access_key.as_ref());
    let layout_path = out.join("draft-symbols-and-navigation.txt");
    std::fs::write(&layout_path, &layout_text).map_err(|source| CoreError::Write {
        path: layout_path.clone(),
        source,
    })?;
    let sheet_path = out.join("review-sheet.md");
    std::fs::write(&sheet_path, &sheet).map_err(|source| CoreError::Write {
        path: sheet_path.clone(),
        source,
    })?;

    println!("{sheet}");
    println!("---");
    println!("Written for review (the keyboard was not touched):");
    println!("  {}", layout_path.display());
    println!("  {}", sheet_path.display());
    println!();
    println!(
        "To apply it, choose a profile and run: {APP_NAME} apply-preset --profile 4 --preset both \
         --access-key {} (add --write --yes to send it to the keyboard).",
        access_key
            .as_ref()
            .map(|a| a.position.as_str())
            .unwrap_or("none")
    );
    Ok(EXIT_OK)
}

fn select_access_key(
    draft: &presets::Draft,
    requested: Option<&str>,
) -> Result<Option<presets::AccessKeyOption>, Failure> {
    match requested {
        None => Ok(draft
            .access_key_options
            .iter()
            .find(|o| o.recommended)
            .cloned()),
        Some("none") => Ok(None),
        Some(name) => draft
            .access_key_options
            .iter()
            .find(|o| o.position.eq_ignore_ascii_case(name))
            .cloned()
            .map(Some)
            .ok_or_else(|| {
                Failure::Usage(format!(
                    "--access-key expects one of {} or none; got '{name}'",
                    draft
                        .access_key_options
                        .iter()
                        .map(|o| o.position.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }),
    }
}

fn run_apply_preset(options: &Options) -> Result<u8, Failure> {
    let mut session = open_session(options)?;
    let profile = options.profile.ok_or_else(|| {
        Failure::Usage("apply-preset needs --profile N (1-9); profile 0 is not programmable".into())
    })?;
    if !profile.is_editable() {
        return Err(Failure::Core(CoreError::ProfileZeroNotEditable));
    }
    let which = options.preset.as_deref().unwrap_or("both");
    let draft = presets::build_draft();

    let mut applied = 0usize;
    if which == "symbols" || which == "both" {
        applied += session.apply_preset(profile, &draft.symbols)?;
    }
    if which == "navigation" || which == "both" {
        applied += session.apply_preset(profile, &draft.navigation)?;
        if let Some(option) = select_access_key(&draft, options.access_key.as_deref())? {
            session.apply_access_key(profile, &option)?;
            applied += 1;
        }
    }
    if !matches!(which, "symbols" | "navigation" | "both") {
        return Err(Failure::Usage(format!(
            "--preset expects symbols, navigation or both; got '{which}'"
        )));
    }

    println!("Built a draft for {profile}: {applied} assignment(s).");
    println!();
    println!("Review sheet:");
    println!("{}", presets::render_review_sheet(&draft, session.layout()));
    println!("---");
    let diff = session.diff();
    println!(
        "{} line(s) in {} file(s) would change.",
        diff.changed_line_count(),
        diff.files
            .iter()
            .filter(|f| !f.changed_lines.is_empty())
            .count()
    );
    for file in &diff.files {
        if !file.changed_lines.is_empty() {
            println!(
                "  {}: line(s) {}",
                file.relative_path,
                file.changed_lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    let report = session.validation_for_write(profile);
    println!(
        "validation: {} error(s), {} warning(s)",
        report.error_count(),
        report.warning_count()
    );
    if report.blocks_write() {
        for f in report.errors() {
            println!(
                "  error {}:{} {}",
                f.relative_path,
                f.line.map(|l| l.to_string()).unwrap_or_else(|| "-".into()),
                f.message
            );
        }
    }

    if !options.write {
        println!();
        println!("Nothing was written. Add --write --yes to send this draft to the keyboard.");
        return Ok(EXIT_OK);
    }
    if !options.yes {
        return Err(Failure::Usage(
            "--write also needs --yes, which is the explicit confirmation that the operator has \
             read the change list above"
                .to_string(),
        ));
    }

    let changed =
        session.file_change_count(session.profile_file_index(profile).ok_or_else(|| {
            Failure::Core(CoreError::ReadOnlyFile {
                path: PathBuf::from(profile.layout_file_name()),
                role: "no layout file for this profile",
            })
        })?);
    let backup = session.backup(&[profile])?;
    println!();
    println!("Backup taken first: {}", backup.directory.display());

    let outcome = session.write_profile(WriteConfirmation {
        profile,
        confirmed_by_operator: true,
        acknowledged_changed_lines: changed,
    })?;

    println!(
        "Wrote {} ({} bytes), verified: {}",
        outcome.relative_path, outcome.bytes_written, outcome.verified
    );
    println!("Backup: {}", outcome.backup_directory.display());
    for step in &outcome.next_steps {
        println!("  {step}");
    }
    println!();
    println!("Before closing the v-Drive from the keyboard:");
    for step in &outcome.eject_instructions {
        println!("  {step}");
    }
    Ok(EXIT_OK)
}

fn run_restore(options: &Options) -> Result<u8, Failure> {
    let session = open_session(options)?;
    let dir = options.from_backup.clone().ok_or_else(|| {
        Failure::Usage("restore needs --from-backup DIR (see the 'backups' command)".to_string())
    })?;
    let set = adv360_core::backup::read_manifest(&dir)?;
    let wanted: Vec<String> = set
        .files
        .iter()
        .filter(|f| match options.profile {
            Some(p) => {
                f.relative_path.contains(&format!("layout{}.txt", p.0))
                    || f.relative_path.contains(&format!("led{}.txt", p.0))
            }
            None => true,
        })
        .map(|f| f.relative_path.clone())
        .collect();
    if wanted.is_empty() {
        return Err(Failure::Core(CoreError::Other(format!(
            "backup {} contains nothing for the requested profile",
            dir.display()
        ))));
    }
    println!("Restoring from {} ({}):", dir.display(), set.created_at);
    for path in &wanted {
        println!("  {path}");
    }
    if !options.yes {
        return Err(Failure::Usage(
            "restore also needs --yes: it writes files back to the v-Drive".to_string(),
        ));
    }
    let restored = session.restore(&set, &wanted)?;
    println!();
    println!("Restored {} file(s).", restored.len());
    for step in device::eject_instructions(&session.device().root) {
        println!("  {step}");
    }
    Ok(EXIT_OK)
}

/// Create a synthetic v-Drive so the interfaces can be explored with no keyboard attached.
///
/// The files are empty apart from the five documented layer headers, which is exactly what the
/// guide says an unmodified layout file looks like. Nothing here can overwrite real data: the
/// command refuses to write into a directory that already contains a layouts folder unless --yes
/// is given.
fn run_fixture(options: &Options) -> Result<u8, Failure> {
    let out = options
        .out
        .clone()
        .ok_or_else(|| Failure::Usage("fixture needs --out DIR".to_string()))?;
    let layouts = out.join("layouts");
    if layouts.exists() && !options.yes {
        return Err(Failure::Usage(format!(
            "{} already contains a layouts folder; pass --yes if you really want to add the \
             missing files to it",
            out.display()
        )));
    }
    for dir in [&layouts, &out.join("lighting")] {
        std::fs::create_dir_all(dir).map_err(|source| CoreError::CreateDir {
            path: dir.clone(),
            source,
        })?;
    }
    let headers: String = LayerId::all()
        .iter()
        .map(|l| format!("{}\n", l.header()))
        .collect();
    for profile in ProfileId::programmable() {
        let path = layouts.join(profile.layout_file_name());
        if !path.exists() {
            std::fs::write(&path, &headers).map_err(|source| CoreError::Write {
                path: path.clone(),
                source,
            })?;
        }
    }
    let settings = out.join("settings.txt");
    if !settings.exists() {
        std::fs::write(
            &settings,
            "# Synthetic fixture created by adv360-config fixture.\n\
             # This is not a real keyboard; nothing here will ever be sent to hardware.\n\
             dummy=1\n",
        )
        .map_err(|source| CoreError::Write {
            path: settings.clone(),
            source,
        })?;
    }
    let led = out.join("lighting/led1.txt");
    if !led.exists() {
        std::fs::write(&led, "[IND1]>[caps][255][000][000]\n").map_err(|source| {
            CoreError::Write {
                path: led.clone(),
                source,
            }
        })?;
    }

    println!("Synthetic v-Drive created at {}", out.display());
    println!("  layouts/layout1.txt .. layout9.txt (five documented layer headers each)");
    println!("  lighting/led1.txt");
    println!("  settings.txt (a dummy file, never written by this tool)");
    println!();
    println!("Explore both interfaces against it, with no keyboard attached:");
    println!("  {APP_NAME} --device {} tui", out.display());
    println!("  {APP_NAME} --device {} gui", out.display());
    println!();
    println!("Or try the presets and the whole review workflow:");
    println!(
        "  {APP_NAME} --device {} apply-preset --profile 4 --preset both",
        out.display()
    );
    println!();
    println!("This is a directory of ordinary text files. Nothing in it is real keyboard data,");
    println!("and the tool still refuses to write to it without --write --yes.");
    Ok(EXIT_OK)
}

fn run_mount_help(options: &Options) -> Result<u8, Failure> {
    println!("Opening the v-Drive from the keyboard:");
    for step in device::mount_instructions() {
        println!("  - {step}");
    }
    println!();
    match open_session(options) {
        Ok(session) => {
            println!(
                "A v-Drive is available at {}.",
                session.device().root.display()
            );
            println!();
            println!("Ejecting it safely before closing it from the keyboard:");
            for step in device::eject_instructions(&session.device().root) {
                println!("  - {step}");
            }
        }
        Err(Failure::Core(_)) => {
            println!("No v-Drive is currently available.");
            println!();
            println!("Once it is mounted, eject it safely before closing it from the keyboard:");
            for step in device::eject_instructions(std::path::Path::new("/run/media/$USER/ADV360"))
            {
                println!("  - {step}");
            }
        }
        Err(other) => return Err(other),
    }
    Ok(EXIT_OK)
}
