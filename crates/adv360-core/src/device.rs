//! Finding the keyboard's v-Drive and reading everything on it.
//!
//! The guide describes the v-Drive as "a series of simple text files (.txt)" in a `layouts`
//! subfolder and a `lighting` subfolder, plus a `settings.txt` that must not be edited. Nothing
//! here assumes a fixed layout: the directory tree is scanned, files are classified by name, and
//! anything unrecognised is reported rather than ignored. The mount point itself is discovered,
//! never guessed from a single hard-coded path.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::layer::ProfileId;
use crate::model::FileRole;
use crate::parser;

/// A file found on the v-Drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredFile {
    /// Path relative to the v-Drive root, using `/` separators.
    pub relative_path: String,
    pub role: FileRole,
    pub profile: Option<ProfileId>,
    pub bytes: u64,
    /// Comment shown next to the file in both interfaces.
    pub note: String,
}

/// A detected (or explicitly supplied) v-Drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VDrive {
    /// Absolute mount point.
    pub root: PathBuf,
    /// Why this path was accepted, shown to the operator.
    pub evidence: Vec<String>,
    /// The directory that actually contains the numbered layouts, as found on disk.
    pub layouts_dir: Option<String>,
    /// The directory that actually contains the indicator files, as found on disk.
    pub lighting_dir: Option<String>,
    pub files: Vec<DiscoveredFile>,
}

/// One path that was considered and rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedCandidate {
    pub path: PathBuf,
    pub reason: String,
}

/// The result of a discovery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub found: Option<VDrive>,
    pub considered: Vec<RejectedCandidate>,
}

impl VDrive {
    /// The file that holds a given profile's layout, when it exists.
    pub fn layout_file(&self, profile: ProfileId) -> Option<&DiscoveredFile> {
        let expected = profile.layout_file_name().to_ascii_lowercase();
        self.files.iter().find(|f| {
            f.role == FileRole::ProfileLayout
                && f.relative_path
                    .rsplit('/')
                    .next()
                    .is_some_and(|n| n.to_ascii_lowercase() == expected)
        })
    }

    /// Every file holding a profile layout, ordered by profile.
    pub fn layout_files(&self) -> Vec<&DiscoveredFile> {
        let mut v: Vec<&DiscoveredFile> = self
            .files
            .iter()
            .filter(|f| f.role == FileRole::ProfileLayout)
            .collect();
        v.sort_by_key(|f| f.profile.map(|p| p.0).unwrap_or(u8::MAX));
        v
    }

    pub fn lighting_files(&self) -> Vec<&DiscoveredFile> {
        let mut v: Vec<&DiscoveredFile> = self
            .files
            .iter()
            .filter(|f| f.role == FileRole::ProfileLighting)
            .collect();
        v.sort_by_key(|f| f.profile.map(|p| p.0).unwrap_or(u8::MAX));
        v
    }

    pub fn settings_file(&self) -> Option<&DiscoveredFile> {
        self.files.iter().find(|f| f.role == FileRole::Settings)
    }

    /// Files this tool would never modify.
    pub fn unwritable_files(&self) -> impl Iterator<Item = &DiscoveredFile> {
        self.files.iter().filter(|f| !f.role.is_writable())
    }

    /// The absolute path of a discovered file.
    pub fn absolute(&self, file: &DiscoveredFile) -> PathBuf {
        self.root.join(&file.relative_path)
    }

    /// Read every discovered `.txt` file into memory.
    ///
    /// Files that are not valid UTF-8 are skipped and reported, so a binary blob on the drive
    /// cannot stop the operator from editing the layouts.
    pub fn read_all(&self) -> (Vec<crate::model::ConfigFile>, Vec<(String, String)>) {
        let mut files = Vec::new();
        let mut skipped = Vec::new();
        for f in &self.files {
            if !f.relative_path.to_ascii_lowercase().ends_with(".txt") {
                continue;
            }
            match parser::read_file(&self.absolute(f), &f.relative_path, f.role) {
                Ok(cfg) => files.push(cfg),
                Err(e) => skipped.push((f.relative_path.clone(), e.to_string())),
            }
        }
        (files, skipped)
    }
}

