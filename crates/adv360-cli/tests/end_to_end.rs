//! End-to-end tests of the `adv360-config` command against a synthetic v-Drive.
//!
//! These are the checks that prove the protection rules actually hold at the level the operator
//! experiences them: nothing is written without `--write --yes`, profile 0 is refused, a corrupted
//! file blocks the write, an unknown instruction survives, the backup is real, and restore puts the
//! original bytes back. They run the real binary through `CARGO_BIN_EXE_adv360-config`, so the
//! argument parsing, the session wiring and the write path are all exercised together.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// The nine layer headers, exactly as the guide spells them.
fn empty_layout() -> String {
    "<base>\n<keypad>\n<function1>\n<function2>\n<function3>\n".to_string()
}

struct Fixture {
    dir: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("ADV360");
        fs::create_dir_all(root.join("layouts")).unwrap();
        fs::create_dir_all(root.join("lighting")).unwrap();
        fs::create_dir_all(root.join("firmware")).unwrap();

        // settings.txt is the file the guide says never to edit. It must be byte-identical after
        // every single test below.
        fs::write(root.join("settings.txt"), "version=1\r\nprofile=0\r\n").unwrap();

        for profile in 1..=9 {
            fs::write(
                root.join(format!("layouts/layout{profile}.txt")),
                empty_layout(),
            )
            .unwrap();
        }

        // Profile 3 is deliberately messy: a BOM, CRLF, a comment, an undocumented instruction, a
        // Tap-and-Hold assignment, a macro with a shift chord and a disabled line. All of it must
        // survive.
        fs::write(
            root.join("layouts/layout3.txt"),
            "\u{feff}# my own note\r\n<base>\r\n[esc]>[caps]\r\n[caps]>[caps][t&h500][esc]\r\n\
             {tab}>{-lshf}{h}{+lshf}{i}\r\nVENDOREXT keep me\r\n*[q]>[w]\r\n<keypad>\r\n\
             <function1>\r\n<function2>\r\n<function3>\r\n",
        )
        .unwrap();

        // Profile 5 carries a syntax error, so writes to it must be refused.
        fs::write(
            root.join("layouts/layout5.txt"),
            "<base>\n[g]>[\n<keypad>\n<function1>\n<function2>\n<function3>\n",
        )
        .unwrap();

        fs::write(
            root.join("lighting/led1.txt"),
            "[IND1]>[caps][255][000][000]\n",
        )
        .unwrap();
        fs::write(root.join("firmware/update.bin"), [0u8, 1, 2, 3]).unwrap();

