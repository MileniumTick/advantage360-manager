//! Writing configuration files, verified and recoverable.
//!
//! The write path is deliberately narrow:
//!
//! 1. the caller must already have taken a dated backup (see [`crate::backup`]);
//! 2. the caller must have validated the file and the operator must have confirmed;
//! 3. this module writes to a temporary file in the *same directory*, flushes it with
//!    `fsync`, reads it back and compares it byte for byte with what was intended;
//! 4. only then does it `rename` the temporary file over the target and `fsync` the directory;
//! 5. it reads the target back one final time and compares again.
//!
//! A failure at any step leaves the original file untouched, because the rename is the only
//! operation that replaces it and the rename is atomic on the v-Drive's filesystem.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{CoreError, CoreResult};
use crate::model::ConfigFile;

/// What a write would do, or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritePlan {
    /// Path as reported to the operator, relative to the v-Drive root.
    pub relative_path: String,
    /// Absolute path on disk.
    pub absolute_path: PathBuf,
    /// The bytes currently on the v-Drive, or `None` when the file does not exist yet.
    pub before: Option<String>,
    /// The bytes that will be written.
    pub after: String,
    /// The 1-based line numbers that differ.
    pub changed_lines: Vec<usize>,
}

impl WritePlan {
    /// True when the plan would not change anything.
    pub fn is_noop(&self) -> bool {
        self.before.as_deref() == Some(self.after.as_str())
    }
}

/// The outcome of a completed write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome {
    pub relative_path: String,
    pub bytes_written: usize,
    /// True when the file was read back after the rename and matched exactly.
    pub verified: bool,
}

/// Build the plan for writing one file, without touching the disk.
pub fn plan(file: &ConfigFile, absolute_path: &Path) -> CoreResult<WritePlan> {
    let after = file.render();
    let before = match fs::read_to_string(absolute_path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(CoreError::Read {
                path: absolute_path.to_path_buf(),
                source,
            })
        }
    };
    let changed_lines = changed_line_numbers(before.as_deref(), &after);
    Ok(WritePlan {
        relative_path: file.relative_path.clone(),
        absolute_path: absolute_path.to_path_buf(),
        before,
        after,
        changed_lines,
    })
}

/// 1-based line numbers that differ between two texts, using a simple longest-common-subsequence
/// table so insertions and deletions do not misreport every following line.
pub fn changed_line_numbers(before: Option<&str>, after: &str) -> Vec<usize> {
    let Some(before) = before else {
        return (1..=after.lines().count().max(1)).collect();
    };
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let mut changed = Vec::new();
    let n = a.len();
    let m = b.len();
    // LCS table over line identity.
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            changed.push(i + 1);
            i += 1;
        } else {
            changed.push(j + 1);
            j += 1;
        }
    }
    while i < n {
        changed.push(i + 1);
        i += 1;
    }
    while j < m {
        changed.push(j + 1);
        j += 1;
    }
    changed.sort_unstable();
    changed.dedup();
    changed
}

/// Write `contents` to `path` atomically and verify the result.
///
/// Returns [`CoreError::Write`] or [`CoreError::Disconnected`] without leaving a partial file in
/// place of the original.
pub fn write_verified(path: &Path, contents: &str) -> CoreResult<WriteOutcome> {
    let dir = path.parent().ok_or_else(|| CoreError::RefusedWrite {
        reason: format!("{} has no parent directory", path.display()),
    })?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "layout.txt".to_string());
    let tmp_path = dir.join(format!(".{file_name}.adv360-tmp"));

    // 1. Write the temporary file next to the target.
    {
        let mut tmp = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)
            .map_err(|source| map_io(path, source))?;
        tmp.write_all(contents.as_bytes())
            .map_err(|source| map_io(path, source))?;
        tmp.flush().map_err(|source| map_io(path, source))?;
        tmp.sync_all().map_err(|source| map_io(path, source))?;
    }

    // 2. Read the temporary file back before it replaces anything.
    let readback = fs::read_to_string(&tmp_path).map_err(|source| map_io(path, source))?;
    if readback != contents {
        let _ = fs::remove_file(&tmp_path);
        return Err(CoreError::VerifyFailed {
            detail: format!(
                "the temporary file for {} did not match what was written ({} bytes intended, {} \
                 bytes read back)",
                path.display(),
                contents.len(),
                readback.len()
            ),
        });
    }

    // 3. Atomically replace the target.
    if let Err(source) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(map_io(path, source));
    }

    // 4. Flush the directory entry so the rename survives a power loss.
    if let Ok(dir_handle) = File::open(dir) {
        let _ = dir_handle.sync_all();
    }

    // 5. Verify the final file.
    let mut final_file = File::open(path).map_err(|source| map_io(path, source))?;
    let mut final_bytes = Vec::new();
    final_file
        .read_to_end(&mut final_bytes)
        .map_err(|source| map_io(path, source))?;
    let final_text = String::from_utf8_lossy(&final_bytes).to_string();
    if final_text != contents {
        return Err(CoreError::VerifyFailed {
            detail: format!(
                "{} did not contain what was written after the rename; sizes: intended {}, on disk \
                 {}",
                path.display(),
                contents.len(),
                final_bytes.len()
            ),
        });
    }
    final_file
        .sync_all()
        .map_err(|source| map_io(path, source))?;

    Ok(WriteOutcome {
        relative_path: path.display().to_string(),
        bytes_written: contents.len(),
        verified: true,
    })
}

