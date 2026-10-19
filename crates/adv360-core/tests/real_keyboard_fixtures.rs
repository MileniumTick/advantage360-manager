//! Fixtures taken verbatim from a real, factory Advantage360 SmartSet v-Drive.
//!
//! Every string below was copied byte for byte from the mounted keyboard at
//! `/run/media/$USER/ADV360`, captured with `od`/`cat -A` so nothing is retyped from memory. They
//! exist because the real files disagreed with the documentation in four places, and a test suite
//! that only ever saw files this tool wrote itself could not have caught any of them:
//!
//! 1. An unused layout file is **nine** lines, not five: the five documented layer headers are
//!    separated by blank lines, and every line ends with CRLF.
//! 2. The layer indicator carries **one line per layer** for the same indicator, which the guide
//!    asks for ("up to 5 colors, one for each layer") but which a naive duplicate check flags.
//! 3. The real firmware writes `[lay3]` for the Fn3 layer LED, while the guide prints `[lay]`.
//! 4. `settings.txt` lives in a `settings/` subfolder and uses a `key=value` format this tool has
//!    no business interpreting.

use adv360_core::layer::LayerId;
use adv360_core::model::{FileRole, LedFunction, LineKind};
use adv360_core::parser;
use adv360_core::validate::{self, Severity};

/// Exactly what the keyboard writes into `layouts/layoutN.txt` for a profile with no custom remaps:
/// 65 bytes, 9 lines, CRLF throughout, no BOM, a trailing terminator.
pub const UNUSED_LAYOUT: &str =
    "<base>\r\n\r\n<keypad>\r\n\r\n<function1>\r\n\r\n<function2>\r\n\r\n<function3>\r\n";

/// `lighting/led1.txt` as the keyboard writes it: 384 bytes, 14 lines, CRLF, lowercase `[indN]`,
/// and `[lay3]` for Fn3.
pub const LIGHTING_LED1: &str = "\
[ind1]>[caps][255][255][255]\r\n\
[ind2]>[prof][255][255][255]\r\n\
[ind3]>[layd][0][0][0]\r\n\
[ind3]>[layk][255][255][255]\r\n\
[ind3]>[lay1][0][0][255]\r\n\
[ind3]>[lay2][0][255][0]\r\n\
[ind3]>[lay3][255][0][0]\r\n\
[ind4]>[nmlk][255][255][255]\r\n\
[ind5]>[sclk][255][255][255]\r\n\
[ind6]>[layd][0][0][0]\r\n\
[ind6]>[layk][255][255][255]\r\n\
[ind6]>[lay1][0][0][255]\r\n\
[ind6]>[lay2][0][255][0]\r\n\
[ind6]>[lay3][255][0][0]\r\n";

/// `settings/settings.txt`, 106 bytes, 9 lines, CRLF. This tool reads it and never writes it.
pub const SETTINGS: &str = "\
profile=1\r\n\
led=0\r\n\
country=us\r\n\
status=2\r\n\
lock=off\r\n\
model=adv360\r\n\
kbd_fw_l=1.0.69\r\n\
kbd_fw_r=1.0.69\r\n\
bl=1.3\r\n";

fn parse(text: &str, relative_path: &str, role: FileRole) -> adv360_core::model::ConfigFile {
    parser::parse_bytes(text.as_bytes(), relative_path, role).expect("the real file must parse")
}

#[test]
fn an_unused_layout_file_round_trips_byte_for_byte() {
    let file = parse(
        UNUSED_LAYOUT,
        "layouts/layout2.txt",
        FileRole::ProfileLayout,
    );
    assert_eq!(UNUSED_LAYOUT.len(), 65, "the real file is 65 bytes");
    assert_eq!(file.lines.len(), 9, "five headers and four blank lines");
    assert_eq!(
        file.render(),
        UNUSED_LAYOUT,
        "the blank lines and the CRLF endings must survive untouched"
    );
    assert!(file.has_crlf());
    assert!(!file.has_mixed_eol());
    assert!(!file.lacks_final_newline());

    // And the layer of every line must be resolved, so a remap added under <base> lands correctly.
    let layers: Vec<Option<LayerId>> = file.lines.iter().map(|l| l.layer).collect();
    assert_eq!(
        layers,
        vec![
            Some(LayerId::Base),
            Some(LayerId::Base),
            Some(LayerId::Keypad),
            Some(LayerId::Keypad),
            Some(LayerId::Function1),
            Some(LayerId::Function1),
            Some(LayerId::Function2),
            Some(LayerId::Function2),
            Some(LayerId::Function3),
        ]
    );
}

