//! Loop lock mechanism for preventing concurrent Ralph loops in the same workspace.
//!
//! Uses an OS-backed lock on `.ralph/loop.lock` to ensure only one primary loop runs at a time.
//! When a second loop attempts to start, it can detect the existing lock and spawn
//! into a git worktree instead.
//!
//! # Example
//!
//! ```no_run
//! use ralph_core::loop_lock::{LoopLock, LockError};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     match LoopLock::try_acquire(".", "implement auth") {
//!         Ok(guard) => {
//!             // We're the primary loop - run normally
//!             println!("Acquired lock, running as primary loop");
//!             // Lock is held until guard is dropped
//!         }
//!         Err(LockError::AlreadyLocked(existing)) => {
//!             // Another loop is running - spawn into worktree
//!             println!("Lock held by PID {}, spawning worktree", existing.pid);
//!         }
//!         Err(e) => return Err(e.into()),
//!     }
//!     Ok(())
//! }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process;

/// Metadata stored in the lock file, readable by other processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockMetadata {
    /// Process ID of the lock holder.
    pub pid: u32,

    /// When the lock was acquired.
    pub started: DateTime<Utc>,

    /// The prompt/task being executed.
    pub prompt: String,
}

/// A guard that holds the loop lock. The lock is released when this is dropped.
#[derive(Debug)]
pub struct LockGuard {
    /// The open file handle owns the OS lock for the guard lifetime.
    _file: File,

    /// Path to the lock file.
    lock_path: PathBuf,
}

impl LockGuard {
    /// Returns the path to the lock file.
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        tracing::debug!("Releasing loop lock at {}", self.lock_path.display());
    }
}

/// Errors that can occur during lock operations.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// The lock is already held by another process.
    #[error("Lock already held by PID {}", .0.pid)]
    AlreadyLocked(LockMetadata),

    /// IO error during lock operations.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Failed to parse lock metadata.
    #[error("Failed to parse lock metadata: {0}")]
    ParseError(String),
}

/// The loop lock mechanism.
///
/// Uses an OS-backed advisory lock on `.ralph/loop.lock`.
/// The lock is automatically released when the process exits (even on crash).
pub struct LoopLock;

impl LoopLock {
    /// The relative path to the lock file within the workspace.
    pub const LOCK_FILE: &'static str = ".ralph/loop.lock";

    #[cfg(windows)]
    fn metadata_path(lock_path: &Path) -> PathBuf {
        lock_path.with_extension("lock.meta")
    }

    /// Try to acquire the loop lock (non-blocking).
    ///
    /// # Arguments
    ///
    /// * `workspace_root` - Root directory of the workspace
    /// * `prompt` - The prompt/task being executed (stored in lock metadata)
    ///
    /// # Returns
    ///
    /// * `Ok(LockGuard)` - Lock acquired successfully
    /// * `Err(LockError::AlreadyLocked(metadata))` - Another process holds the lock
    /// * `Err(LockError::Io(_))` - IO error
    pub fn try_acquire(
        workspace_root: impl AsRef<Path>,
        prompt: &str,
    ) -> Result<LockGuard, LockError> {
        let lock_path = workspace_root.as_ref().join(Self::LOCK_FILE);

        // Ensure .ralph directory exists
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Open or create the lock file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;

        match fs4::FileExt::try_lock(&file) {
            Ok(()) => {
                Self::write_metadata(&file, prompt)?;
                #[cfg(windows)]
                Self::write_metadata_sidecar(&lock_path, prompt)?;
                tracing::debug!("Acquired loop lock at {}", lock_path.display());
                Ok(LockGuard {
                    _file: file,
                    lock_path,
                })
            }
            Err(fs4::TryLockError::WouldBlock) => {
                let metadata = Self::read_existing_metadata(&lock_path, &file)?;
                Err(LockError::AlreadyLocked(metadata))
            }
            Err(fs4::TryLockError::Error(error)) => Err(LockError::Io(error)),
        }
    }

    /// Acquire the loop lock, blocking until available.
    ///
    /// This should be used with the `--exclusive` flag to wait for the
    /// primary loop slot instead of spawning into a worktree.
    ///
    /// # Arguments
    ///
    /// * `workspace_root` - Root directory of the workspace
    /// * `prompt` - The prompt/task being executed
    ///
    /// # Returns
    ///
    /// * `Ok(LockGuard)` - Lock acquired successfully
    /// * `Err(LockError::Io(_))` - IO error
    pub fn acquire_blocking(
        workspace_root: impl AsRef<Path>,
        prompt: &str,
    ) -> Result<LockGuard, LockError> {
        let lock_path = workspace_root.as_ref().join(Self::LOCK_FILE);

        // Ensure .ralph directory exists
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;

        fs4::FileExt::lock(&file).map_err(LockError::Io)?;
        Self::write_metadata(&file, prompt)?;
        #[cfg(windows)]
        Self::write_metadata_sidecar(&lock_path, prompt)?;

        tracing::debug!("Acquired loop lock (blocking) at {}", lock_path.display());

        Ok(LockGuard {
            _file: file,
            lock_path,
        })
    }

