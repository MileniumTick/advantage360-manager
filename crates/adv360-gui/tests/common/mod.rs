//! Shared fixture helpers for the GUI integration tests.
//!
//! A synthetic v-Drive is built in the system temp directory; no dependency is added for it.

use std::path::PathBuf;

use adv360_core::layer::LayerId;
use adv360_core::session::{Session, SessionOptions};

/// A throwaway v-Drive on disk, removed when dropped.
pub struct TempDrive {
    /// The directory that looks like the keyboard's mount point.
    pub root: PathBuf,
    base: PathBuf,
}

impl TempDrive {
    /// Create a synthetic v-Drive with all nine profiles, a lighting file and settings.txt.
    pub fn new(tag: &str) -> TempDrive {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base =
            std::env::temp_dir().join(format!("adv360-gui-{tag}-{}-{nanos}", std::process::id()));
        let root = base.join("ADV360");
        std::fs::create_dir_all(root.join("layouts")).unwrap();
        std::fs::create_dir_all(root.join("lighting")).unwrap();
        std::fs::write(root.join("settings.txt"), "never touch\n").unwrap();
        let headers: String = LayerId::all()
            .iter()
            .map(|layer| format!("{}\n", layer.header()))
            .collect();
        for profile in 1..=9u8 {
            std::fs::write(
                root.join(format!("layouts/layout{profile}.txt")),
                headers.clone(),
            )
            .unwrap();
        }
        std::fs::write(
            root.join("lighting/led1.txt"),
            "[IND1]>[caps][255][000][000]\n",
        )
        .unwrap();
        TempDrive { root, base }
    }

    /// Where this fixture's dated backups go.
    pub fn backups(&self) -> PathBuf {
        self.base.join("backups")
    }
}

impl Drop for TempDrive {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Open a session on the fixture with a deterministic `us` layout.
pub fn open(drive: &TempDrive) -> Session {
    Session::open_at(
        &drive.root,
        SessionOptions {
            backup_root: drive.backups(),
            layout_override: Some(("us".to_string(), None)),
            ..Default::default()
        },
    )
    .expect("the synthetic v-Drive must open")
}
