pub mod killswitch;
pub mod wireguard;

use anyhow::{Context, Result};
use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

const SUDO_TIMEOUT: Duration = Duration::from_secs(5);

/// Run a command with timeout to prevent hanging on sudo password prompts
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
