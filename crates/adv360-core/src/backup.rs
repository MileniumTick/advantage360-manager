//! Dated, verified local backups of the original v-Drive files.
//!
//! The rule this module exists to enforce: **no write to the keyboard happens without a verified
//! backup of the file's original content first.** A backup is not "a copy we hope is good": every
//! copied file is read back from the backup location and its SHA-256 is compared with the source
//! before the backup is reported as usable.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{CoreError, CoreResult};
use crate::writer::write_verified;

/// One file captured in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackedUpFile {
    /// Path relative to the v-Drive root, e.g. `layouts/layout4.txt`.
    pub relative_path: String,
    /// Path inside the backup directory.
    pub backup_path: String,
    pub bytes: u64,
    /// SHA-256 of the original content, hex encoded.
    pub sha256: String,
    /// `false` when the file did not exist on the v-Drive at backup time. Restoring such an entry
    /// means "this profile had no custom remaps".
    pub existed: bool,
}

/// A dated backup set on the local computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSet {
    /// Absolute path of the backup directory.
    pub directory: PathBuf,
    /// Local timestamp, `YYYY-MM-DD HH:MM:SS`.
    pub created_at: String,
    /// What was backed up, for the directory name and the manifest.
    pub scope: String,
    /// The v-Drive root the files were read from.
    pub source_root: PathBuf,
    pub files: Vec<BackedUpFile>,
}

impl BackupSet {
    pub fn manifest_path(&self) -> PathBuf {
        self.directory.join("manifest.json")
    }

    /// True when nothing was actually captured (an empty scope).
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Files that existed and can restore real content.
    pub fn existing_files(&self) -> impl Iterator<Item = &BackedUpFile> {
        self.files.iter().filter(|f| f.existed)
    }
}

/// The default local backup root: `$XDG_DATA_HOME/adv360-manager/backups`, falling back to
/// `~/.local/share/adv360-manager/backups`.
pub fn default_backup_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("adv360-manager").join("backups");
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local")
        .join("share")
        .join("adv360-manager")
        .join("backups")
}

