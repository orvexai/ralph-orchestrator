//! Platform-specific control for already-running Ralph loop processes.

use std::io;

/// Requests termination of a loop process and its descendants.
pub(crate) fn terminate_process(pid: u32, force: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let signal = if force {
            Signal::SIGKILL
        } else {
            Signal::SIGTERM
        };
        kill(Pid::from_raw(pid as i32), signal).map_err(io::Error::other)
    }

    #[cfg(windows)]
    {
        let taskkill = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .map(|root| root.join("System32").join("taskkill.exe"))
            .unwrap_or_else(|| std::path::PathBuf::from("taskkill.exe"));
        let mut command = std::process::Command::new(taskkill);
        command.args(["/PID", &pid.to_string(), "/T"]);
        if force {
            command.arg("/F");
        }

        let output = command.output()?;
        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not found") || stderr.contains("no running instance") {
            Ok(())
        } else {
            Err(io::Error::other(format!("taskkill failed: {stderr}")))
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, force);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process termination is not supported on this platform",
        ))
    }
}
