use std::path::{Path, PathBuf};

use anyhow::Context;
use ralph_core::LoopLock;

pub(crate) const LOOP_LOCK_FILE: &str = ".ralph/loop.lock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockState {
    Active,
    Inactive,
    Stale,
}

pub(crate) fn lock_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(LOOP_LOCK_FILE)
}

pub(crate) fn lock_state(workspace_root: &Path) -> anyhow::Result<LockState> {
    let path = lock_path(workspace_root);

    if !path.exists() {
        return Ok(LockState::Inactive);
    }

    match LoopLock::is_locked(workspace_root)
        .with_context(|| format!("check lock state at {}", path.display()))?
    {
        true => Ok(LockState::Active),
        false => Ok(LockState::Stale),
    }
}
