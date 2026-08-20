//! Platform-specific descendant-process cleanup.

#[cfg(windows)]
use std::io;

#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{CommandWrap, KillOnDrop};

/// Wraps a Tokio command in the platform's owned process-tree primitive.
///
/// Unix uses a process group; Windows uses a Job Object. Kill-on-drop is
/// enabled on both platforms so cancellation cannot orphan the command tree.
pub(crate) fn wrap_command(command: tokio::process::Command) -> CommandWrap {
    let mut command = CommandWrap::from(command);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    command.wrap(KillOnDrop);
    command
}

/// Force-terminates a process and its descendants on Windows.
#[cfg(windows)]
pub(crate) fn force_kill_tree(pid: u32) -> io::Result<()> {
    let taskkill = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .map(|root| root.join("System32").join("taskkill.exe"))
        .unwrap_or_else(|| std::path::PathBuf::from("taskkill.exe"));
    let output = std::process::Command::new(taskkill)
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    // The process may have exited between discovery and cleanup.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("not found") || stderr.contains("no running instance") {
        Ok(())
    } else {
        Err(io::Error::other(format!("taskkill failed: {stderr}")))
    }
}
