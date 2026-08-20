use std::process::Command;
use tempfile::TempDir;

fn run_ralph(temp_path: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ralph"))
        .args(args)
        .current_dir(temp_path)
        .output()
        .expect("execute ralph")
}

fn run_ralph_with_home(
    temp_path: &std::path::Path,
    home_path: &std::path::Path,
    args: &[&str],
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ralph"))
        .args(args)
        .current_dir(temp_path)
        .env("HOME", home_path)
        .env("USERPROFILE", home_path)
        .output()
        .expect("execute ralph")
}

#[test]
fn test_run_dry_run_succeeds() {
    let temp_dir = TempDir::new().expect("temp dir");
    let temp_path = temp_dir.path();

    let output = run_ralph(
        temp_path,
        &[
            "run",
            "--dry-run",
            "--skip-preflight",
            "--prompt",
            "hello world",
            "--completion-promise",
            "done",
            "--max-iterations",
            "1",
            "--backend",
            "claude",
            "--no-tui",
        ],
    );

    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dry run mode"), "stdout: {stdout}");
}

#[test]
fn test_run_dry_run_uses_user_scoped_config_defaults() {
    let temp_dir = TempDir::new().expect("temp dir");
    let home_dir = TempDir::new().expect("temp home");
    let temp_path = temp_dir.path();
    let user_config_dir = home_dir.path().join(".ralph");
    std::fs::create_dir_all(&user_config_dir).expect("create user config dir");
    std::fs::write(
        user_config_dir.join("config.yml"),
        r"
cli:
  backend: claude
event_loop:
  max_iterations: 7
",
    )
    .expect("write user config");

    let output = run_ralph_with_home(
        temp_path,
        home_dir.path(),
        &[
            "run",
            "--dry-run",
            "--skip-preflight",
            "--prompt",
            "hello world",
            "--no-tui",
        ],
    );

    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Backend: claude"), "stdout: {stdout}");
    assert!(stdout.contains("Max iterations: 7"), "stdout: {stdout}");
}

#[test]
fn test_run_dry_run_local_config_overrides_user_scoped_defaults() {
    let temp_dir = TempDir::new().expect("temp dir");
    let home_dir = TempDir::new().expect("temp home");
    let temp_path = temp_dir.path();
    let user_config_dir = home_dir.path().join(".ralph");
    std::fs::create_dir_all(&user_config_dir).expect("create user config dir");
    std::fs::write(
        user_config_dir.join("config.yml"),
        r"
cli:
  backend: claude
event_loop:
  max_iterations: 7
",
    )
    .expect("write user config");
    std::fs::write(
        temp_path.join("ralph.yml"),
        r"
cli:
  backend: gemini
event_loop:
  max_iterations: 3
",
    )
    .expect("write local config");

    let output = run_ralph_with_home(
        temp_path,
        home_dir.path(),
        &[
            "run",
            "--dry-run",
            "--skip-preflight",
            "--prompt",
            "hello world",
            "--no-tui",
        ],
    );

    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Backend: gemini"), "stdout: {stdout}");
    assert!(stdout.contains("Max iterations: 3"), "stdout: {stdout}");
}