/// Candidate roots, in the order they are tried.
fn candidate_roots(explicit: Option<&Path>) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    if let Some(p) = explicit {
        out.push((p.to_path_buf(), "supplied with --device".to_string()));
        return out;
    }

    // 1. Any block device whose volume label mentions the keyboard.
    if let Ok(entries) = fs::read_dir("/dev/disk/by-label") {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().contains("adv360") {
                if let Ok(target) = fs::read_link(e.path()) {
                    let dev = PathBuf::from("/dev").join(
                        target
                            .file_name()
                            .map(PathBuf::from)
                            .unwrap_or_else(|| target.clone()),
                    );
                    if let Some(mount) = mountpoint_for(&dev) {
                        out.push((mount, format!("volume label '{name}' is mounted here")));
                    }
                }
            }
        }
    }

    // 2. Where udisks2 and the classic layouts put removable media.
    let user = std::env::var("USER").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for base in ["/run/media", "/media"] {
        if !user.is_empty() {
            dirs.push(PathBuf::from(base).join(&user));
        }
        dirs.push(PathBuf::from(base));
    }
    dirs.push(PathBuf::from("/mnt"));
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(name) = p.file_name() {
                    let n = name.to_string_lossy();
                    if n.to_ascii_lowercase().contains("adv360") {
                        out.push((p.clone(), format!("{n} under {}", dir.display())));
                    } else {
                        out.push((p, "removable media mount point".to_string()));
                    }
                }
            }
        }
    }

    out
}

/// Look up the mount point of a device node in `/proc/self/mountinfo`.
fn mountpoint_for(device: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string("/proc/self/mountinfo").ok()?;
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // mountinfo: id parent major:minor root mountpoint options ... - fstype source superopts
        if fields.len() < 5 {
            continue;
        }
        let mount_point = fields[4];
        let source = fields
            .iter()
            .position(|f| *f == "-")
            .and_then(|idx| fields.get(idx + 2))
            .copied()
            .unwrap_or("");
        if Path::new(source) == device {
            return Some(PathBuf::from(mount_point.replace("\\040", " ")));
        }
    }
    None
}

/// Assess whether a directory looks like an Advantage360 v-Drive.
///
/// Returns the evidence lines when it does, or a rejection reason when it does not.
pub fn assess(root: &Path) -> Result<Vec<String>, String> {
    if !root.exists() {
        return Err("path does not exist".to_string());
    }
    if !root.is_dir() {
        return Err("path is not a directory".to_string());
    }

    let mut evidence = Vec::new();
    let name_looks_right = root
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase().contains("adv360"))
        .unwrap_or(false);
    if name_looks_right {
        evidence.push("mount point name contains ADV360".to_string());
    }

    let mut has_layouts = false;
    let mut has_settings = false;
    let mut numbered_layouts = 0usize;
    for entry in walk(root, 3) {
        let Some(file_name) = entry.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        if entry.is_dir() {
            if file_name.eq_ignore_ascii_case("layouts") {
                has_layouts = true;
                evidence.push(format!("contains a '{file_name}' folder"));
            }
            continue;
        }
        if file_name.eq_ignore_ascii_case("settings.txt") {
            has_settings = true;
        }
        if ProfileId::parse_layout_file_name(&file_name).is_some() {
            numbered_layouts += 1;
        }
    }
    if has_settings {
        evidence.push("contains settings.txt".to_string());
    }
    if numbered_layouts > 0 {
        evidence.push(format!(
            "contains {numbered_layouts} numbered layout file(s)"
        ));
    }

    // Accept on structure, not only on the mount point's name: a v-Drive mounted by hand under
    // an arbitrary name must still be usable.
    if !(has_layouts || has_settings) {
        return Err(
            "no 'layouts' folder and no settings.txt, so this does not look like an ADV360 v-Drive"
                .to_string(),
        );
    }
    if !name_looks_right {
        evidence.push(
            "accepted on structure alone: the mount point name does not contain ADV360, so \
             verify that this is really the keyboard's drive"
                .to_string(),
        );
    }
    Ok(evidence)
}

/// Walk a directory tree up to `max_depth`, without following symlinks.
fn walk(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            out.push(path.clone());
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir && depth < max_depth {
                stack.push((path, depth + 1));
            }
        }
    }
    out
}

