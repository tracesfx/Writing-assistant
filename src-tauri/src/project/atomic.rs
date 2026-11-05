//! Atomic file writes: temp file → fsync → rename (SPEC §2 non-negotiable 6).
//!
//! A crash at any point leaves either the old file or the new file, never a
//! half-written one. Leftover temp files are named `.<name>.<ulid>.margin-tmp`
//! and are removed by [`cleanup_temp_files`] on project open.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

pub const TEMP_SUFFIX: &str = ".margin-tmp";

fn temp_path_for(path: &Path) -> io::Result<PathBuf> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    Ok(dir.join(format!(".{name}.{}{TEMP_SUFFIX}", ulid::Ulid::generate())))
}

/// Write `bytes` to `path` atomically, creating parent directories.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = temp_path_for(path)?;
    let result = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        rename_with_retry(&tmp, path)?;
        sync_dir(path);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Rename, retrying briefly on Windows sharing violations (antivirus and
/// search indexers often hold a file open for a few milliseconds).
pub fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < 8 && is_transient(&e) => {
                attempt += 1;
                sleep(Duration::from_millis(15 * attempt));
            }
            Err(e) => return Err(e),
        }
    }
}

fn is_transient(e: &io::Error) -> bool {
    // ERROR_ACCESS_DENIED (5), ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33)
    matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33))
        || e.kind() == io::ErrorKind::PermissionDenied
}

#[cfg(unix)]
fn sync_dir(path: &Path) {
    if let Some(dir) = path.parent() {
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) {
    // NTFS renames are journaled metadata operations; directory handles
    // cannot be fsynced through std on Windows.
}

/// Remove leftover temp files (from a crash mid-write) under `dir`, recursively.
/// Returns how many were removed.
pub fn cleanup_temp_files(dir: &Path) -> io::Result<usize> {
    let mut removed = 0;
    if !dir.is_dir() {
        return Ok(0);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            let name = entry.file_name();
            // Never descend into the hidden git repository.
            if name == "history" || name == ".git" {
                continue;
            }
            removed += cleanup_temp_files(&p)?;
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(TEMP_SUFFIX))
        {
            fs::remove_file(&p)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("a.md");
        atomic_write(&p, b"one").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "one");
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "two");
        // No temp files left behind.
        let leftovers: Vec<_> = fs::read_dir(p.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn crash_before_rename_leaves_original_intact() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("scene.md");
        atomic_write(&p, b"original").unwrap();
        // Simulate a crash after writing the temp file but before the rename.
        let tmp = temp_path_for(&p).unwrap();
        fs::write(&tmp, b"half-writ").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "original");
        // Recovery on next open removes the partial temp file.
        assert_eq!(cleanup_temp_files(dir.path()).unwrap(), 1);
        assert!(!tmp.exists());
        assert_eq!(fs::read_to_string(&p).unwrap(), "original");
    }

    #[test]
    fn failed_write_cleans_up_temp() {
        let dir = tempfile::tempdir().unwrap();
        // Target is a directory: the rename fails, and no temp file may remain.
        let target = dir.path().join("is_dir");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("x"), b"x").unwrap();
        assert!(atomic_write(&target, b"data").is_err());
        let temps = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(TEMP_SUFFIX))
            .count();
        assert_eq!(temps, 0);
    }
}