fn map_io(path: &Path, source: std::io::Error) -> CoreError {
    match source.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => {
            CoreError::Disconnected {
                detail: format!("{}: {source}", path.display()),
            }
        }
        std::io::ErrorKind::PermissionDenied => CoreError::Write {
            path: path.to_path_buf(),
            source,
        },
        _ => CoreError::Write {
            path: path.to_path_buf(),
            source,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerId;
    use crate::model::{Eol, FileRole, Line, LineKind, Origin, RemapLine};
    use crate::parser::parse_bytes;

    fn sample() -> ConfigFile {
        parse_bytes(
            b"<base>\n[q]>[w]\n",
            "layouts/layout1.txt",
            FileRole::ProfileLayout,
        )
        .unwrap()
    }

    #[test]
    fn plan_is_a_noop_when_nothing_changed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("layout1.txt");
        fs::write(&p, "<base>\n[q]>[w]\n").unwrap();
        let file = sample();
        let plan = plan(&file, &p).unwrap();
        assert!(plan.is_noop());
        assert!(plan.changed_lines.is_empty());
    }

    #[test]
    fn plan_reports_the_changed_line_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("layout1.txt");
        fs::write(&p, "<base>\n[q]>[w]\n<keypad>\n").unwrap();

        let mut file = parse_bytes(
            b"<base>\n[q]>[e]\n<keypad>\n",
            "layouts/layout1.txt",
            FileRole::ProfileLayout,
        )
        .unwrap();
        file.lines[1].set_content(
            "[q]>[e]".into(),
            LineKind::Remap(RemapLine::single("q", "e")),
            Some(LayerId::Base),
        );
        let plan = plan(&file, &p).unwrap();
        assert_eq!(plan.changed_lines, vec![2]);
        assert_eq!(plan.after, "<base>\n[q]>[e]\n<keypad>\n");
    }

    #[test]
    fn an_inserted_line_is_reported_as_changed() {
        let before = "<base>\n";
        let after = "<base>\n[q]>[w]\n";
        assert_eq!(changed_line_numbers(Some(before), after), vec![2]);
    }

    #[test]
    fn a_missing_file_is_reported_as_no_previous_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("new.txt");
        let file = sample();
        let plan = plan(&file, &p).unwrap();
        assert!(plan.before.is_none());
        assert!(!plan.is_noop());
    }

    #[test]
    fn write_verified_replaces_atomically_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("layout1.txt");
        fs::write(&p, "original").unwrap();
        let out = write_verified(&p, "replacement\n").unwrap();
        assert!(out.verified);
        assert_eq!(fs::read_to_string(&p).unwrap(), "replacement\n");
        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("adv360-tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn write_verified_creates_a_file_that_did_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("layout9.txt");
        assert!(!p.exists());
        let out = write_verified(&p, "<base>\n").unwrap();
        assert!(out.verified);
        assert_eq!(fs::read_to_string(&p).unwrap(), "<base>\n");
    }

    #[test]
    fn a_write_failure_reports_a_write_error_and_keeps_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("layout1.txt");
        fs::write(&p, "original").unwrap();
        // Point the write at a path whose parent is a file, which cannot work.
        let blocked = p.join("nested.txt");
        let err = write_verified(&blocked, "x").unwrap_err();
        assert!(matches!(
            err,
            CoreError::Write { .. } | CoreError::Disconnected { .. }
        ));
        assert_eq!(fs::read_to_string(&p).unwrap(), "original");
    }

    #[test]
    fn verification_matches_the_byte_stream_including_crlf_and_no_final_newline() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("layout1.txt");
        let contents = "\u{feff}<base>\r\n[q]>[w]";
        fs::write(&p, "<base>").unwrap();
        write_verified(&p, contents).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), contents);
    }

    #[test]
    fn an_edited_line_keeps_everything_else_intact() {
        // The core promise: only the edited line changes.
        let original = "\u{feff}# vendor note\r\n<base>\r\nWEIRD LINE\r\n[q]>[w]\r\n<keypad>\r\n";
        let mut file = parse_bytes(
            original.as_bytes(),
            "layouts/layout1.txt",
            FileRole::ProfileLayout,
        )
        .unwrap();
        let idx = file
            .lines
            .iter()
            .position(|l| matches!(l.kind, LineKind::Remap(_)))
            .unwrap();
        let new = RemapLine::single("q", "e");
        file.lines[idx].set_content(new.render(), LineKind::Remap(new), Some(LayerId::Base));
        let rendered = file.render();
        assert_eq!(
            rendered,
            "\u{feff}# vendor note\r\n<base>\r\nWEIRD LINE\r\n[q]>[e]\r\n<keypad>\r\n"
        );
        // And the untouched lines are still Origin::Original.
        let untouched = file
            .lines
            .iter()
            .filter(|l| l.origin == Origin::Original)
            .count();
        assert_eq!(untouched, file.lines.len() - 1);
        // Re-parsing the result must be lossless.
        let again = parse_bytes(
            rendered.as_bytes(),
            "layouts/layout1.txt",
            FileRole::ProfileLayout,
        )
        .unwrap();
        assert_eq!(again.render(), rendered);
        let _ = (
            Eol::Lf,
            Line::inserted(String::new(), Eol::Lf, LineKind::Blank, None),
            Origin::Inserted,
        );
    }
}
