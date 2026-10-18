//! Tests that drive [`App`] through [`run_headless`] and assert on the rendered frame text.
//!
//! There is no terminal here: every frame comes from `ratatui::backend::TestBackend`, so the tests
//! cover the same state machine `run` wires to Crossterm.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use adv360_core::prelude::*;
use crossterm::event::KeyCode;

use crate::{run_headless, Msg};

struct TestDrive {
    base: PathBuf,
}

impl Drop for TestDrive {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn unique_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("adv360-tui-test-{}-{nanos}", std::process::id()))
}

fn fixture() -> (TestDrive, Session) {
    let base = unique_dir();
    let root = base.join("ADV360");
    fs::create_dir_all(root.join("layouts")).expect("layouts dir");
    fs::create_dir_all(root.join("lighting")).expect("lighting dir");
    fs::write(root.join("settings.txt"), "never touch\n").expect("settings");
    let headers: String = LayerId::all()
        .iter()
        .map(|layer| format!("{}\n", layer.header()))
        .collect();
    for profile in ProfileId::programmable() {
        fs::write(
            root.join(format!("layouts/layout{}.txt", profile.0)),
            headers.clone(),
        )
        .expect("layout file");
    }
    fs::write(root.join("lighting/led1.txt"), "[IND1]>[caps]\n").expect("led file");
    let session = Session::open_at(
        &root,
        SessionOptions {
            backup_root: base.join("backups"),
            layout_override: Some(("us".to_string(), None)),
            ..Default::default()
        },
    )
    .expect("open fixture v-Drive");
    (TestDrive { base }, session)
}

fn ch(c: char) -> Msg {
    Msg::text(c)
}

fn key(code: KeyCode) -> Msg {
    Msg::key(code)
}

fn text(value: &str) -> Vec<Msg> {
    value.chars().map(Msg::text).collect()
}

fn last(frames: &[String]) -> &str {
    frames.last().expect("at least one frame")
}

#[test]
fn run_headless_switches_profile_and_layer() {
    let (_drive, session) = fixture();
    let frames = run_headless(session, &[ch('p'), ch('l')], (90, 28));
    assert_eq!(frames.len(), 3, "initial frame plus one per message");
    assert!(last(&frames).contains("Profile 2"), "{}", last(&frames));
    assert!(last(&frames).contains("Layer Keypad"), "{}", last(&frames));
}

#[test]
fn run_headless_moves_the_key_cursor() {
    let (_drive, session) = fixture();
    let frames = run_headless(session, &[key(KeyCode::Right)], (90, 28));
    assert!(last(&frames).contains("Key: 1"), "{}", last(&frames));
}

#[test]
fn run_headless_searches_catalogue_assigns_and_undoes() {
    let (_drive, session) = fixture();
    let mut script = vec![ch('/')];
    script.extend(text("caps"));
    script.extend([key(KeyCode::Enter), key(KeyCode::Enter)]);
    script.push(ch('u'));
    let frames = run_headless(session, &script, (90, 28));

    assert!(
        frames.iter().any(|frame| frame.contains("Search: 'caps'")),
        "the search needle must be visible"
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("Key: eql [custom]")),
        "the assignment must make the key custom"
    );
    assert!(last(&frames).contains("Undone"), "{}", last(&frames));
}

#[test]
fn run_headless_reviews_preset_drafts_without_applying_them() {
    let (_drive, session) = fixture();
    let frames = run_headless(session, &[ch('g')], (120, 40));
    let screen = last(&frames);
    assert!(screen.contains("Programming symbols"), "{screen}");
    assert!(
        screen.contains("Nothing here is applied until"),
        "the review-only rule must be on screen"
    );
}

#[test]
fn run_headless_shows_the_diff_and_the_help() {
    let (_drive, session) = fixture();
    let frames = run_headless(session, &[ch('d'), ch('?')], (90, 28));
    assert!(
        frames[1].contains("Differences") || frames[1].contains("No differences"),
        "{}",
        frames[1]
    );
    let help = last(&frames);
    // The panel wraps long lines, so assert on the words rather than on one contiguous phrase.
    assert!(
        help.contains("Advantage360 SmartSet Direct") && help.contains("Programming Guide"),
        "the help must list the documentation sources:\n{help}"
    );
    assert!(help.contains("kinesis-ergo.com"), "{help}");
}

#[test]
fn run_headless_distinguishes_token_and_character_in_the_layout_panel() {
    let (_drive, session) = fixture();
    let frames = run_headless(session, &[ch('y')], (90, 28));
    let screen = last(&frames);
    assert!(screen.contains("XKB layout: us"), "{screen}");
    assert!(
        screen.contains("token is what the keyboard sends"),
        "{screen}"
    );
}