#[test]
fn a_real_lighting_file_round_trips_and_is_not_reported_as_full_of_duplicates() {
    let file = parse(
        LIGHTING_LED1,
        "lighting/led1.txt",
        FileRole::ProfileLighting,
    );
    assert_eq!(LIGHTING_LED1.len(), 384, "the real file is 384 bytes");
    assert_eq!(file.lines.len(), 14);
    assert_eq!(file.render(), LIGHTING_LED1);

    let report = validate::validate_file(&file);
    assert_eq!(
        report.error_count(),
        0,
        "a factory lighting file must produce no errors: {:#?}",
        report.errors().collect::<Vec<_>>()
    );
    assert!(
        !report.findings.iter().any(|f| f.code == "duplicate-led"),
        "the layer indicator legitimately carries one line per layer, so this must not be a \
         duplicate: {:#?}",
        report.findings
    );
}

#[test]
fn the_layer_indicator_is_recognised_for_all_five_layers_including_fn3() {
    let file = parse(
        LIGHTING_LED1,
        "lighting/led1.txt",
        FileRole::ProfileLighting,
    );
    let layers: Vec<LayerId> = file
        .lines
        .iter()
        .filter_map(|l| match &l.kind {
            LineKind::Led(led) => match led.function {
                LedFunction::Layer(layer) => Some(layer),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for layer in LayerId::all() {
        assert!(
            layers.contains(layer),
            "{layer:?} must be recognised; the real files write [lay3] for Fn3"
        );
    }
    assert!(file
        .lines
        .iter()
        .any(|l| l.raw.to_ascii_lowercase().contains("[lay3]")));
}

#[test]
fn fn3_is_written_as_lay3_and_the_guides_lay_is_still_accepted_on_read() {
    assert_eq!(LedFunction::Layer(LayerId::Function3).token(), "lay3");
    // A file handwritten from the guide's text must still be understood.
    assert_eq!(
        LedFunction::parse("lay"),
        Some(LedFunction::Layer(LayerId::Function3))
    );
    let handwritten = parse(
        "[IND3]>[lay][255][000][000]\r\n",
        "lighting/led1.txt",
        FileRole::ProfileLighting,
    );
    assert!(matches!(
        handwritten.lines[0].kind,
        LineKind::Led(ref led) if led.function == LedFunction::Layer(LayerId::Function3)
    ));
}

#[test]
fn a_real_duplicate_indicator_assignment_is_still_reported() {
    // The layer LED may repeat per layer, but the same indicator set to the same thing twice is a
    // real mistake and must stay visible.
    let text = "[ind1]>[caps][255][255][255]\r\n[ind1]>[caps][000][000][000]\r\n";
    let file = parse(text, "lighting/led1.txt", FileRole::ProfileLighting);
    let report = validate::validate_file(&file);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.code == "duplicate-led" && f.severity == Severity::Warning),
        "{:#?}",
        report.findings
    );
}

#[test]
fn settings_txt_is_read_and_never_interpreted_line_by_line() {
    let file = parse(SETTINGS, "settings/settings.txt", FileRole::Settings);
    assert_eq!(SETTINGS.len(), 106, "the real file is 106 bytes");
    assert_eq!(file.render(), SETTINGS, "it must round-trip byte for byte");
    assert_eq!(file.role, FileRole::Settings);
    assert!(!file.role.is_writable());

    let report = validate::validate_file(&file);
    assert!(
        report.findings.iter().any(|f| f.code == "role-read-only"),
        "the operator must be told this file is never written"
    );
    assert!(
        !report.findings.iter().any(|f| f.line.is_some()),
        "its key=value lines are not configuration this tool understands, so it must not comment \
         on them line by line: {:#?}",
        report.findings
    );
}

#[test]
fn the_firmware_version_from_the_real_settings_file_is_visible_to_the_operator() {
    // The keyboard's own settings file states the firmware, and the Direct Programming Guide warns
    // that some features may require an update, so reading it is how the operator can tell.
    let file = parse(SETTINGS, "settings/settings.txt", FileRole::Settings);
    let firmware: Vec<&str> = file
        .lines
        .iter()
        .map(|l| l.raw.as_str())
        .filter(|l| l.starts_with("kbd_fw_") || l.starts_with("bl=") || l.starts_with("model="))
        .collect();
    assert_eq!(
        firmware,
        vec![
            "model=adv360",
            "kbd_fw_l=1.0.69",
            "kbd_fw_r=1.0.69",
            "bl=1.3"
        ]
    );
}

/// The first two lines of the real profile 1, byte for byte, including the 0xF3 that makes the file
/// ISO-8859-1 rather than UTF-8. This is what the keyboard had on it, and it is why profile 1 never
/// worked with this tool: the file is skipped rather than decoded wrongly.
pub const PROFILE1_LATIN1_PREFIX: &[u8] =
    b"# =====================================================================\r\n\
# KINESIS ADVANTAGE360 - PROFILE 1: OPTIMIZACI\xf3N DEL CLUSTER\r\n";

#[test]
fn a_latin1_layout_file_is_skipped_loudly_and_never_guessed_at() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ADV360");
    std::fs::create_dir_all(root.join("layouts")).unwrap();
    std::fs::create_dir_all(root.join("lighting")).unwrap();
    for profile in 1..=9 {
        let path = root.join(format!("layouts/layout{profile}.txt"));
        if profile == 1 {
            std::fs::write(&path, PROFILE1_LATIN1_PREFIX).unwrap();
        } else {
            std::fs::write(&path, UNUSED_LAYOUT).unwrap();
        }
    }
    std::fs::write(root.join("settings.txt"), SETTINGS).unwrap();

    let mut session = adv360_core::session::Session::open_at(
        &root,
        adv360_core::session::SessionOptions {
            backup_root: dir.path().join("backups"),
            layout_override: Some(("us".to_string(), None)),
            ..Default::default()
        },
    )
    .unwrap();

    // It is skipped, and the reason is reported rather than swallowed.
    assert_eq!(session.skipped_files().len(), 1);
    assert!(session.skipped_files()[0].0.ends_with("layout1.txt"));
    assert!(session.skipped_files()[0].1.contains("not valid UTF-8"));

    // A clean report that omitted it would be a lie, so it appears in the validation output.
    let report = session.validation();
    let unreadable = report
        .findings
        .iter()
        .find(|f| f.code == "file-unreadable")
        .expect("the operator must be told that a file was skipped");
    assert!(unreadable.relative_path.ends_with("layout1.txt"));
    assert!(unreadable.hint.is_some());

    // The file is untouched: not converted, not truncated, not rewritten.
    assert_eq!(
        std::fs::read(root.join("layouts/layout1.txt")).unwrap(),
        PROFILE1_LATIN1_PREFIX
    );

    // Trying to write that profile explains the real reason instead of claiming there is no file.
    session.backup(&[adv360_core::layer::ProfileId(1)]).unwrap();
    let error = session
        .write_profile(adv360_core::session::WriteConfirmation {
            profile: adv360_core::layer::ProfileId(1),
            confirmed_by_operator: true,
            acknowledged_changed_lines: 0,
        })
        .expect_err("writing a file that could not be read must fail");
    assert!(
        error.to_string().contains("could not be read"),
        "the error must name the real cause, got: {error}"
    );
    assert_eq!(
        std::fs::read(root.join("layouts/layout1.txt")).unwrap(),
        PROFILE1_LATIN1_PREFIX
    );

    // And another profile is still fully usable, which is what matters to the operator.
    session
        .set_binding(
            adv360_core::layer::ProfileId(4),
            adv360_core::layer::LayerId::Base,
            "hk1",
            Some(adv360_core::presets::Assignment::Remap {
                position: "hk1".into(),
                action: "q".into(),
            }),
        )
        .unwrap();
    let changed = session.file_change_count(
        session
            .profile_file_index(adv360_core::layer::ProfileId(4))
            .unwrap(),
    );
    assert_eq!(
        changed, 1,
        "one line in the profile that does have a readable file"
    );
}