/// Classify a file found on the drive.
pub fn classify(
    relative_path: &str,
    name: &str,
    in_layouts: bool,
    in_lighting: bool,
) -> (FileRole, Option<ProfileId>, String) {
    if name.eq_ignore_ascii_case("settings.txt") {
        return (
            FileRole::Settings,
            None,
            "The guide says: do not edit the settings.txt file. Shown read-only, never written."
                .to_string(),
        );
    }
    if let Some(p) = ProfileId::parse_layout_file_name(name) {
        if p.0 == 0 {
            return (
                FileRole::BackupLayout,
                Some(p),
                "Named layout0.txt. The guide says profile 0 is non-programmable and has no layout \
                 file, so this is treated as a backup layout and never written."
                    .to_string(),
            );
        }
        return (
            FileRole::ProfileLayout,
            Some(p),
            format!(
                "Layout for profile {}. The only kind of file this tool writes.",
                p.0
            ),
        );
    }
    if let Some(p) = ProfileId::parse_lighting_file_name(name) {
        return (
            FileRole::ProfileLighting,
            Some(p),
            format!("Indicator lighting for profile {}.", p.0),
        );
    }
    if in_layouts {
        return (
            FileRole::BackupLayout,
            None,
            "A .txt file in the layouts folder that is not named layout<N>.txt. The guide says a \
             backup layout cannot be loaded without renaming it first, so it is never written."
                .to_string(),
        );
    }
    if in_lighting {
        return (
            FileRole::Other,
            None,
            "Indicator-related file with an unexpected name.".to_string(),
        );
    }
    let _ = relative_path;
    (
        FileRole::Other,
        None,
        "Unrecognised file on the v-Drive.".to_string(),
    )
}

/// Discover the v-Drive, or explain why it could not be found.
pub fn discover(explicit: Option<&Path>) -> CoreResult<Discovery> {
    let mut considered = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();

    for (path, why) in candidate_roots(explicit) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(canonical.clone()) {
            continue;
        }
        match assess(&path) {
            Ok(mut evidence) => {
                evidence.insert(0, format!("found via {why}"));
                let drive = build(&path, evidence)?;
                return Ok(Discovery {
                    found: Some(drive),
                    considered,
                });
            }
            Err(reason) => considered.push(RejectedCandidate { path, reason }),
        }
    }

    if let Some(p) = explicit {
        return Err(CoreError::NotAVDrive {
            root: p.to_path_buf(),
            missing: considered
                .first()
                .map(|c| c.reason.clone())
                .unwrap_or_else(|| "it does not exist".to_string()),
        });
    }

    Err(CoreError::DeviceNotFound {
        detail: if considered.is_empty() {
            "no removable media was found in /run/media, /media or /mnt, and no block device is \
             labelled ADV360"
                .to_string()
        } else {
            format!(
                "{} candidate path(s) were checked and none looked like an ADV360 v-Drive",
                considered.len()
            )
        },
    })
}