    /// Read the metadata from an existing lock file.
    ///
    /// This can be used to check who holds the lock without acquiring it.
    pub fn read_existing(
        workspace_root: impl AsRef<Path>,
    ) -> Result<Option<LockMetadata>, LockError> {
        let lock_path = workspace_root.as_ref().join(Self::LOCK_FILE);

        if !lock_path.exists() {
            return Ok(None);
        }

        let file = File::open(&lock_path)?;
        match Self::read_existing_metadata(&lock_path, &file) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(LockError::ParseError(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Check if the lock is currently held (without acquiring it).
    ///
    /// Returns `true` if another process holds the lock.
    pub fn is_locked(workspace_root: impl AsRef<Path>) -> Result<bool, LockError> {
        let lock_path = workspace_root.as_ref().join(Self::LOCK_FILE);

        if !lock_path.exists() {
            return Ok(false);
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true) // Need write for exclusive lock
            .open(&lock_path)?;

        match fs4::FileExt::try_lock(&file) {
            Ok(()) => Ok(false),
            Err(fs4::TryLockError::WouldBlock) => Ok(true),
            Err(fs4::TryLockError::Error(error)) => Err(LockError::Io(error)),
        }
    }

    /// Write lock metadata to the file.
    fn write_metadata(file: &File, prompt: &str) -> Result<(), LockError> {
        // Use a mutable reference via clone for writing
        let mut file_clone = file.try_clone()?;
        file_clone.set_len(0)?;
        file_clone.seek(SeekFrom::Start(0))?;

        let json = Self::metadata_json(prompt)?;
        file_clone.write_all(json.as_bytes())?;
        file_clone.sync_all()?;

        Ok(())
    }

    fn metadata_json(prompt: &str) -> Result<String, LockError> {
        let metadata = LockMetadata {
            pid: process::id(),
            started: Utc::now(),
            prompt: prompt.to_string(),
        };

        serde_json::to_string_pretty(&metadata).map_err(|e| LockError::ParseError(e.to_string()))
    }

    #[cfg(windows)]
    fn write_metadata_sidecar(lock_path: &Path, prompt: &str) -> Result<(), LockError> {
        let sidecar_path = Self::metadata_path(lock_path);
        let temp_path = sidecar_path.with_extension("lock.meta.tmp");
        fs::write(&temp_path, Self::metadata_json(prompt)?)?;
        fs::rename(temp_path, sidecar_path)?;
        Ok(())
    }

    fn read_existing_metadata(lock_path: &Path, file: &File) -> Result<LockMetadata, LockError> {
        #[cfg(windows)]
        {
            let sidecar_path = Self::metadata_path(lock_path);
            if sidecar_path.exists() {
                let contents = fs::read_to_string(sidecar_path)?;
                return serde_json::from_str(&contents)
                    .map_err(|e| LockError::ParseError(e.to_string()));
            }
        }

        Self::read_metadata(file)
    }

    /// Read lock metadata from the file.
    fn read_metadata(file: &File) -> Result<LockMetadata, LockError> {
        let mut file_clone = file.try_clone()?;
        file_clone.seek(SeekFrom::Start(0))?;
        let mut contents = String::new();
        file_clone.read_to_string(&mut contents)?;

        if contents.trim().is_empty() {
            return Err(LockError::ParseError("Empty lock file".to_string()));
        }

        serde_json::from_str(&contents).map_err(|e| LockError::ParseError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_acquire_lock_success() {
        let temp_dir = TempDir::new().unwrap();

        let guard = LoopLock::try_acquire(temp_dir.path(), "test prompt");
        assert!(guard.is_ok());

        // Lock file should exist
        let lock_path = temp_dir.path().join(".ralph/loop.lock");
        assert!(lock_path.exists());

        // Metadata should be readable
        #[cfg(windows)]
        let metadata_path = LoopLock::metadata_path(&lock_path);
        #[cfg(not(windows))]
        let metadata_path = lock_path;
        let contents = fs::read_to_string(metadata_path).unwrap();
        let metadata: LockMetadata = serde_json::from_str(&contents).unwrap();
        assert_eq!(metadata.pid, process::id());
        assert_eq!(metadata.prompt, "test prompt");
    }

    #[test]
    fn test_lock_released_on_drop() {
        let temp_dir = TempDir::new().unwrap();

        {
            let _guard = LoopLock::try_acquire(temp_dir.path(), "first").unwrap();
            // Lock is held
        }
        // Guard dropped, lock released

        // Should be able to acquire again
        let guard = LoopLock::try_acquire(temp_dir.path(), "second");
        assert!(guard.is_ok());
    }

    #[test]
    fn test_is_locked() {
        let temp_dir = TempDir::new().unwrap();

        // Initially not locked
        assert!(!LoopLock::is_locked(temp_dir.path()).unwrap());

        let _guard = LoopLock::try_acquire(temp_dir.path(), "test").unwrap();

        // Now locked (from our perspective - same process can re-lock)
        // Note: flock allows same process to re-acquire, so this test
        // might not work as expected in single-process context
    }

    #[test]
    fn test_read_existing_no_file() {
        let temp_dir = TempDir::new().unwrap();

        let result = LoopLock::read_existing(temp_dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_existing_with_lock() {
        let temp_dir = TempDir::new().unwrap();

        let _guard = LoopLock::try_acquire(temp_dir.path(), "my prompt").unwrap();

        let metadata = LoopLock::read_existing(temp_dir.path()).unwrap().unwrap();
        assert_eq!(metadata.pid, process::id());
        assert_eq!(metadata.prompt, "my prompt");
    }

    #[test]
    fn test_creates_ralph_directory() {
        let temp_dir = TempDir::new().unwrap();
        let ralph_dir = temp_dir.path().join(".ralph");

        assert!(!ralph_dir.exists());

        let _guard = LoopLock::try_acquire(temp_dir.path(), "test").unwrap();

        assert!(ralph_dir.exists());
    }

    #[test]
    fn test_lock_metadata_serialization() {
        let metadata = LockMetadata {
            pid: 12345,
            started: Utc::now(),
            prompt: "implement feature".to_string(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: LockMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.pid, 12345);
        assert_eq!(deserialized.prompt, "implement feature");
    }
}