#[test]
fn run_headless_requires_a_typed_write_confirmation() {
    let (_drive, session) = fixture();
    let mut script = vec![ch('/')];
    script.extend(text("caps"));
    script.extend([
        key(KeyCode::Enter),
        key(KeyCode::Enter),
        ch('b'),
        ch('w'),
        key(KeyCode::Enter),
        key(KeyCode::Enter),
        key(KeyCode::Enter),
    ]);
    script.extend(text("WRITE"));
    script.push(key(KeyCode::Enter));
    let frames = run_headless(session, &script, (100, 32));

    let rejected = frames
        .iter()
        .position(|frame| frame.contains("Confirmation not accepted"))
        .expect("an untyped Enter must be rejected");
    assert!(
        !frames[rejected].contains("WRITE VERIFIED"),
        "a write must never happen without the typed confirmation"
    );
    assert!(
        last(&frames).contains("WRITE VERIFIED"),
        "{}",
        last(&frames)
    );
    assert!(
        last(&frames).contains("Backup location:"),
        "{}",
        last(&frames)
    );
}

#[test]
fn run_headless_displays_core_errors_with_recovery_hints() {
    let (_drive, session) = fixture();
    let mut script = vec![ch('/')];
    script.extend(text("caps"));
    script.extend([key(KeyCode::Enter), key(KeyCode::Enter), ch('w')]);
    script.push(key(KeyCode::Enter));
    script.extend(text("WRITE"));
    script.push(key(KeyCode::Enter));
    let frames = run_headless(session, &script, (100, 32));
    let screen = last(&frames);
    assert!(
        screen.contains("backup step first"),
        "the BackupRequired recovery hint must be displayed: {screen}"
    );
}

#[test]
fn run_headless_never_panics_on_narrow_or_short_terminals() {
    for size in [(40, 12), (80, 4), (20, 3), (120, 8), (56, 16)] {
        let (_drive, session) = fixture();
        let frames = run_headless(
            session,
            &[ch('p'), ch('l'), ch('g'), ch('d'), ch('?'), ch('w')],
            size,
        );
        assert!(!frames.is_empty());
        for frame in &frames {
            assert!(
                frame.lines().count() >= 1,
                "every frame must have content at {size:?}"
            );
        }
    }
}

#[test]
fn run_headless_honours_quit() {
    let (_drive, session) = fixture();
    let frames = run_headless(session, &[Msg::Quit], (40, 12));
    assert_eq!(frames.len(), 2);
}

/// A tiny sanity check that the fixture really opened a v-Drive, so a failure above is not hidden.
#[test]
fn the_fixture_has_all_nine_profiles() {
    let (_drive, session) = fixture();
    assert_eq!(session.available_profiles().len(), 9);
    assert_eq!(session.profile_summary().len(), 10);
    assert!(!session.profile_summary()[0].editable);
}

/// Print the real rendered frames, so the terminal interface can be reviewed without a terminal.
///
/// This is the evidence that the TUI is not a mock-up: the frames below come from the same state
/// machine and the same `draw` call the live interface uses, rendered into Ratatui's test backend.
/// Run with `cargo test -p adv360-tui -- --nocapture prints_the_real_frames`.
#[test]
fn prints_the_real_frames_for_review() {
    let (width, height) = (118u16, 34u16);

    let wanted: Vec<(&str, Vec<Msg>)> = vec![
        ("OPEN — the key map and the help legend", vec![]),
        ("HELP — the documented sources", vec![ch('?')]),
        (
            "CATALOGUE — searching for a symbol action",
            vec![key(KeyCode::Esc), ch('/'), ch('o'), ch('b')],
        ),
        (
            "DIFF — an assignment and its pending change",
            vec![
                key(KeyCode::Esc),
                ch('/'),
                ch('c'),
                ch('a'),
                ch('p'),
                ch('s'),
                key(KeyCode::Enter),
                key(KeyCode::Enter),
                ch('d'),
            ],
        ),
        (
            "VALIDATION — findings with line numbers",
            vec![key(KeyCode::Esc), ch('v')],
        ),
        (
            "PRESETS — the review-only draft",
            vec![key(KeyCode::Esc), ch('g')],
        ),
        (
            "LAYOUT — token versus character",
            vec![key(KeyCode::Esc), ch('y')],
        ),
        (
            "NARROW — a 46x14 terminal still renders",
            vec![key(KeyCode::Esc)],
        ),
    ];

    for (title, script) in wanted {
        let (w, h) = if title.starts_with("NARROW") {
            (46u16, 14u16)
        } else {
            (width, height)
        };
        // A fresh session per screen keeps each frame independent of the previous script.
        let (_drive, session) = fixture();
        let frames = run_headless(session, &script, (w, h));
        let frame = last(&frames);
        println!("\n{}", "=".repeat(usize::from(w)));
        println!("== {title}   [{w}x{h}]");
        println!("{}", "=".repeat(usize::from(w)));
        println!("{frame}");
        // A frame must never be empty and must never contain a control artefact.
        assert!(!frame.trim().is_empty(), "{title} rendered nothing");
        assert!(
            !frame.contains("panic"),
            "{title} rendered a panic: {frame}"
        );
    }
}
