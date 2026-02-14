#!/usr/bin/env rust-script
//! Command Runner with Logging
//!
//! A wrapper tool that executes commands while capturing and logging their output.
//! When Ctrl-C is pressed, sends 'q' to the child process for graceful shutdown
//! (useful for `flutter run` which accepts 'q' to quit gracefully).
//!
//! Usage:
//!   rust-script cmd-run.rs [OPTIONS] <command> [args...]
//!
//! Options:
//!   --log=<file>    Log output to specified file
//!   --cwd=<dir>     Change working directory before executing command
//!
//! Examples:
//!   rust-script cmd-run.rs --log=build.log flutter build apk
//!   rust-script cmd-run.rs --log=logs/test.log --cwd=project cargo test
//!
//! ```cargo
//! [dependencies]
//! chrono = "0.4"
//! anyhow = "1.0"
//! which = "6.0"
//! ctrlc = "3.4"
//!
//! [target.'cfg(windows)'.dependencies]
//! windows-sys = { version = "0.59", features = ["Win32_System_Console", "Win32_Foundation"] }
//! ```

use anyhow::{Context, Result};
use chrono::Local;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use which::which;

/// Enable raw mode on Windows stdin so each keypress is available immediately.
/// Returns the original console mode for later restoration.
#[cfg(windows)]
fn enable_raw_mode() -> Option<u32> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut original_mode: u32 = 0;
        if GetConsoleMode(handle, &mut original_mode) == 0 {
            return None;
        }
        // Disable line input and echo so each keypress is read immediately
        let raw_mode = original_mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT);
        SetConsoleMode(handle, raw_mode);
        Some(original_mode)
    }
}