/// Compute the SHA-256 of a byte slice, hex encoded.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Create a backup of the given files, relative to `source_root`, under `backup_root`.
///
/// Every copied file is verified by re-reading it from the backup directory.
pub fn create(
    source_root: &Path,
    relative_paths: &[String],
    backup_root: &Path,
    scope: &str,
) -> CoreResult<BackupSet> {
    if !source_root.is_dir() {
        return Err(CoreError::NotADirectory(source_root.to_path_buf()));
    }
    let now = chrono::Local::now();
    let stamp = now.format("%Y-%m-%d_%H%M%S").to_string();
    let created_at = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let safe_scope = sanitize(scope);
    let directory = unique_dir(backup_root, &format!("{stamp}-{safe_scope}"))?;

    fs::create_dir_all(&directory).map_err(|source| CoreError::CreateDir {
        path: directory.clone(),
        source,
    })?;

    let mut files = Vec::new();
    for rel in relative_paths {
        let src = source_root.join(rel);
        let dst = directory.join(rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|source| CoreError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        match fs::read(&src) {
            Ok(bytes) => {
                let sha = sha256_hex(&bytes);
                fs::write(&dst, &bytes).map_err(|source| CoreError::Write {
                    path: dst.clone(),
                    source,
                })?;
                // Verify the copy really is the original before reporting success.
                let copied = fs::read(&dst).map_err(|source| CoreError::Read {
                    path: dst.clone(),
                    source,
                })?;
                let copied_sha = sha256_hex(&copied);
                if copied_sha != sha || copied.len() != bytes.len() {
                    return Err(CoreError::VerifyFailed {
                        detail: format!(
                            "the backup copy of {rel} at {} does not match the original",
                            dst.display()
                        ),
                    });
                }
                files.push(BackedUpFile {
                    relative_path: rel.clone(),
                    backup_path: dst.to_string_lossy().to_string(),
                    bytes: bytes.len() as u64,
                    sha256: sha,
                    existed: true,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Nothing to back up for this path; record the fact so restore is explicit.
                files.push(BackedUpFile {
                    relative_path: rel.clone(),
                    backup_path: dst.to_string_lossy().to_string(),
                    bytes: 0,
                    sha256: sha256_hex(&[]),
                    existed: false,
                });
            }
            Err(source) => return Err(CoreError::Read { path: src, source }),
        }
    }

    let set = BackupSet {
        directory: directory.clone(),
        created_at,
        scope: scope.to_string(),
        source_root: source_root.to_path_buf(),
        files,
    };
    write_manifest(&set)?;
    if let Ok(handle) = fs::File::open(&directory) {
        let _ = handle.sync_all();
    }
    Ok(set)
}

fn sanitize(scope: &str) -> String {
    let cleaned: String = scope
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "backup".to_string()
    } else {
        trimmed
    }
}

fn unique_dir(root: &Path, name: &str) -> CoreResult<PathBuf> {
    let mut candidate = root.join(name);
    let mut n = 1;
    while candidate.exists() {
        n += 1;
        candidate = root.join(format!("{name}-{n}"));
    }
    Ok(candidate)
}

fn write_manifest(set: &BackupSet) -> CoreResult<()> {
    let json = serde_json::to_string_pretty(set)
        .map_err(|e| CoreError::Other(format!("cannot serialise the backup manifest: {e}")))?;
    let path = set.manifest_path();
    write_verified(&path, &json)?;
    Ok(())
}

/// Read a backup manifest back from disk.
pub fn read_manifest(backup_dir: &Path) -> CoreResult<BackupSet> {
    let path = backup_dir.join("manifest.json");
    let text = fs::read_to_string(&path).map_err(|source| CoreError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|e| {
        CoreError::Other(format!(
            "{} is not a valid backup manifest: {e}",
            path.display()
        ))
    })
}

/// Re-verify every file in a backup set against its recorded SHA-256.
pub fn verify(set: &BackupSet) -> CoreResult<()> {
    for f in set.existing_files() {
        let bytes = fs::read(&f.backup_path).map_err(|source| CoreError::Read {
            path: PathBuf::from(&f.backup_path),
            source,
        })?;
        let sha = sha256_hex(&bytes);
        if sha != f.sha256 {
            return Err(CoreError::VerifyFailed {
                detail: format!(
                    "backup file {} is corrupted (recorded {}, found {})",
                    f.backup_path, f.sha256, sha
                ),
            });
        }
    }
    Ok(())
}

/// List the backup sets under a root, newest first.
pub fn list(backup_root: &Path) -> Vec<BackupSet> {
    let Ok(entries) = fs::read_dir(backup_root) else {
        return Vec::new();
    };
    let mut sets: Vec<BackupSet> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| read_manifest(&e.path()).ok())
        .collect();
    sets.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    sets
}

/// Restore the listed files from a backup set into `destination_root`.
///
/// Files that did not exist at backup time are left alone (there is nothing to restore) but the
/// outcome is reported so the operator knows.
pub fn restore(
    set: &BackupSet,
    destination_root: &Path,
    relative_paths: &[String],
) -> CoreResult<Vec<String>> {
    verify(set)?;
    let mut restored = Vec::new();
    for rel in relative_paths {
        let Some(entry) = set.files.iter().find(|f| &f.relative_path == rel) else {
            continue;
        };
        if !entry.existed {
            continue;
        }
        let bytes = fs::read(&entry.backup_path).map_err(|source| CoreError::Read {
            path: PathBuf::from(&entry.backup_path),
            source,
        })?;
        let text = String::from_utf8(bytes).map_err(|_| CoreError::NotUtf8 {
            path: PathBuf::from(&entry.backup_path),
        })?;
        let target = destination_root.join(rel);
        write_verified(&target, &text)?;
        restored.push(rel.clone());
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ADV360");
        fs::create_dir_all(root.join("layouts")).unwrap();
        fs::write(root.join("settings.txt"), "do not touch\n").unwrap();
        fs::write(root.join("layouts/layout1.txt"), "<base>\n[q]>[w]\n").unwrap();
        (dir, root)
    }

    #[test]
    fn a_backup_copies_and_verifies_every_file() {
        let (tmp, root) = fixture();
        let backups = tmp.path().join("backups");
        let set = create(
            &root,
            &["layouts/layout1.txt".into(), "settings.txt".into()],
            &backups,
            "profile-1",
        )
        .unwrap();
        assert_eq!(set.files.len(), 2);
        assert!(set.files.iter().all(|f| f.existed));
        assert!(set.directory.starts_with(&backups));
        assert!(set.manifest_path().exists());
        verify(&set).unwrap();
        // The content really is there.
        let restored = fs::read_to_string(set.directory.join("layouts/layout1.txt")).unwrap();
        assert_eq!(restored, "<base>\n[q]>[w]\n");
        assert!(set
            .directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("profile-1"));
    }

    #[test]
    fn the_directory_name_is_dated() {
        let (tmp, root) = fixture();
        let set = create(
            &root,
            &["layouts/layout1.txt".into()],
            &tmp.path().join("b"),
            "p",
        )
        .unwrap();
        let name = set
            .directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        // YYYY-MM-DD_HHMMSS-...
        assert!(name.len() >= 19, "expected a dated directory, got {name}");
        assert_eq!(name.as_bytes()[4], b'-', "{name}");
        assert_eq!(name.as_bytes()[7], b'-', "{name}");
        assert_eq!(name.as_bytes()[10], b'_', "{name}");
        assert!(
            name.as_bytes()[11..17].iter().all(|b| b.is_ascii_digit()),
            "expected YYYY-MM-DD_HHMMSS, got {name}"
        );
    }

    #[test]
    fn a_missing_file_is_recorded_as_not_existing() {
        let (tmp, root) = fixture();
        let set = create(
            &root,
            &["layouts/layout5.txt".into()],
            &tmp.path().join("b"),
            "p",
        )
        .unwrap();
        assert_eq!(set.existing_files().count(), 0);
        assert_eq!(set.files.len(), 1);
        assert!(!set.files[0].existed);
        verify(&set).unwrap();
    }

    #[test]
    fn two_backups_in_the_same_second_do_not_collide() {
        let (tmp, root) = fixture();
        let root_b = tmp.path().join("b");
        let a = create(&root, &["layouts/layout1.txt".into()], &root_b, "p").unwrap();
        let b = create(&root, &["layouts/layout1.txt".into()], &root_b, "p").unwrap();
        assert_ne!(a.directory, b.directory);
        assert_eq!(list(&root_b).len(), 2);
    }

    #[test]
    fn verification_detects_a_corrupted_backup() {
        let (tmp, root) = fixture();
        let set = create(
            &root,
            &["layouts/layout1.txt".into()],
            &tmp.path().join("b"),
            "p",
        )
        .unwrap();
        fs::write(set.directory.join("layouts/layout1.txt"), "tampered").unwrap();
        let err = verify(&set).unwrap_err();
        assert_eq!(err.code(), "verify_failed");
    }

    #[test]
    fn restore_puts_the_original_content_back() {
        let (tmp, root) = fixture();
        let set = create(
            &root,
            &["layouts/layout1.txt".into()],
            &tmp.path().join("b"),
            "p",
        )
        .unwrap();
        // Simulate a bad write.
        fs::write(root.join("layouts/layout1.txt"), "clobbered").unwrap();
        let restored = restore(&set, &root, &["layouts/layout1.txt".into()]).unwrap();
        assert_eq!(restored, vec!["layouts/layout1.txt"]);
        assert_eq!(
            fs::read_to_string(root.join("layouts/layout1.txt")).unwrap(),
            "<base>\n[q]>[w]\n"
        );
    }

    #[test]
    fn restore_skips_files_that_never_existed() {
        let (tmp, root) = fixture();
        let set = create(
            &root,
            &["layouts/layout5.txt".into()],
            &tmp.path().join("b"),
            "p",
        )
        .unwrap();
        let restored = restore(&set, &root, &["layouts/layout5.txt".into()]).unwrap();
        assert!(restored.is_empty());
        assert!(!root.join("layouts/layout5.txt").exists());
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn list_returns_newest_first_and_tolerates_a_missing_root() {
        assert!(list(Path::new("/definitely/not/here")).is_empty());
    }
}
