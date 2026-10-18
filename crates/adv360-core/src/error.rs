//! Errors returned by the core.
//!
//! Every variant carries enough context for a user interface to explain what happened and what to
//! do next. The protected-write path deliberately returns distinct variants for "refused before
//! touching the v-Drive" versus "the write itself failed", because the recovery instructions
//! differ.

use std::path::PathBuf;

/// Anything that can go wrong in the core.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot create directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid UTF-8, so it cannot be edited safely; it is shown read-only")]
    NotUtf8 { path: PathBuf },

    #[error("{0} is not a directory")]
    NotADirectory(PathBuf),

    #[error("no Advantage360 v-Drive found: {detail}")]
    DeviceNotFound { detail: String },

    #[error("the v-Drive root {root} does not look like an Advantage360 (missing: {missing})")]
    NotAVDrive { root: PathBuf, missing: String },

    #[error(
        "validation failed with {errors} error(s); nothing was written. First problem: {first}"
    )]
    ValidationFailed { errors: usize, first: String },

    #[error("refusing to write: {reason}")]
    RefusedWrite { reason: String },

    #[error("no backup was taken, so writing is not allowed: {detail}")]
    BackupRequired { detail: String },

    #[error("profile 0 is not programmable and has no layout file")]
    ProfileZeroNotEditable,

    #[error("{path} is {role} and this tool never modifies it")]
    ReadOnlyFile { path: PathBuf, role: &'static str },

    #[error("the keyboard appears to have been disconnected during the write: {detail}")]
    Disconnected { detail: String },

    #[error("could not verify the written content: {detail}")]
    VerifyFailed { detail: String },

    #[error("unknown action token {token:?}")]
    UnknownActionToken { token: String },

    #[error("unknown position token {token:?}")]
    UnknownPositionToken { token: String },

    #[error("{0}")]
    Other(String),
}

impl CoreError {
    /// A short, stable code both interfaces can switch on.
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::Read { .. } => "read",
            CoreError::Write { .. } => "write",
            CoreError::CreateDir { .. } => "create_dir",
            CoreError::NotUtf8 { .. } => "not_utf8",
            CoreError::NotADirectory(_) => "not_a_directory",
            CoreError::DeviceNotFound { .. } => "device_not_found",
            CoreError::NotAVDrive { .. } => "not_a_vdrive",
            CoreError::ValidationFailed { .. } => "validation_failed",
            CoreError::RefusedWrite { .. } => "refused_write",
            CoreError::BackupRequired { .. } => "backup_required",
            CoreError::ProfileZeroNotEditable => "profile_zero_not_editable",
            CoreError::ReadOnlyFile { .. } => "read_only_file",
            CoreError::Disconnected { .. } => "disconnected",
            CoreError::VerifyFailed { .. } => "verify_failed",
            CoreError::UnknownActionToken { .. } => "unknown_action_token",
            CoreError::UnknownPositionToken { .. } => "unknown_position_token",
            CoreError::Other(_) => "other",
        }
    }

    /// Concrete recovery steps, in the order the operator should try them.
    pub fn recovery_hints(&self) -> Vec<String> {
        match self {
            CoreError::DeviceNotFound { .. } | CoreError::NotAVDrive { .. } => vec![
                "Mount the v-Drive from the keyboard: press and hold SmartSet, then tap the mount \
                 shortcut (F8 in the factory layout)."
                    .into(),
                "If the shortcut is unknown, see the Advantage360 User's Manual section on mounting \
                 the v-Drive."
                    .into(),
                "Then re-run the detection, or pass the mount point manually with --device PATH."
                    .into(),
            ],
            CoreError::Write { path, .. } => vec![
                format!(
                    "The dated backup is still on your computer and is untouched: restore the \
                     file with the rescue command for {}.",
                    path.display()
                ),
                "Do not close the v-Drive from the keyboard until the backup has been restored or \
                 the write retried."
                    .into(),
                "Check that the drive is not mounted read-only and that the filesystem is not full."
                    .into(),
            ],
            CoreError::VerifyFailed { detail } => vec![
                format!("The written content did not match what was intended: {detail}"),
                "The dated backup still holds the original content; restore it if the file looks \
                 wrong."
                    .into(),
                "Do not close the v-Drive from the keyboard before checking the file.".into(),
            ],
            CoreError::Disconnected { .. } => vec![
                "Reconnect the keyboard and re-mount the v-Drive.".into(),
                "The dated backup taken before the write is still valid; restore it if the layout \
                 file is now truncated."
                    .into(),
            ],
            CoreError::ValidationFailed { .. } => vec![
                "Open the differences and validation view, fix the listed lines, then write again."
                    .into(),
            ],
            CoreError::BackupRequired { .. } => vec![
                "Run the backup step first; the tool refuses to write without one on purpose.".into(),
            ],
            _ => vec![],
        }
    }
}

/// Convenience alias.
pub type CoreResult<T> = Result<T, CoreError>;