/// Restore original console mode on Windows.
#[cfg(windows)]
fn restore_console_mode(mode: u32) {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            SetConsoleMode(handle, mode);
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let mut log_path: Option<PathBuf> = None;
    let mut working_dir: Option<PathBuf> = None;
    let mut cmd_args: Vec<String> = Vec::new();
    let mut command_name: Option<String> = None;

    let mut i = 1; // Skip program name
    while i < args.len() {
        if args[i].starts_with("--log=") {
            let path_str = args[i].strip_prefix("--log=").unwrap();
            log_path = Some(PathBuf::from(path_str));
        } else if args[i] == "--log" && i + 1 < args.len() {
            log_path = Some(PathBuf::from(&args[i + 1]));
            i += 1; // Skip next argument
        } else if args[i].starts_with("--cwd=") {
            let path_str = args[i].strip_prefix("--cwd=").unwrap();
            working_dir = Some(PathBuf::from(path_str));
        } else if args[i] == "--cwd" && i + 1 < args.len() {
            working_dir = Some(PathBuf::from(&args[i + 1]));
            i += 1; // Skip next argument
        } else {
            // First non-log argument is the command
            if command_name.is_none() {
                command_name = Some(args[i].clone());
            } else {
                cmd_args.push(args[i].clone());
            }
        }
        i += 1;
    }

    let command_name = command_name.ok_or_else(|| {
        anyhow::anyhow!("Usage: cmd-run [--log=FILE] [--cwd=DIR] <command> [args...]\nExample: cmd-run --log=build.log --cwd=flutter flutter build apk --release")
    })?;

    // Resolve command path
    let resolved_command = if command_name.contains(['/', '\\']) {
        PathBuf::from(&command_name)
    } else {
        which(&command_name)
            .with_context(|| format!("Command not found in PATH: {}", command_name))?
    };

    // Change to working directory if specified
    if let Some(ref cwd) = working_dir {
        std::env::set_current_dir(cwd)
            .with_context(|| format!("Failed to change directory to: {}", cwd.display()))?;
    }

    // Resolve log file path
    let log_path = log_path.map(|p| {
        if p.is_absolute() {
            p
        } else {
            std::env::current_dir().unwrap().join(p)
        }
    });

    let mut log_file_handle = if let Some(ref path) = log_path {
        // Create log directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create log directory: {}", parent.display()))?;
        }

        let mut file = File::create(path)
            .with_context(|| format!("Failed to create log file: {}", path.display()))?;

        println!("Logging to: {}\n", path.display());

        // Write log header
        let timestamp = Local::now().to_rfc3339();
        let cwd = std::env::current_dir().unwrap();
        writeln!(file, "=== Command Log ===")?;
        writeln!(file, "Timestamp: {}", timestamp)?;
        writeln!(file, "Command: {} {}", command_name, cmd_args.join(" "))?;
        writeln!(file, "Working Directory: {}", cwd.display())?;
        writeln!(file, "===================\n")?;

        Some(file)
    } else {
        None
    };

    // Enable raw mode so each keypress is available immediately (for r, R, q, etc.)
    #[cfg(windows)]
    let original_console_mode = enable_raw_mode();

    // Spawn command process with piped stdin for graceful Ctrl-C handling
    let mut child = Command::new(&resolved_command)
        .args(&cmd_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start command: {}", command_name))?;

    // Take stdin handle, wrap in Arc<Mutex> so the Ctrl-C handler can access it
    let child_stdin = Arc::new(Mutex::new(Some(child.stdin.take().expect("Failed to get stdin"))));
    let child_stdin_for_ctrlc = Arc::clone(&child_stdin);

    // Set up Ctrl-C handler: send 'q' to child for graceful shutdown
    ctrlc::set_handler(move || {
        if let Ok(mut guard) = child_stdin_for_ctrlc.lock() {
            if let Some(ref mut stdin) = *guard {
                let _ = stdin.write_all(b"q\n");
                let _ = stdin.flush();
            }
            // Drop the child stdin to signal EOF, so stdin_thread stops too
            *guard = None;
        }
    }).with_context(|| "Failed to set Ctrl-C handler")?;

    // Thread to forward parent stdin to child stdin (byte by byte for responsiveness)
    let child_stdin_for_fwd = Arc::clone(&child_stdin);
    let _stdin_thread = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut buf = [0u8; 1];
        loop {
            match stdin.lock().read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(1) => {
                    if let Ok(mut guard) = child_stdin_for_fwd.lock() {
                        if let Some(ref mut child_in) = *guard {
                            if child_in.write_all(&buf).is_err() {
                                break;
                            }
                            let _ = child_in.flush();
                        } else {
                            break; // stdin was dropped by Ctrl-C handler
                        }
                    }
                }
                _ => break,
            }
        }
    });

    // Get stdout and stderr handles
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    // Create threads to handle output
    let log_path_clone = log_path.clone();
    let stdout_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut log_file = log_path_clone.and_then(|p| File::options().append(true).open(p).ok());

        for line in reader.lines() {
            if let Ok(line) = line {
                println!("{}", line);
                if let Some(ref mut file) = log_file {
                    let _ = writeln!(file, "{}", line);
                }
            }
        }
    });

    let log_path_clone2 = log_path.clone();
    let stderr_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut log_file = log_path_clone2.and_then(|p| File::options().append(true).open(p).ok());

        for line in reader.lines() {
            if let Ok(line) = line {
                eprintln!("{}", line);
                if let Some(ref mut file) = log_file {
                    let _ = writeln!(file, "{}", line);
                }
            }
        }
    });

    // Wait for process to complete first — this closes stdout/stderr pipes
    let status = child.wait().with_context(|| format!("Failed to wait for command: {}", command_name))?;
    let exit_code = status.code().unwrap_or(1);

    // Now output threads can finish (pipes are closed)
    stdout_thread.join().expect("stdout thread panicked");
    stderr_thread.join().expect("stderr thread panicked");

    // Restore original console mode BEFORE any output
    #[cfg(windows)]
    if let Some(mode) = original_console_mode {
        restore_console_mode(mode);
    }

    // Write log footer
    if let Some(ref mut file) = log_file_handle {
        writeln!(file, "\n===================")?;
        writeln!(file, "Exit code: {}", exit_code)?;
        writeln!(file, "Finished at: {}", Local::now().to_rfc3339())?;
    }

    if !status.success() {
        eprintln!("\nCommand failed with exit code {}", exit_code);
        if let Some(ref path) = log_path {
            eprintln!("Check log file: {}", path.display());
        }
    } else {
        println!("\n✓ Command completed successfully");
    }

    // Use process::exit to force-terminate the blocked stdin thread
    std::process::exit(exit_code);
}