#[test]
fn test_run_smoke_executes_user_scoped_hook_during_real_loop() {
    let temp_dir = TempDir::new().expect("temp dir");
    let home_dir = TempDir::new().expect("temp home");
    let temp_path = temp_dir.path();
    let user_config_dir = home_dir.path().join(".ralph");
    std::fs::create_dir_all(&user_config_dir).expect("create user config dir");

    let hook_marker = temp_path.join("global-hook-ran.txt");
    let hook_script_name = if cfg!(windows) {
        "global-hook.cmd"
    } else {
        "global-hook.sh"
    };
    let backend_script_name = if cfg!(windows) {
        "backend-complete.cmd"
    } else {
        "backend-complete.sh"
    };
    let hook_script = temp_path.join(hook_script_name);
    let backend_script = temp_path.join(backend_script_name);

    if cfg!(windows) {
        std::fs::write(
            &hook_script,
            "@echo off\r\necho hook ran > \"%~dp0global-hook-ran.txt\"\r\n",
        )
        .expect("write hook script");
        std::fs::write(
            &backend_script,
            format!(
                "@echo off\r\n\"{}\" emit LOOP_COMPLETE smoke-done\r\n",
                env!("CARGO_BIN_EXE_ralph")
            ),
        )
        .expect("write backend script");
    } else {
        std::fs::write(
            &hook_script,
            format!(
                "#!/bin/sh\nprintf 'hook ran' > \"{}\"\n",
                hook_marker.display()
            ),
        )
        .expect("write hook script");
        std::fs::write(
            &backend_script,
            format!(
                "#!/bin/sh\ncat >/dev/null\n\"{}\" emit LOOP_COMPLETE smoke-done\n",
                env!("CARGO_BIN_EXE_ralph")
            ),
        )
        .expect("write backend script");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        for path in [&hook_script, &backend_script] {
            let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).expect("set executable permissions");
        }
    }

    let hook_command = if cfg!(windows) {
        r#"["cmd.exe", "/C", "global-hook.cmd"]"#
    } else {
        r#"["./global-hook.sh"]"#
    };
    std::fs::write(
        user_config_dir.join("config.yml"),
        format!(
            r#"
hooks:
  enabled: true
  events:
    pre.loop.start:
      - name: global-hook
        command: {hook_command}
        on_error: warn
"#
        ),
    )
    .expect("write user config");

    let backend_config = if cfg!(windows) {
        r#"  command: "cmd.exe"
  args: ["/C", "backend-complete.cmd"]"#
    } else {
        r#"  command: "./backend-complete.sh""#
    };
    std::fs::write(
        temp_path.join("ralph.yml"),
        format!(
            r#"
cli:
  backend: custom
{backend_config}
  prompt_mode: stdin
event_loop:
  max_iterations: 3
  max_runtime_seconds: 10
"#
        ),
    )
    .expect("write local config");

    let output = run_ralph_with_home(
        temp_path,
        home_dir.path(),
        &["run", "--no-tui", "--prompt", "smoke test"],
    );

    assert!(
        output.status.success(),
        "run failed: {}\nstdout:{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        hook_marker.exists(),
        "expected global hook marker at {}; stderr: {}; stdout: {}",
        hook_marker.display(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn test_run_smoke_local_hook_overrides_user_scoped_hook_during_real_loop() {
    let temp_dir = TempDir::new().expect("temp dir");
    let home_dir = TempDir::new().expect("temp home");
    let temp_path = temp_dir.path();
    let user_config_dir = home_dir.path().join(".ralph");
    std::fs::create_dir_all(&user_config_dir).expect("create user config dir");

    let global_marker = temp_path.join("global-hook-ran.txt");
    let local_marker = temp_path.join("local-hook-ran.txt");
    let global_hook_name = if cfg!(windows) {
        "global-hook.cmd"
    } else {
        "global-hook.sh"
    };
    let local_hook_name = if cfg!(windows) {
        "local-hook.cmd"
    } else {
        "local-hook.sh"
    };
    let backend_script_name = if cfg!(windows) {
        "backend-complete.cmd"
    } else {
        "backend-complete.sh"
    };
    let global_hook_script = temp_path.join(global_hook_name);
    let local_hook_script = temp_path.join(local_hook_name);
    let backend_script = temp_path.join(backend_script_name);

    if cfg!(windows) {
        std::fs::write(
            &global_hook_script,
            "@echo off\r\necho global hook ran > \"%~dp0global-hook-ran.txt\"\r\n",
        )
        .expect("write global hook script");
        std::fs::write(
            &local_hook_script,
            "@echo off\r\necho local hook ran > \"%~dp0local-hook-ran.txt\"\r\n",
        )
        .expect("write local hook script");
        std::fs::write(
            &backend_script,
            format!(
                "@echo off\r\n\"{}\" emit LOOP_COMPLETE smoke-done\r\n",
                env!("CARGO_BIN_EXE_ralph")
            ),
        )
        .expect("write backend script");
    } else {
        std::fs::write(
            &global_hook_script,
            format!(
                "#!/bin/sh\nprintf 'global hook ran' > \"{}\"\n",
                global_marker.display()
            ),
        )
        .expect("write global hook script");
        std::fs::write(
            &local_hook_script,
            format!(
                "#!/bin/sh\nprintf 'local hook ran' > \"{}\"\n",
                local_marker.display()
            ),
        )
        .expect("write local hook script");
        std::fs::write(
            &backend_script,
            format!(
                "#!/bin/sh\ncat >/dev/null\n\"{}\" emit LOOP_COMPLETE smoke-done\n",
                env!("CARGO_BIN_EXE_ralph")
            ),
        )
        .expect("write backend script");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        for path in [&global_hook_script, &local_hook_script, &backend_script] {
            let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).expect("set executable permissions");
        }
    }

    let global_hook_command = if cfg!(windows) {
        r#"["cmd.exe", "/C", "global-hook.cmd"]"#
    } else {
        r#"["./global-hook.sh"]"#
    };
    std::fs::write(
        user_config_dir.join("config.yml"),
        format!(
            r#"
hooks:
  enabled: true
  events:
    pre.loop.start:
      - name: global-hook
        command: {global_hook_command}
        on_error: warn
"#
        ),
    )
    .expect("write user config");

    let local_hook_command = if cfg!(windows) {
        r#"["cmd.exe", "/C", "local-hook.cmd"]"#
    } else {
        r#"["./local-hook.sh"]"#
    };
    let backend_config = if cfg!(windows) {
        r#"  command: "cmd.exe"
  args: ["/C", "backend-complete.cmd"]"#
    } else {
        r#"  command: "./backend-complete.sh""#
    };
    std::fs::write(
        temp_path.join("ralph.yml"),
        format!(
            r#"
cli:
  backend: custom
{backend_config}
  prompt_mode: stdin
hooks:
  enabled: true
  events:
    pre.loop.start:
      - name: local-hook
        command: {local_hook_command}
        on_error: warn
event_loop:
  max_iterations: 3
  max_runtime_seconds: 10
"#
        ),
    )
    .expect("write local config");

    let output = run_ralph_with_home(
        temp_path,
        home_dir.path(),
        &["run", "--no-tui", "--prompt", "smoke override test"],
    );

    assert!(
        output.status.success(),
        "run failed: {}\nstdout:{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        local_marker.exists(),
        "expected local hook marker at {}; stderr: {}; stdout: {}",
        local_marker.display(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !global_marker.exists(),
        "did not expect global hook marker at {}",
        global_marker.display()
    );
}

#[cfg(unix)]
#[test]
fn test_run_autonomous_mode_does_not_apply_adapter_wall_clock_timeout() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().expect("temp dir");
    let temp_path = temp_dir.path();
    let backend_script = temp_path.join("slow-complete.sh");

    std::fs::write(
        &backend_script,
        format!(
            "#!/bin/sh\ncat >/dev/null\nprintf 'heartbeat-1\\n'\nsleep 0.4\nprintf 'heartbeat-2\\n'\nsleep 0.4\nprintf 'heartbeat-3\\n'\nsleep 0.4\n\"{}\" emit LOOP_COMPLETE autonomous-done\n",
            env!("CARGO_BIN_EXE_ralph")
        ),
    )
    .expect("write backend script");

    let mut permissions = std::fs::metadata(&backend_script)
        .expect("metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&backend_script, permissions).expect("set executable permissions");

    std::fs::write(
        temp_path.join("ralph.yml"),
        r#"
cli:
  backend: custom
  command: "./slow-complete.sh"
  prompt_mode: stdin
adapters:
  claude:
    timeout: 1
event_loop:
  completion_promise: "LOOP_COMPLETE"
  max_iterations: 1
  max_runtime_seconds: 10
"#,
    )
    .expect("write config");

    let output = run_ralph(
        temp_path,
        &[
            "run",
            "--autonomous",
            "--skip-preflight",
            "--prompt",
            "slow autonomous test",
        ],
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("heartbeat-3"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Event emitted: LOOP_COMPLETE"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("Execution inactivity timeout reached, sending SIGTERM"),
        "stderr: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn test_run_text_backend_exits_promptly_after_event_emitted_even_if_backend_lingers() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let temp_dir = TempDir::new().expect("temp dir");
    let temp_path = temp_dir.path();
    let backend_script = temp_path.join("emit-then-hang.sh");

    std::fs::write(
        &backend_script,
        format!(
            "#!/bin/sh\ncat >/dev/null\n\"{}\" emit LOOP_COMPLETE lingering-done\ntrap '' TERM\nsleep 30\n",
            env!("CARGO_BIN_EXE_ralph")
        ),
    )
    .expect("write backend script");

    let mut permissions = std::fs::metadata(&backend_script)
        .expect("metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&backend_script, permissions).expect("set executable permissions");

    std::fs::write(
        temp_path.join("ralph.yml"),
        r#"
cli:
  backend: custom
  command: "./emit-then-hang.sh"
  prompt_mode: stdin
event_loop:
  completion_promise: "LOOP_COMPLETE"
  max_iterations: 5
  max_runtime_seconds: 60
adapters:
  custom:
    timeout: 30
"#,
    )
    .expect("write config");

    let started = Instant::now();
    let output = run_ralph(
        temp_path,
        &[
            "run",
            "--autonomous",
            "--skip-preflight",
            "--prompt",
            "emit completion and stop promptly",
        ],
    );
    let elapsed = started.elapsed();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "run failed: {stderr}\nstdout:{stdout}"
    );
    assert!(
        stdout.contains("Event emitted: LOOP_COMPLETE"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "loop should terminate shortly after event emission, not wait for the full backend timeout; elapsed={elapsed:?}\nstdout:{stdout}\nstderr:{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn test_run_text_backend_exits_promptly_after_event_emitted_even_if_backend_keeps_logging() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let temp_dir = TempDir::new().expect("temp dir");
    let temp_path = temp_dir.path();
    let backend_script = temp_path.join("emit-then-heartbeat.sh");

    std::fs::write(
        &backend_script,
        format!(
            "#!/bin/sh\ncat >/dev/null\n\"{}\" emit LOOP_COMPLETE noisy-done\ntrap '' TERM\nwhile :; do\n  printf 'heartbeat-after-event\\n'\n  sleep 1\ndone\n",
            env!("CARGO_BIN_EXE_ralph")
        ),
    )
    .expect("write backend script");

    let mut permissions = std::fs::metadata(&backend_script)
        .expect("metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&backend_script, permissions).expect("set executable permissions");

    std::fs::write(
        temp_path.join("ralph.yml"),
        r#"
cli:
  backend: custom
  command: "./emit-then-heartbeat.sh"
  prompt_mode: stdin
event_loop:
  completion_promise: "LOOP_COMPLETE"
  max_iterations: 5
  max_runtime_seconds: 60
adapters:
  custom:
    timeout: 30
"#,
    )
    .expect("write config");

    let started = Instant::now();
    let output = run_ralph(
        temp_path,
        &[
            "run",
            "--autonomous",
            "--skip-preflight",
            "--prompt",
            "emit completion and stop promptly even if the backend keeps logging",
        ],
    );
    let elapsed = started.elapsed();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "run failed: {stderr}\nstdout:{stdout}"
    );
    assert!(
        stdout.contains("Event emitted: LOOP_COMPLETE"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("heartbeat-after-event"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "loop should terminate after a fixed post-event grace period even if the backend keeps logging; elapsed={elapsed:?}\nstdout:{stdout}\nstderr:{stderr}"
    );
}

#[test]
fn test_run_continue_requires_scratchpad() {
    let temp_dir = TempDir::new().expect("temp dir");
    let temp_path = temp_dir.path();

    let output = run_ralph(
        temp_path,
        &["run", "--continue", "--dry-run", "--skip-preflight"],
    );

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Cannot continue: scratchpad not found"),
        "stderr: {stderr}"
    );
}
