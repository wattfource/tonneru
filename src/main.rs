mod app;
mod config;
mod network;
mod theme;
mod ui;
mod vpn;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use app::{App, Popup};

#[derive(Parser, Debug)]
#[command(name = "tonneru")]
#[command(author = "Sean Fournier")]
#[command(version = "0.1.0")]
#[command(about = "A terminal-friendly VPN manager for Arch Linux / Omarchy")]
struct Args {
    /// Run in daemon mode (for waybar integration)
    #[arg(short, long)]
    daemon: bool,

    /// Output current VPN status as JSON (for waybar)
    #[arg(short, long)]
    status: bool,

    /// Connect to a specific VPN profile
    #[arg(short, long)]
    connect: Option<String>,

    /// Disconnect from VPN
    #[arg(long)]
    disconnect: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Handle CLI-only commands
    if args.status {
        return print_status().await;
    }

    if args.disconnect {
        return disconnect_vpn().await;
    }

    if let Some(profile) = args.connect {
        return connect_vpn(&profile).await;
    }

    if args.daemon {
        return run_daemon().await;
    }

    // Run TUI
    run_tui().await
}

async fn print_status() -> Result<()> {
    let status = vpn::wireguard::get_status().await?;
    
    // Determine effective state (connected AND fresh handshake)
    let is_effectively_connected = status.connected && !status.handshake_stale;
    
    // Determine class for waybar
    // If connected but stale, we show as degraded/disconnected so user notices
    let class = if is_effectively_connected { 
        "connected" 
    } else if status.connected {
        "degraded" // Connected but stale
    } else { 
        "disconnected" 
    };
    
    // Build tooltip with health info
    let tooltip = if status.connected {
        let mut lines = vec![
            format!("󰒘 {}", status.interface.as_deref().unwrap_or("VPN")),
        ];
        
        if let Some(endpoint) = &status.endpoint {
            lines.push(format!("󰖟 {}", endpoint));
        }
        
        if let (Some(rx), Some(tx)) = (&status.transfer_rx, &status.transfer_tx) {
            lines.push(format!("↓ {}  ↑ {}", rx, tx));
        }
        
        // Health warnings
        if !status.routing_ok {
            lines.push("⚠ Routing not configured".to_string());
        }
        if status.handshake_stale {
            lines.push("⏳ Handshake stale (connection lost?)".to_string());
        }
        
        lines.join("\n")
    } else {
        "VPN disconnected\nClick to manage".to_string()
    };
    
    // Output waybar-compatible JSON
    let output = serde_json::json!({
        "text": if status.connected { 
            status.interface.as_deref().unwrap_or("VPN").to_string()
        } else { 
            String::new()
        },
        "tooltip": tooltip,
        "class": class,
        "alt": class, // Use class as alt text for format-icons
        "connected": status.connected,
        "interface": status.interface,
        "endpoint": status.endpoint,
        "healthy": is_effectively_connected && status.routing_ok
    });
    
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

async fn connect_vpn(profile: &str) -> Result<()> {
    vpn::wireguard::connect(profile).await?;
    notify("tonneru", &format!("Connected to {}", profile))?;
    Ok(())
}

async fn disconnect_vpn() -> Result<()> {
    vpn::wireguard::disconnect().await?;
    notify("tonneru", "VPN disconnected")?;
    Ok(())
}

async fn run_daemon() -> Result<()> {
    // Daemon mode for auto-connect based on network rules
    tracing::info!("Starting tonneru daemon");
    network::monitor::start_monitoring().await
}

async fn run_tui() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new().await?;

    // Main loop
    let result = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') if app.popup == Popup::None => return Ok(()),
                        KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                            return Ok(())
                        }
                        _ => {
                            // Handle key and catch any errors to prevent crashes
                            if let Err(e) = app.handle_key(key).await {
                                app.status_message = Some(format!("Error: {}", e));
                            }
                        }
                    }
                }
            }
        }

        // Periodic refresh
        let _ = app.tick().await;
    }
}

fn notify(summary: &str, body: &str) -> Result<()> {
    notify_rust::Notification::new()
        .summary(summary)
        .body(body)
        .icon("network-vpn")
        .show()?;
    Ok(())
}