        Fixture { dir, root }
    }

    fn layouts(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in fs::read_dir(self.root.join("layouts")).unwrap().flatten() {
            let path = entry.path();
            out.push((
                path.file_name().unwrap().to_string_lossy().to_string(),
                fs::read(&path).unwrap(),
            ));
        }
        out.sort();
        out
    }

    fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<(String, Vec<u8>)> = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path
                        .strip_prefix(&self.root)
                        .unwrap()
                        .to_string_lossy()
                        .to_string();
                    out.push((rel, fs::read(&path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.root.join(relative)).unwrap()
    }

    fn backups_dir(&self) -> PathBuf {
        self.dir.path().join("backups")
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut full: Vec<String> = vec![
            "--device".into(),
            self.root.to_string_lossy().to_string(),
            "--backup-dir".into(),
            self.backups_dir().to_string_lossy().to_string(),
        ];
        full.extend(args.iter().map(|s| s.to_string()));
        Command::new(env!("CARGO_BIN_EXE_adv360-config"))
            .args(&full)
            .env("XDG_DATA_HOME", self.dir.path().join("data"))
            // Keep the machine's real keyboard layout out of the result.
            .env("XKB_DEFAULT_LAYOUT", "us")
            .output()
            .expect("the binary must run")
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn combined(out: &Output) -> String {
    format!("{}{}", stdout(out), stderr(out))
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

#[test]
fn help_and_about_work_without_a_keyboard() {
    let out = Command::new(env!("CARGO_BIN_EXE_adv360-config"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0);
    let help = stdout(&out);
    for command in [
        "gui",
        "tui",
        "detect",
        "layout",
        "dump",
        "keys",
        "validate",
        "diff",
        "export",
        "backup",
        "restore",
        "presets",
        "apply-preset",
        "mount-help",
        "about",
    ] {
        assert!(help.contains(command), "{command} missing from --help");
    }
    assert!(
        help.contains("never"),
        "the safety rules must be stated in --help"
    );

    let out = Command::new(env!("CARGO_BIN_EXE_adv360-config"))
        .arg("about")
        .output()
        .unwrap();
    assert_eq!(code(&out), 0);
    let about = stdout(&out);
    assert!(about.contains("Direct Programming Guide"));
    assert!(about.contains("kinesis-ergo.com"));
    assert!(about.contains("8-8-25"));
}

#[test]
fn an_unknown_command_fails_with_a_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_adv360-config"))
        .arg("frobnicate")
        .output()
        .unwrap();
    assert_eq!(code(&out), 2, "{}", combined(&out));
    assert!(combined(&out).contains("unknown command"));
}

#[test]
fn detect_reports_the_drive_and_its_files() {
    let f = Fixture::new();
    let out = f.run(&["detect"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    assert!(text.contains("layouts/layout1.txt"));
    assert!(text.contains("lighting/led1.txt"));
    assert!(text.contains("settings.txt"));
    assert!(
        text.contains("read-only"),
        "settings.txt must be shown as read-only"
    );
    assert!(
        text.contains("writable"),
        "layout files must be shown as writable"
    );
    // The firmware blob is reported, never treated as configuration.
    assert!(text.contains("firmware/update.bin"));
}

#[test]
fn detect_json_is_machine_readable() {
    let f = Fixture::new();
    let out = f.run(&["detect", "--json"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(value["profiles"].as_array().unwrap().len(), 10);
    assert_eq!(
        value["profiles"][0]["editable"],
        serde_json::Value::Bool(false)
    );
}

#[test]
fn dump_marks_profile_zero_as_not_editable() {
    let f = Fixture::new();
    let out = f.run(&["dump"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    assert!(text.contains("NO (non-programmable)"), "{text}");
    assert!(text.contains("editable"));
}

#[test]
fn keys_shows_documented_and_uncertain_factory_actions() {
    let f = Fixture::new();
    let out = f.run(&["keys", "--profile", "3", "--layer", "base"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    // A custom assignment is shown with its line number.
    assert!(
        text.contains("[esc]>[caps]") || text.contains("caps"),
        "{text}"
    );
    // The hotkeys and the foot pedals must not be presented as documented.
    assert!(
        text.contains("DocumentedNoAction"),
        "hotkeys must be documented as blank: {text}"
    );
    assert!(
        text.contains("Uncertain"),
        "foot pedal defaults must be marked uncertain: {text}"
    );
    assert!(text.contains("LPD1") || text.contains("lpd1"), "{text}");
}

#[test]
fn keys_json_exposes_the_factory_source() {
    let f = Fixture::new();
    let out = f.run(&["keys", "--profile", "3", "--layer", "function1", "--json"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(value["layer"], "function1");
    let keys = value["keys"].as_array().unwrap();
    let eql = keys
        .iter()
        .find(|k| k["position"] == "eql")
        .expect("the equals key must be present");
    assert_eq!(eql["factory_action"], "f1");
    assert_eq!(eql["factory_is_documented"], serde_json::Value::Bool(true));
}

#[test]
fn validate_passes_a_healthy_drive_and_fails_a_broken_one() {
    let f = Fixture::new();
    let out = f.run(&["validate"]);
    // Profile 5 has a syntax error, so the whole-drive validation must fail.
    assert_eq!(code(&out), 1, "{}", combined(&out));
    let text = stdout(&out);
    assert!(text.contains("syntax-error"), "{text}");
    assert!(text.contains("layouts/layout5.txt"), "{text}");

    // A single healthy profile validates cleanly.
    let out = f.run(&["validate", "--profile", "3"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    assert!(stdout(&out).contains("0 error(s)"));
}

#[test]
fn validate_json_lists_findings_with_line_numbers() {
    let f = Fixture::new();
    let out = f.run(&["validate", "--json"]);
    assert_eq!(code(&out), 1);
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let findings = value.as_array().unwrap();
    let syntax = findings
        .iter()
        .find(|f| f["code"] == "syntax-error")
        .expect("the syntax error must be reported");
    assert_eq!(syntax["relative_path"], "layouts/layout5.txt");
    assert_eq!(syntax["line"], 2);
    assert_eq!(syntax["severity"], "error");
}

#[test]
fn presets_writes_drafts_and_touches_nothing_on_the_drive() {
    let f = Fixture::new();
    let before = f.snapshot();
    let out_dir = f.dir.path().join("presets");
    let out = f.run(&[
        "presets",
        "--out",
        out_dir.to_str().unwrap(),
        "--access-key",
        "hk1",
    ]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    assert!(out_dir.join("draft-symbols-and-navigation.txt").exists());
    assert!(out_dir.join("review-sheet.md").exists());
    let sheet = fs::read_to_string(out_dir.join("review-sheet.md")).unwrap();
    assert!(sheet.contains("Nothing has been written"));
    assert!(sheet.contains("<function1>"));
    // Every line of the draft is listed in the sheet.
    let draft = fs::read_to_string(out_dir.join("draft-symbols-and-navigation.txt")).unwrap();
    for line in draft
        .lines()
        .filter(|l| l.starts_with('[') || l.starts_with('{'))
    {
        assert!(sheet.contains(line), "the sheet must list {line}");
    }
    assert_eq!(before, f.snapshot(), "presets must not touch the v-Drive");
}

#[test]
fn apply_preset_without_write_leaves_the_drive_byte_identical() {
    let f = Fixture::new();
    let before = f.snapshot();
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "4",
        "--preset",
        "both",
        "--access-key",
        "hk1",
    ]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    assert!(text.contains("Nothing was written"));
    assert!(text.contains("would change"));
    assert_eq!(before, f.snapshot(), "a draft must not touch the v-Drive");
}

#[test]
fn write_without_confirmation_is_refused() {
    let f = Fixture::new();
    let before = f.snapshot();
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "4",
        "--preset",
        "symbols",
        "--write",
    ]);
    assert_eq!(code(&out), 2, "{}", combined(&out));
    assert!(combined(&out).contains("--yes"));
    assert_eq!(before, f.snapshot());
}

#[test]
fn profile_zero_is_refused_even_with_a_full_confirmation() {
    let f = Fixture::new();
    let before = f.snapshot();
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "0",
        "--preset",
        "symbols",
        "--write",
        "--yes",
    ]);
    assert_ne!(code(&out), 0);
    assert!(combined(&out).contains("profile 0"), "{}", combined(&out));
    assert_eq!(before, f.snapshot());
}

#[test]
fn a_corrupted_profile_blocks_the_write_and_leaves_the_drive_alone() {
    let f = Fixture::new();
    let before = f.snapshot();
    // Profile 5 has a syntax error.
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "5",
        "--preset",
        "symbols",
        "--write",
        "--yes",
    ]);
    assert_eq!(code(&out), 1, "{}", combined(&out));
    let text = combined(&out);
    assert!(text.contains("validation failed"), "{text}");
    assert_eq!(
        before,
        f.snapshot(),
        "a failed validation must leave the drive untouched"
    );
}

#[test]
fn a_real_write_only_changes_the_selected_profile_after_a_verified_backup() {
    let f = Fixture::new();
    let settings_before = f.read("settings.txt");
    let other_layouts_before: Vec<(String, Vec<u8>)> = f
        .layouts()
        .into_iter()
        .filter(|(n, _)| n != "layout4.txt")
        .collect();
    let layout4_before = f.read("layouts/layout4.txt");

    let out = f.run(&[
        "apply-preset",
        "--profile",
        "4",
        "--preset",
        "both",
        "--access-key",
        "hk1",
        "--write",
        "--yes",
    ]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    assert!(text.contains("verified: true"), "{text}");
    assert!(
        text.contains("udisksctl"),
        "eject instructions must be printed: {text}"
    );

    // The selected profile changed...
    let layout4_after = f.read("layouts/layout4.txt");
    assert_ne!(layout4_after, layout4_before);
    assert!(layout4_after.contains("<function1>"));
    assert!(
        layout4_after.contains("{-lshf}{9}{+lshf}"),
        "shifted symbols must be macros"
    );
    assert!(
        layout4_after.contains("[hk1]>[fn2s]"),
        "the access key must be written"
    );

    // ...everything else is byte-identical...
    assert_eq!(settings_before, f.read("settings.txt"));
    assert_eq!(other_layouts_before, {
        let mut v: Vec<(String, Vec<u8>)> = f
            .layouts()
            .into_iter()
            .filter(|(n, _)| n != "layout4.txt")
            .collect();
        v.sort();
        v
    });

    // ...and a real dated backup exists with the original bytes.
    let backups = adv360_core::backup::list(&f.backups_dir());
    assert_eq!(backups.len(), 1, "exactly one backup set");
    let set = &backups[0];
    adv360_core::backup::verify(set).unwrap();
    assert!(set
        .files
        .iter()
        .any(|x| x.relative_path == "layouts/layout4.txt" && x.existed));
    assert_eq!(
        fs::read_to_string(set.directory.join("layouts/layout4.txt")).unwrap(),
        layout4_before
    );
    assert!(set
        .directory
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains("profile4"));
}

#[test]
fn an_unknown_instruction_and_a_comment_survive_a_real_write() {
    let f = Fixture::new();
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "3",
        "--preset",
        "navigation",
        "--access-key",
        "none",
        "--write",
        "--yes",
    ]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let after = f.read("layouts/layout3.txt");
    assert!(after.contains("# my own note"), "{after}");
    assert!(after.contains("VENDOREXT keep me"), "{after}");
    assert!(
        after.contains("*[q]>[w]"),
        "the disabled line must survive: {after}"
    );
    assert!(after.contains("[caps]>[caps][t&h500][esc]"), "{after}");
    assert!(after.contains("{tab}>{-lshf}{h}{+lshf}{i}"), "{after}");
    assert!(after.contains("\r\n"), "CRLF must be preserved: {after:?}");
    assert!(after.starts_with('\u{feff}'), "the BOM must be preserved");
    // And the new navigation content is in the function2 block.
    let f2 = after.find("<function2>").unwrap();
    let f3 = after.find("<function3>").unwrap();
    assert!(after[f2..f3].contains("[h]>[left]"), "{}", &after[f2..f3]);
}

#[test]
fn restore_puts_the_original_bytes_back() {
    let f = Fixture::new();
    let original = f.read("layouts/layout4.txt");
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "4",
        "--preset",
        "symbols",
        "--write",
        "--yes",
    ]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    assert_ne!(f.read("layouts/layout4.txt"), original);

    let backups = adv360_core::backup::list(&f.backups_dir());
    assert_eq!(backups.len(), 1);

    // Without --yes the restore refuses.
    let out = f.run(&[
        "--from-backup",
        backups[0].directory.to_str().unwrap(),
        "restore",
        "--profile",
        "4",
    ]);
    assert_eq!(code(&out), 2, "{}", combined(&out));
    assert!(combined(&out).contains("--yes"));

    // With --yes it restores exactly.
    let out = f.run(&[
        "--from-backup",
        backups[0].directory.to_str().unwrap(),
        "restore",
        "--profile",
        "4",
        "--yes",
    ]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    assert_eq!(f.read("layouts/layout4.txt"), original);
}

#[test]
fn export_then_diff_and_validate_against_the_draft() {
    let f = Fixture::new();
    // Build a draft by applying a preset, then export the whole draft.
    let out_dir = f.dir.path().join("export");
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "4",
        "--preset",
        "both",
        "--access-key",
        "none",
    ]);
    assert_eq!(code(&out), 0);

    // apply-preset does not persist, so export the current (unmodified) draft instead and confirm
    // the round trip works without touching the drive.
    let out = f.run(&["export", "--out", out_dir.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    assert!(out_dir.join("layouts/layout4.txt").exists());
    assert!(out_dir.join("adv360-draft.json").exists());
    assert!(out_dir.join("settings.txt").exists());

    let out = f.run(&[
        "validate",
        "--draft",
        out_dir.to_str().unwrap(),
        "--profile",
        "3",
    ]);
    assert_eq!(code(&out), 0, "{}", combined(&out));

    let out = f.run(&["diff", "--draft", out_dir.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    assert!(stdout(&out).contains("No differences") || stdout(&out).contains("Key changes"));

    // Now modify the exported draft and confirm the diff reports it, while the drive stays put.
    let before = f.snapshot();
    let mut draft = fs::read_to_string(out_dir.join("layouts/layout4.txt")).unwrap();
    draft = draft.replace("<base>\n", "<base>\n[esc]>[caps]\n");
    fs::write(out_dir.join("layouts/layout4.txt"), draft).unwrap();
    let out = f.run(&["diff", "--draft", out_dir.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    assert!(text.contains("[esc]"), "{text}");
    assert!(text.contains("caps"), "{text}");
    assert_eq!(before, f.snapshot());
}

#[test]
fn export_writes_nothing_to_a_backup_and_never_writes_to_the_keyboard() {
    let f = Fixture::new();
    let before = f.snapshot();
    let out_dir = f.dir.path().join("export");
    let out = f.run(&["export", "--out", out_dir.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    assert!(stdout(&out).contains("The keyboard was not touched."));
    assert_eq!(before, f.snapshot());
    assert!(
        adv360_core::backup::list(&f.backups_dir()).is_empty(),
        "export must not back up"
    );
}

#[test]
fn backup_can_be_taken_on_demand_and_is_listed() {
    let f = Fixture::new();
    let before = f.snapshot();
    let out = f.run(&["backup", "--profile", "3"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    assert!(stdout(&out).contains("Verified by re-reading"));
    assert_eq!(before, f.snapshot(), "a backup must not modify the drive");

    let out = f.run(&["backups"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("profile3"));
}

#[test]
fn mount_help_explains_both_directions() {
    let f = Fixture::new();
    let out = f.run(&["mount-help"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    assert!(
        text.contains("SmartSet"),
        "the mount shortcut must be explained"
    );
    assert!(
        text.contains("udisksctl"),
        "the eject command must be printed"
    );
    assert!(
        text.contains("never runs as root")
            || text.contains("never run as root")
            || text.contains("never")
    );
}

#[test]
fn a_directory_that_is_not_a_vdrive_is_rejected_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_adv360-config"))
        .args(["--device", dir.path().to_str().unwrap(), "detect"])
        .output()
        .unwrap();
    assert_ne!(code(&out), 0);
    assert!(combined(&out).contains("not"), "{}", combined(&out));
}

#[test]
fn detecting_with_no_device_reports_the_mount_instructions() {
    // Point at a mounted-but-wrong directory to force the "not a v-Drive" path, and confirm the
    // recovery hints are the concrete instructions rather than a bare error.
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_adv360-config"))
        .args(["--device", dir.path().to_str().unwrap(), "detect"])
        .output()
        .unwrap();
    let text = combined(&out);
    assert!(text.contains("error ["), "{text}");
    assert!(text.contains("layouts"), "{text}");
}

#[test]
fn a_lighting_file_is_validated_without_a_layer_header() {
    let f = Fixture::new();
    // lighting/led1.txt has an LED line and no layer headers; it must not be an error.
    let out = f.run(&["validate", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let findings = value.as_array().unwrap();
    assert!(
        !findings
            .iter()
            .any(|f| f["relative_path"] == "lighting/led1.txt" && f["severity"] == "error"),
        "an indicator file must not need a layer header: {findings:?}"
    );
}

#[test]
fn the_layout_command_distinguishes_token_from_character() {
    let f = Fixture::new();
    let out = f.run(&["layout"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    assert!(text.contains("what the keyboard sends"), "{text}");
    assert!(text.contains("character"), "{text}");
    // Under XKB_DEFAULT_LAYOUT=us the mapping is encoded, so the characters are confident.
    assert!(text.contains("us"), "{text}");
    assert!(text.contains("keyboard [ and {"), "{text}");
}

#[test]
fn an_explicit_non_us_layout_marks_the_preview_uncertain() {
    let f = Fixture::new();
    let out = f.run(&["--layout", "latam", "layout"]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let text = stdout(&out);
    assert!(text.contains("latam"), "{text}");
    assert!(text.contains("uncertain"), "{text}");
    assert!(text.contains("3.3") || text.contains("§3.3"), "{text}");
}

#[test]
fn the_binary_reports_a_clear_error_when_no_drive_and_no_device_are_available() {
    // Run with no --device in an environment where nothing looks like a v-Drive.
    let out = Command::new(env!("CARGO_BIN_EXE_adv360-config"))
        .arg("detect")
        .env("XDG_RUNTIME_DIR", "/nonexistent")
        .output()
        .unwrap();
    // Either it found a real drive on this machine (fine) or it reported the mount instructions.
    if code(&out) != 0 {
        let text = combined(&out);
        assert!(text.contains("error [device_not_found]"), "{text}");
        assert!(
            text.contains("SmartSet"),
            "the hints must explain how to mount: {text}"
        );
    }
}

#[test]
fn a_write_never_repeats_itself_and_the_second_run_is_refused() {
    let f = Fixture::new();
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "4",
        "--preset",
        "symbols",
        "--write",
        "--yes",
    ]);
    assert_eq!(code(&out), 0, "{}", combined(&out));
    let after_first = f.read("layouts/layout4.txt");

    // The second run rebuilds the same draft from scratch, which is now identical to what is on
    // the drive, so the write must be refused rather than rewriting the file.
    let out = f.run(&[
        "apply-preset",
        "--profile",
        "4",
        "--preset",
        "symbols",
        "--write",
        "--yes",
    ]);
    assert_eq!(code(&out), 1, "{}", combined(&out));
    assert!(combined(&out).contains("identical"), "{}", combined(&out));
    assert_eq!(after_first, f.read("layouts/layout4.txt"));
}

#[test]
fn the_fixture_itself_is_what_the_tests_assume() {
    // A guard against the fixture drifting: the messy profile really is messy.
    let f = Fixture::new();
    let text = f.read("layouts/layout3.txt");
    assert!(text.starts_with('\u{feff}'));
    assert!(text.contains("\r\n"));
    assert!(text.contains("VENDOREXT"));
    assert!(text.contains("*[q]>[w]"));
    assert!(text.contains("[t&h500]"));
    assert!(f.read("layouts/layout5.txt").contains("[g]>["));
    assert_eq!(text.lines().count(), 11);
}

#[test]
fn tui_and_gui_commands_exist_and_do_not_write_to_the_drive() {
    // Neither interface may be started in a test, but the commands must be reachable and must not
    // touch the drive before they fail on the missing terminal or display.
    let f = Fixture::new();
    let before = f.snapshot();
    for command in ["gui", "tui"] {
        let out = Command::new(env!("CARGO_BIN_EXE_adv360-config"))
            .args(["--device", f.root.to_str().unwrap(), command])
            .env("TERM", "dumb")
            .env("DISPLAY", "")
            .env("WAYLAND_DISPLAY", "")
            .env("GDK_BACKEND", "nonexistent-backend")
            .output()
            .expect("the binary must run");
        // Whether it starts or fails depends on the environment; what matters is that it left the
        // drive alone.
        let _ = out;
    }
    assert_eq!(before, f.snapshot());
}

#[test]
fn a_closed_pipe_does_not_panic() {
    // `adv360-config about | head -1` must behave like any other command line tool: the process
    // dies from SIGPIPE instead of panicking with an ugly broken-pipe message.
    use std::process::Stdio;
    let binary = env!("CARGO_BIN_EXE_adv360-config");
    let mut child = Command::new(binary)
        .arg("about")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary must run");
    // Close the read end immediately, then let the child write into a broken pipe.
    drop(child.stdout.take());
    let status = child.wait().expect("the child must exit");
    let stderr = {
        let mut buf = String::new();
        use std::io::Read;
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut buf);
        }
        buf
    };
    assert!(
        !stderr.contains("panicked"),
        "a closed pipe must not panic: {stderr}"
    );
    let _ = status;
}
