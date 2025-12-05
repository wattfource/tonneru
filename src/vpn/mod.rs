pub mod killswitch;
pub mod wireguard;

use anyhow::{Context, Result};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::timeout;

/// Timeout for privileged operations
pub const SUDO_TIMEOUT: Duration = Duration::from_secs(5);

/// Path to the secure helper script
const HELPER_PATH: &str = "/usr/lib/tonneru/tonneru-sudo";

/// Run the tonneru-sudo helper with the given command and arguments
/// This is the single entry point for all privileged operations
pub async fn run_helper(args: &[&str]) -> Result<std::process::Output> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    
    let result = timeout(SUDO_TIMEOUT, tokio::task::spawn_blocking(move || {
        Command::new("sudo")
            .arg(HELPER_PATH)
            .args(&args)
            .output()
    })).await;
    
    match result {
        Ok(Ok(output)) => output.context("Helper execution failed"),
        Ok(Err(e)) => anyhow::bail!("Task failed: {}", e),
        Err(_) => anyhow::bail!("Command timed out (sudo may need password or user not in tonneru group)"),
    }
}

/// Run the tonneru-sudo helper with stdin input
pub async fn run_helper_with_stdin(args: &[&str], stdin_data: &str) -> Result<std::process::Output> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let stdin_data = stdin_data.to_string();
    
    let result = timeout(SUDO_TIMEOUT, tokio::task::spawn_blocking(move || {
        use std::io::Write;
        
        let mut child = Command::new("sudo")
            .arg(HELPER_PATH)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(stdin_data.as_bytes())?;
        }
        
        child.wait_with_output()
    })).await;
    
    match result {
        Ok(Ok(output)) => output.context("Helper execution failed"),
        Ok(Err(e)) => anyhow::bail!("Task failed: {}", e),
        Err(_) => anyhow::bail!("Command timed out (sudo may need password or user not in tonneru group)"),
    }
}

/// Run a command with timeout to prevent hanging on sudo password prompts
/// DEPRECATED: Use run_helper() instead for privileged operations
#[allow(dead_code)]
pub async fn run_command_with_timeout(cmd: &str, args: &[&str]) -> Result<std::process::Output> {
    let cmd = cmd.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    
    let result = timeout(SUDO_TIMEOUT, tokio::task::spawn_blocking(move || {
        Command::new(&cmd)
            .args(&args)
            .output()
    })).await;
    
    match result {
        Ok(Ok(output)) => output.context("Command execution failed"),
        Ok(Err(e)) => anyhow::bail!("Task failed: {}", e),
        Err(_) => anyhow::bail!("Command timed out (sudo may need password)"),
    }
}