/// Build a [`VDrive`] for a verified root.
pub fn build(root: &Path, evidence: Vec<String>) -> CoreResult<VDrive> {
    let mut files = Vec::new();
    let mut layouts_dir = None;
    let mut lighting_dir = None;

    for path in walk(root, 3) {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        if path.is_dir() {
            if name.eq_ignore_ascii_case("layouts") {
                layouts_dir = Some(name);
            } else if name.eq_ignore_ascii_case("lighting") || name.eq_ignore_ascii_case("led") {
                lighting_dir = Some(name);
            }
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative_path = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        if relative_path.starts_with('.') || relative_path.contains("/.") {
            // Editor and desktop junk (macOS leaves ._ files) is reported but not treated as
            // configuration.
            continue;
        }
        let metadata = fs::metadata(&path).map_err(|source| CoreError::Read {
            path: path.clone(),
            source,
        })?;
        let parent_names: Vec<String> = relative
            .parent()
            .map(|p| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let in_layouts = parent_names
            .iter()
            .any(|p| p.eq_ignore_ascii_case("layouts"));
        let in_lighting = parent_names
            .iter()
            .any(|p| p.eq_ignore_ascii_case("lighting") || p.eq_ignore_ascii_case("led"));
        let (role, profile, note) = classify(&relative_path, &name, in_layouts, in_lighting);
        files.push(DiscoveredFile {
            relative_path,
            role,
            profile,
            bytes: metadata.len(),
            note,
        });
    }

    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(VDrive {
        root: root.to_path_buf(),
        evidence,
        layouts_dir,
        lighting_dir,
        files,
    })
}

/// Human instructions for mounting the v-Drive, used by both interfaces and by every error path.
pub fn mount_instructions() -> Vec<String> {
    vec![
        "Connect the keyboard with its own USB cable to the computer (not through a KVM switch or \
         an unpowered hub if you can avoid it)."
            .into(),
        "Press and hold the SmartSet key (the gear icon on the right module) and tap the mount \
         shortcut to open the v-Drive."
            .into(),
        "Wait for a removable volume to appear. On Omarchy/Arch with udisks2 it is normally \
         mounted at /run/media/$USER/ADV360."
            .into(),
        "Re-run detection, or point the tool at the folder explicitly: adv360-config --device \
         /run/media/$USER/ADV360 detect"
            .into(),
        "This tool never runs as root and never mounts the drive itself: the keyboard is the one \
         that presents the v-Drive."
            .into(),
    ]
}

/// Safe-eject instructions. The guide is explicit that the v-Drive must be ejected before it is
/// closed from the keyboard.
pub fn eject_instructions(root: &Path) -> Vec<String> {
    let mut out = vec![
        "Save and close every file you opened on the v-Drive.".to_string(),
        "Unmount it so all writes are flushed:".to_string(),
    ];
    match device_node_for(root) {
        Some(dev) => {
            out.push(format!("    udisksctl unmount -b {}", dev.display()));
        }
        None => {
            out.push("    Find the device with: lsblk -o NAME,LABEL,MOUNTPOINT".to_string());
            out.push("    Then run: udisksctl unmount -b /dev/sdXN".to_string());
        }
    }
    out.push(
        "Only after the drive is unmounted, close the v-Drive from the keyboard with the same \
         SmartSet shortcut."
            .to_string(),
    );
    out.push(
        "Closing the v-Drive while the computer is still writing can corrupt a layout file; the \
         guide warns about exactly this."
            .to_string(),
    );
    out
}

/// Resolve the device node that backs a mount point, when possible.
pub fn device_node_for(mount_point: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string("/proc/self/mountinfo").ok()?;
    let target = mount_point
        .canonicalize()
        .unwrap_or_else(|_| mount_point.to_path_buf());
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        let mp = PathBuf::from(fields[4].replace("\\040", " "));
        let canonical = mp.canonicalize().unwrap_or_else(|_| mp.clone());
        if canonical == target {
            if let Some(idx) = fields.iter().position(|f| *f == "-") {
                if let Some(source) = fields.get(idx + 2) {
                    return Some(PathBuf::from(source));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ADV360");
        fs::create_dir_all(root.join("layouts")).unwrap();
        fs::create_dir_all(root.join("lighting")).unwrap();
        fs::create_dir_all(root.join("firmware")).unwrap();
        fs::write(root.join("settings.txt"), "nope\n").unwrap();
        fs::write(root.join("layouts/layout1.txt"), "<base>\n[q]>[w]\n").unwrap();
        fs::write(root.join("layouts/layout4.txt"), "<base>\n").unwrap();
        fs::write(root.join("layouts/my_backup.txt"), "<base>\n").unwrap();
        fs::write(root.join("lighting/led1.txt"), "[IND1]>[null]\n").unwrap();
        fs::write(root.join("firmware/update.bin"), [0u8, 1, 2]).unwrap();
        dir
    }

    #[test]
    fn a_realistic_drive_is_recognised() {
        let dir = fixture();
        let root = dir.path().join("ADV360");
        let evidence = assess(&root).unwrap();
        assert!(evidence.iter().any(|e| e.contains("ADV360")));
        assert!(evidence.iter().any(|e| e.contains("layouts")));
    }

    #[test]
    fn a_non_keyboard_directory_is_rejected_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        let err = assess(dir.path()).unwrap_err();
        assert!(err.contains("layouts"), "{err}");
    }

    #[test]
    fn a_directory_that_does_not_exist_is_rejected() {
        let err = assess(Path::new("/definitely/not/here")).unwrap_err();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn files_are_classified_by_name_and_never_by_position() {
        let dir = fixture();
        let drive = build(&dir.path().join("ADV360"), vec![]).unwrap();
        let by_name = |n: &str| {
            drive
                .files
                .iter()
                .find(|f| f.relative_path == n)
                .unwrap_or_else(|| panic!("{n} missing"))
        };
        assert_eq!(by_name("layouts/layout1.txt").role, FileRole::ProfileLayout);
        assert_eq!(by_name("layouts/layout1.txt").profile, Some(ProfileId(1)));
        assert_eq!(
            by_name("layouts/my_backup.txt").role,
            FileRole::BackupLayout
        );
        assert_eq!(by_name("settings.txt").role, FileRole::Settings);
        assert_eq!(by_name("lighting/led1.txt").role, FileRole::ProfileLighting);
        assert_eq!(by_name("firmware/update.bin").role, FileRole::Other);
    }

    #[test]
    fn only_profile_layouts_are_writable() {
        let dir = fixture();
        let drive = build(&dir.path().join("ADV360"), vec![]).unwrap();
        let writable: Vec<&str> = drive
            .files
            .iter()
            .filter(|f| f.role.is_writable())
            .map(|f| f.relative_path.as_str())
            .collect();
        assert_eq!(writable, vec!["layouts/layout1.txt", "layouts/layout4.txt"]);
        assert!(drive.layout_file(ProfileId(1)).is_some());
        assert!(drive.layout_file(ProfileId(5)).is_none());
        assert_eq!(drive.layout_files().len(), 2);
        assert_eq!(drive.lighting_files().len(), 1);
        assert!(drive.settings_file().is_some());
    }

    #[test]
    fn layout0_is_never_writable_even_if_it_exists() {
        let dir = fixture();
        fs::write(dir.path().join("ADV360/layouts/layout0.txt"), "<base>\n").unwrap();
        let drive = build(&dir.path().join("ADV360"), vec![]).unwrap();
        let f = drive
            .files
            .iter()
            .find(|f| f.relative_path == "layouts/layout0.txt")
            .unwrap();
        assert_eq!(f.role, FileRole::BackupLayout);
        assert!(!f.role.is_writable());
        assert!(f.note.contains("non-programmable"));
    }

    #[test]
    fn read_all_reads_every_text_file_including_the_read_only_ones() {
        let dir = fixture();
        let drive = build(&dir.path().join("ADV360"), vec![]).unwrap();
        let (files, skipped) = drive.read_all();
        assert!(skipped.is_empty(), "{skipped:?}");
        assert!(files.iter().any(|f| f.relative_path == "settings.txt"));
        assert!(files
            .iter()
            .any(|f| f.relative_path == "layouts/layout1.txt"));
        assert!(files.iter().any(|f| f.relative_path == "lighting/led1.txt"));
    }

    #[test]
    fn discovery_accepts_an_explicit_path() {
        let dir = fixture();
        fs::create_dir_all(dir.path().join("notadv360")).unwrap();
        let d = discover(Some(&dir.path().join("ADV360"))).unwrap();
        assert!(d.found.is_some());
        assert!(d.considered.is_empty());
    }

    #[test]
    fn discovery_rejects_an_explicit_path_that_is_not_a_vdrive() {
        let dir = tempfile::tempdir().unwrap();
        let err = discover(Some(dir.path())).unwrap_err();
        assert_eq!(err.code(), "not_a_vdrive");
    }

    #[test]
    fn discovery_accepts_a_correctly_structured_drive_under_any_name() {
        // A drive mounted by hand somewhere else must still work, but the evidence says so.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("mystick");
        fs::create_dir_all(root.join("layouts")).unwrap();
        let d = discover(Some(&root)).unwrap();
        let drive = d.found.unwrap();
        assert!(drive.evidence.iter().any(|e| e.contains("structure alone")));
    }

    #[test]
    fn hidden_editor_junk_is_ignored() {
        let dir = fixture();
        fs::write(dir.path().join("ADV360/layouts/._layout1.txt"), "junk").unwrap();
        fs::write(dir.path().join("ADV360/.DS_Store"), "junk").unwrap();
        let drive = build(&dir.path().join("ADV360"), vec![]).unwrap();
        assert!(!drive.files.iter().any(|f| f.relative_path.contains("._")));
        assert!(!drive
            .files
            .iter()
            .any(|f| f.relative_path.contains(".DS_Store")));
    }

    #[test]
    fn mount_and_eject_instructions_name_the_real_paths() {
        let mount = mount_instructions();
        assert!(mount.iter().any(|l| l.contains("/run/media")));
        assert!(mount.iter().any(|l| l.contains("SmartSet")));
        let eject = eject_instructions(Path::new("/run/media/x/ADV360"));
        assert!(eject.iter().any(|l| l.contains("udisksctl")));
        assert!(eject.iter().any(|l| l.contains("corrupt")));
    }
}
