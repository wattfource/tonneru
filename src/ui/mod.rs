mod components;

use std::sync::OnceLock;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect, Alignment},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::app::{App, Popup, Section};
use crate::theme::Theme;

// Load theme colors from system (Omarchy/Hyprland) once at startup
static THEME: OnceLock<Theme> = OnceLock::new();

fn theme() -> &'static Theme {
    THEME.get_or_init(Theme::load)
}

// Helper functions to get theme colors
fn accent() -> Color { theme().accent }
fn accent_bright() -> Color { theme().accent_bright }
fn inactive() -> Color { theme().inactive }
fn success() -> Color { theme().success }
fn warning() -> Color { theme().warning }
fn danger() -> Color { theme().danger }
fn text() -> Color { theme().text }
fn text_dim() -> Color { theme().text_dim }
fn bg_selected() -> Color { theme().bg_selected }
fn header() -> Color { theme().header }

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    
    // Responsive layout based on terminal height
    // Networks, Tunnels, and Kill Switch boxes
    let (networks_height, tunnels_height) = if area.height < 25 {
        // Small terminal - use minimum heights
        (Constraint::Min(4), Constraint::Min(4))
    } else {
        // Equal split for both boxes (minus kill switch box height)
        (Constraint::Ratio(1, 2), Constraint::Ratio(1, 2))
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(0)
        .constraints([
            Constraint::Length(1),               // Info line
            networks_height,                     // Networks box
            tunnels_height,                      // Tunnels box
            Constraint::Length(3),               // Kill Switch box (one-liner with border)
            Constraint::Length(1),               // Footer
        ])
        .split(area);

    draw_info_line(f, app, chunks[0]);
    draw_networks_box(f, app, chunks[1]);
    draw_tunnels_box(f, app, chunks[2]);
    draw_killswitch_box(f, app, chunks[3]);
    draw_footer(f, app, chunks[4]);

    // Draw popups on top
    match app.popup {
        Popup::None => {}
        Popup::FileBrowser => draw_file_browser(f, app),
        Popup::ConfigPreview => draw_config_preview(f, app),
        Popup::ManualConfig => draw_manual_config(f, app),
        Popup::Help => draw_help_popup(f),
        Popup::Confirm => draw_confirm_popup(f, app),
    }
}

fn draw_killswitch_box(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.section == Section::KillSwitch;
    let border_color = if is_active { accent() } else { inactive() };
    let title_style = if is_active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(inactive())
    };

    let block = Block::default()
        .title(Span::styled(" Internet (k)ill Switch ", title_style))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    // Kill switch status
    let (status_icon, status_text, status_color) = if app.kill_switch_enabled {
        ("󰯄", "ENABLED - All traffic blocked except VPN", danger())
    } else {
        ("󰒙", "Disabled - Traffic allowed without VPN", text_dim())
    };

    // Action hint
    let action_hint = if is_active {
        vec![
            Span::styled("  ", Style::default()),
            Span::styled(status_icon, Style::default().fg(status_color)),
            Span::styled(format!(" {} ", status_text), Style::default().fg(status_color)),
            Span::styled("│ ", Style::default().fg(inactive())),
            Span::styled("Space", Style::default().fg(accent())),
            Span::styled("/", Style::default().fg(inactive())),
            Span::styled("k", Style::default().fg(accent())),
            Span::styled(" toggle", Style::default().fg(text_dim())),
        ]
    } else {
        vec![
            Span::styled("  ", Style::default()),
            Span::styled(status_icon, Style::default().fg(status_color)),
            Span::styled(format!(" {}", status_text), Style::default().fg(status_color)),
        ]
    };

    let content = Paragraph::new(Line::from(action_hint))
        .block(block);

    f.render_widget(content, area);
}

fn draw_info_line(f: &mut Frame, app: &App, area: Rect) {
    // Priority: pending change countdown > status message > info message > ready
    let line = if let Some(ref pending) = app.pending_change {
        // Show countdown with action description
        let action_text = match pending.action {
            crate::app::PendingAction::Connect => format!("Connect to {}", pending.tunnel_name.as_deref().unwrap_or("?")),
            crate::app::PendingAction::Disconnect => "Disconnect VPN".to_string(),
            crate::app::PendingAction::Reconnect => format!("Switch to {}", pending.tunnel_name.as_deref().unwrap_or("?")),
            crate::app::PendingAction::KillSwitchOn => "Enable kill switch".to_string(),
            crate::app::PendingAction::KillSwitchOff => "Disable kill switch".to_string(),
        };
        
        let countdown_color = match app.countdown_seconds {
            4 => accent(),
            3 => accent(),
            2 => warning(),
            1 => danger(),
            _ => danger(),
        };
        
        Line::from(vec![
            Span::styled("󰔟 ", Style::default().fg(countdown_color)),
            Span::styled(format!("{}", app.countdown_seconds), Style::default().fg(countdown_color).add_modifier(Modifier::BOLD)),
            Span::styled(" │ ", Style::default().fg(text_dim())),
            Span::styled(action_text, Style::default().fg(text())),
            Span::styled(" │ ", Style::default().fg(text_dim())),
            Span::styled("(Esc cancels)", Style::default().fg(text_dim())),
        ])
    } else if let Some(ref status) = app.status_message {
        // Show status/action feedback (e.g., "Connected to wg0", "Config saved")
        Line::from(vec![
            Span::styled(status, Style::default().fg(warning())),
        ])
    } else if let Some(ref info) = app.info_message {
        // Show VPN status/traffic info
        Line::from(vec![
            Span::styled(info, Style::default().fg(text_dim())),
        ])
    } else {
        Line::from(vec![
            Span::styled("Ready", Style::default().fg(text_dim())),
        ])
    };

    let info = Paragraph::new(line).alignment(Alignment::Center);
    f.render_widget(info, area);
}

fn draw_networks_box(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.section == Section::Networks;
    let border_color = if is_active { accent() } else { inactive() };
    let title_style = if is_active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(inactive())
    };

    let block = Block::default()
        .title(Span::styled(" Networks ", title_style))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    // Responsive columns based on width
    let show_type = area.width > 70;
    
    let header = if show_type {
        Row::new(vec![
            Span::styled("", Style::default().fg(header())),
            Span::styled("Name", Style::default().fg(header())),
            Span::styled("Type", Style::default().fg(header())),
            Span::styled("(r)ule", Style::default().fg(header())),
            Span::styled("(t)unnel", Style::default().fg(header())),
        ])
    } else {
        Row::new(vec![
            Span::styled("", Style::default().fg(header())),
            Span::styled("Name", Style::default().fg(header())),
            Span::styled("(r)ule", Style::default().fg(header())),
            Span::styled("(t)unnel", Style::default().fg(header())),
        ])
    };

    let rows: Vec<Row> = if app.networks.is_empty() {
        vec![Row::new(vec![
            Span::styled("  No networks detected", Style::default().fg(text_dim())),
        ])]
    } else {
        app.networks
            .iter()
            .enumerate()
            .map(|(i, network)| {
                let icon = match network.network_type.as_str() {
                    "wifi" => "󰖩",
                    "ethernet" => "󰈀",
                    _ => "󰛳",
                };
                let icon_color = if network.connected { success() } else { text_dim() };
                
                let rule = app.get_network_rule(network);
                let (rule_text, rule_color) = match rule {
                    Some(r) if r.always_vpn => ("Always", success()),
                    Some(r) if r.never_vpn => ("Never", danger()),
                    Some(r) if r.session_vpn => ("Session", accent_bright()),
                    _ => ("-", text_dim()),
                };

                // Get tunnel name from the rule
                let tunnel_name = rule
                    .and_then(|r| r.tunnel_name.as_ref())
                    .map(|t| t.as_str())
                    .unwrap_or("-");
                let tunnel_color = if tunnel_name != "-" { accent_bright() } else { text_dim() };

                let connected_indicator = if network.connected { " ●" } else { "" };

                let row_style = if i == app.selected_network && is_active {
                    Style::default()
                        .bg(bg_selected())
                        .fg(text()) // Ensure text is visible against selection bg
                        // .add_modifier(Modifier::REVERSED) // Removed reversed modifier to match netpala style more closely
                } else {
                    Style::default()
                };

                if show_type {
                    Row::new(vec![
                        Span::styled(icon, Style::default().fg(icon_color)),
                        Span::styled(format!("{}{}", network.name, connected_indicator), Style::default().fg(text())),
                        Span::styled(&network.network_type, Style::default().fg(text_dim())),
                        Span::styled(rule_text, Style::default().fg(rule_color)),
                        Span::styled(tunnel_name, Style::default().fg(tunnel_color)),
                    ])
                    .style(row_style)
                } else {
                    Row::new(vec![
                        Span::styled(icon, Style::default().fg(icon_color)),
                        Span::styled(format!("{}{}", network.name, connected_indicator), Style::default().fg(text())),
                        Span::styled(rule_text, Style::default().fg(rule_color)),
                        Span::styled(tunnel_name, Style::default().fg(tunnel_color)),
                    ])
                    .style(row_style)
                }
            })
            .collect()
    };

    let widths = if show_type {
        vec![
            Constraint::Length(3),
            Constraint::Percentage(35),
            Constraint::Percentage(12),
            Constraint::Percentage(15),
            Constraint::Percentage(33),
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Percentage(40),
            Constraint::Percentage(18),
            Constraint::Percentage(37),
        ]
    };

    let table = Table::new(rows, widths)
        .header(header.style(Style::default()))
        .block(block);

    f.render_widget(table, area);
}

fn draw_tunnels_box(f: &mut Frame, app: &App, area: Rect) {
    // Always show config panel alongside tunnels list
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),  // Tunnels list
            Constraint::Percentage(65),  // Config viewer
        ])
        .split(area);

    draw_tunnels_list(f, app, chunks[0]);
    draw_config_viewer(f, app, chunks[1]);
}

fn draw_tunnels_list(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.section == Section::Tunnels;
    let border_color = if is_active { accent() } else { inactive() };
    let title_style = if is_active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(inactive())
    };

    let block = Block::default()
        .title(Span::styled(" Tunnels ", title_style))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let header = Row::new(vec![
        Span::styled("", Style::default().fg(header())),
        Span::styled("Name", Style::default().fg(header())),
        Span::styled("Status", Style::default().fg(header())),
    ]);

    let rows: Vec<Row> = if app.tunnels.is_empty() {
        vec![
            Row::new(vec![
                Span::styled("  No tunnels configured", Style::default().fg(text_dim())),
            ]),
            Row::new(vec![
                Span::styled("  Press 'f' to import", Style::default().fg(accent())),
            ]),
        ]
    } else {
        app.tunnels
            .iter()
            .enumerate()
            .map(|(i, tunnel)| {
                let is_connected = tunnel.connected || (app.vpn_status.connected
                    && app.vpn_status.interface.as_deref() == Some(&tunnel.name));

                // Determine status based on connection AND routing health
                let (icon, icon_color, status, status_color) = if is_connected {
                    if !app.vpn_status.routing_ok {
                        // Interface up but routing broken
                        ("󰒙", warning(), "UP ⚠", warning())
                    } else if app.vpn_status.handshake_stale {
                        // Routing OK but handshake stale
                        ("󰒘", warning(), "UP ?", warning())
                    } else {
                        // All good
                        ("󰒘", success(), "UP ✓", success())
                    }
                } else {
                    ("󰒙", text_dim(), "DOWN", text_dim())
                };

                let row_style = if i == app.selected_tunnel && app.section == Section::Tunnels {
                    Style::default()
                        .bg(bg_selected())
                        .fg(text())
                        // .add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };

                Row::new(vec![
                    Span::styled(icon, Style::default().fg(icon_color)),
                    Span::styled(&tunnel.name, Style::default().fg(text())),
                    Span::styled(status, Style::default().fg(status_color)),
                ])
                .style(row_style)
            })
            .collect()
    };

    let widths = vec![
        Constraint::Length(3),
        Constraint::Percentage(60),
        Constraint::Percentage(35),
    ];

    let table = Table::new(rows, widths)
        .header(header.style(Style::default()))
        .block(block);

    f.render_widget(table, area);
}

fn draw_config_viewer(f: &mut Frame, app: &App, area: Rect) {
    // Config viewer is always visible but not separately active
    let border_color = inactive();
    let title_style = Style::default().fg(inactive());

    let block = Block::default()
        .title(Span::styled(" Config ", title_style))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if app.tunnels.is_empty() {
        let help = Paragraph::new("No tunnel selected")
            .style(Style::default().fg(text_dim()))
            .block(block);
        f.render_widget(help, area);
        return;
    }

    // Get the visible lines based on scroll offset
    let inner_height = area.height.saturating_sub(2) as usize; // Account for borders
    let lines: Vec<&str> = app.tunnel_config_content.lines().collect();
    let start = app.tunnel_config_scroll;
    let end = (start + inner_height).min(lines.len());
    
    let visible_lines: Vec<Line> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_num = start + i + 1;
            let style = if line.starts_with('[') {
                Style::default().fg(accent_bright())
            } else if line.starts_with('#') {
                Style::default().fg(text_dim())
            } else if line.contains('=') {
                Style::default().fg(text())
            } else {
                Style::default().fg(text_dim())
            };
            
            Line::from(vec![
                Span::styled(format!("{:3} ", line_num), Style::default().fg(inactive())),
                Span::styled(*line, style),
            ])
        })
        .collect();

    let content = Paragraph::new(visible_lines)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(content, area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let hints: Vec<(&str, &str)> = match app.section {
        Section::Networks => vec![
            ("↑↓", "Nav"),
            ("r", "Rule"),
            ("t", "Tunnel"),
            ("d", "Del"),
            ("Tab", "Next"),
            ("h", "Help"),
        ],
        Section::Tunnels => vec![
            ("↑↓", "Nav"),
            ("Space", "Connect"),
            ("e", "Edit"),
            ("n", "New"),
            ("i", "Import"),
            ("d", "Del"),
        ],
        Section::KillSwitch => vec![
            ("Space", "Toggle"),
            ("k", "Toggle"),
            ("Tab", "Next"),
            ("h", "Help"),
        ],
    };

    // Responsive: show fewer hints on narrow terminals
    let max_hints = if area.width < 60 { 4 } else if area.width < 80 { 5 } else { hints.len() };

    let hint_spans: Vec<Span> = hints
        .iter()
        .take(max_hints)
        .flat_map(|(key, action)| {
            vec![
                Span::styled(*key, Style::default().fg(accent())),
                Span::styled(format!(" {} │ ", action), Style::default().fg(text_dim())),
            ]
        })
        .collect();

    // Footer is commands legend ONLY - no status messages here
    let footer = Paragraph::new(Line::from(hint_spans))
        .alignment(Alignment::Center);

    f.render_widget(footer, area);
}

fn draw_file_browser(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(
        if area.width < 80 { 90 } else { 70 },
        if area.height < 30 { 85 } else { 70 },
        area
    );

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(Span::styled(" 󰈔 Select WireGuard Config ", Style::default().fg(accent())))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent()));

    f.render_widget(block, popup_area);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(popup_area);

    let path_str = app.browser_path.to_string_lossy();
    let path_display = Paragraph::new(Line::from(vec![
        Span::styled("󰉋 ", Style::default().fg(accent())),
        Span::styled(path_str.as_ref(), Style::default().fg(text())),
    ]))
    .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(inactive())));
    f.render_widget(path_display, inner[0]);

    let rows: Vec<Row> = if app.browser_entries.is_empty() {
        vec![Row::new(vec![
            Span::styled("  No .conf files in this directory", Style::default().fg(text_dim())),
        ])]
    } else {
        app.browser_entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let icon = if entry.is_dir { "󰉋" } else { "󰈔" };
                let icon_color = if entry.is_dir { accent() } else { success() };
                
                let row_style = if i == app.browser_selected {
                    Style::default()
                        .bg(bg_selected())
                        .fg(text())
                        // .add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };

                Row::new(vec![
                    Span::styled(format!("  {} ", icon), Style::default().fg(icon_color)),
                    Span::styled(&entry.name, Style::default().fg(text())),
                ])
                .style(row_style)
            })
            .collect()
    };

    let widths = [Constraint::Length(5), Constraint::Percentage(90)];
    let table = Table::new(rows, widths);
    f.render_widget(table, inner[1]);

    let hint = Paragraph::new(Line::from(vec![
        Span::styled("j/k", Style::default().fg(accent())),
        Span::raw(" nav │ "),
        Span::styled("Enter", Style::default().fg(accent())),
        Span::raw(" select │ "),
        Span::styled("Backspace", Style::default().fg(accent())),
        Span::raw(" up │ "),
        Span::styled("Esc", Style::default().fg(accent())),
        Span::raw(" cancel"),
    ]))
    .alignment(Alignment::Center)
    .style(Style::default().fg(text_dim()));
    f.render_widget(hint, inner[2]);
}

fn draw_config_preview(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(
        if area.width < 100 { 95 } else { 80 },
        if area.height < 35 { 90 } else { 80 },
        area
    );

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(Span::styled(" 󰈔 Preview & Save Tunnel ", Style::default().fg(accent())))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent()));

    f.render_widget(block, popup_area);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(popup_area);

    // Name input
    let name_display = if app.input_buffer.is_empty() {
        &app.preview_name
    } else {
        &app.input_buffer
    };

    let name_border = if app.preview_field == 0 { accent() } else { inactive() };
    let name_input = Paragraph::new(format!("{}_", name_display))
        .style(Style::default().fg(text()))
        .block(
            Block::default()
                .title(Span::styled(" Tunnel Name ", Style::default().fg(if app.preview_field == 0 { accent() } else { header() })))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(name_border)),
        );
    f.render_widget(name_input, inner[0]);

    // Config preview
    let preview_lines: Vec<Line> = app.config_preview
        .lines()
        .take(inner[1].height.saturating_sub(2) as usize)
        .map(|line| {
            if line.starts_with('[') {
                Line::styled(line, Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            } else if line.contains('=') {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Line::from(vec![
                        Span::styled(parts[0], Style::default().fg(header())),
                        Span::styled("=", Style::default().fg(text_dim())),
                        Span::styled(parts[1], Style::default().fg(text())),
                    ])
                } else {
                    Line::styled(line, Style::default().fg(text()))
                }
            } else {
                Line::styled(line, Style::default().fg(text_dim()))
            }
        })
        .collect();

    let config_view = Paragraph::new(preview_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(Span::styled(" Config → /etc/wireguard/ ", Style::default().fg(header())))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(inactive())),
        );
    f.render_widget(config_view, inner[1]);

    // Action buttons
    let button_style = if app.preview_field == 1 { 
        Style::default().bg(bg_selected()) 
    } else { 
        Style::default() 
    };

    let buttons = Paragraph::new(Line::from(vec![
        Span::styled("  [ ", Style::default().fg(text_dim())),
        Span::styled("Enter = Save", Style::default().fg(success()).add_modifier(Modifier::BOLD)),
        Span::styled(" ]  [ ", Style::default().fg(text_dim())),
        Span::styled("Esc = Cancel", Style::default().fg(danger())),
        Span::styled(" ]  ", Style::default().fg(text_dim())),
    ]))
    .style(button_style)
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if app.preview_field == 1 { accent() } else { inactive() })),
    );
    f.render_widget(buttons, inner[2]);
}

fn draw_manual_config(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(
        if area.width < 100 { 95 } else { 80 },
        if area.height < 35 { 90 } else { 80 },
        area
    );

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(Span::styled(" 󰈔 Create WireGuard Tunnel ", Style::default().fg(accent())))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent()));

    f.render_widget(block, popup_area);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(popup_area);

    // Name input
    let name_border = if app.preview_field == 0 { accent() } else { inactive() };
    let name_cursor = if app.preview_field == 0 { "_" } else { "" };
    let name_input = Paragraph::new(format!("{}{}", app.input_buffer, name_cursor))
        .style(Style::default().fg(text()))
        .block(
            Block::default()
                .title(Span::styled(" Tunnel Name ", Style::default().fg(if app.preview_field == 0 { accent() } else { header() })))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(name_border)),
        );
    f.render_widget(name_input, inner[0]);

    // Config content area
    let content_border = if app.preview_field == 1 { accent() } else { inactive() };
    let content_cursor = if app.preview_field == 1 { "█" } else { "" };
    
    let content_lines: Vec<Line> = app.config_preview
        .lines()
        .map(|line| {
            if line.starts_with('[') {
                Line::styled(line, Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            } else if line.contains('=') {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Line::from(vec![
                        Span::styled(parts[0], Style::default().fg(header())),
                        Span::styled("=", Style::default().fg(text_dim())),
                        Span::styled(parts[1], Style::default().fg(text())),
                    ])
                } else {
                    Line::styled(line, Style::default().fg(text()))
                }
            } else {
                Line::styled(line, Style::default().fg(text()))
            }
        })
        .collect();

    // Add cursor to last line if in content field
    let mut display_lines = content_lines;
    if app.preview_field == 1 {
        if display_lines.is_empty() {
            display_lines.push(Line::styled(content_cursor, Style::default().fg(accent())));
        } else {
            // Just show the cursor indicator at the end
            display_lines.push(Line::styled(content_cursor, Style::default().fg(accent())));
        }
    }

    let config_edit = Paragraph::new(display_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(Span::styled(" Paste/Type Config (Tab to switch fields) ", Style::default().fg(if app.preview_field == 1 { accent() } else { header() })))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(content_border)),
        );
    f.render_widget(config_edit, inner[1]);

    // Action buttons / instructions
    let buttons = Paragraph::new(Line::from(vec![
        Span::styled("  [ ", Style::default().fg(text_dim())),
        Span::styled("F2 = Save", Style::default().fg(success()).add_modifier(Modifier::BOLD)),
        Span::styled(" ]  [ ", Style::default().fg(text_dim())),
        Span::styled("Tab = Switch Field", Style::default().fg(accent())),
        Span::styled(" ]  [ ", Style::default().fg(text_dim())),
        Span::styled("Esc = Cancel", Style::default().fg(danger())),
        Span::styled(" ]  ", Style::default().fg(text_dim())),
    ]))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(inactive())),
    );
    f.render_widget(buttons, inner[2]);
}

fn draw_help_popup(f: &mut Frame) {
    let area = f.area();
    let popup_area = centered_rect(
        if area.width < 80 { 95 } else { 70 },
        if area.height < 40 { 95 } else { 85 },
        area
    );

    f.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(Span::styled("═══ Navigation ═══", Style::default().fg(header()).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("  Tab       ", Style::default().fg(accent())),
            Span::raw("Switch sections (Networks → Tunnels → Kill Switch)"),
        ]),
        Line::from(vec![
            Span::styled("  ↑/↓ j/k   ", Style::default().fg(accent())),
            Span::raw("Move up/down in lists"),
        ]),
        Line::from(""),
        Line::from(Span::styled("═══ Tunnel Actions ═══", Style::default().fg(header()).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("  Space     ", Style::default().fg(accent())),
            Span::raw("Connect/Disconnect selected tunnel"),
        ]),
        Line::from(vec![
            Span::styled("  f         ", Style::default().fg(accent())),
            Span::raw("Import .conf file from file browser"),
        ]),
        Line::from(vec![
            Span::styled("  c         ", Style::default().fg(accent())),
            Span::raw("View/edit tunnel config"),
        ]),
        Line::from(vec![
            Span::styled("  d         ", Style::default().fg(accent())),
            Span::raw("Delete selected tunnel"),
        ]),
        Line::from(""),
        Line::from(Span::styled("═══ Network Rules ═══", Style::default().fg(header()).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("  r         ", Style::default().fg(accent())),
            Span::raw("Cycle rule: Always → Never → Session → None"),
        ]),
        Line::from(vec![
            Span::styled("  t         ", Style::default().fg(accent())),
            Span::raw("Cycle tunnel assignment for network"),
        ]),
        Line::from(""),
        Line::from(Span::styled("═══ Kill Switch ═══", Style::default().fg(header()).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("  k/Space   ", Style::default().fg(accent())),
            Span::raw("Toggle kill switch (when box is active)"),
        ]),
        Line::from(vec![
            Span::raw("            Blocks all traffic except through VPN"),
        ]),
        Line::from(""),
        Line::from(Span::styled("═══ Quick Start ═══", Style::default().fg(header()).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("  tonneru              ", Style::default().fg(accent())),
            Span::raw("Launch this TUI"),
        ]),
        Line::from(vec![
            Span::styled("  tonneru --daemon     ", Style::default().fg(accent())),
            Span::raw("Run as background daemon"),
        ]),
        Line::from(vec![
            Span::styled("  tonneru --status     ", Style::default().fg(accent())),
            Span::raw("Get JSON status for scripts"),
        ]),
        Line::from(""),
        Line::from(Span::styled("═══ Service Management ═══", Style::default().fg(header()).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("  systemctl --user status tonneru   ", Style::default().fg(text_dim())),
        ]),
        Line::from(vec![
            Span::styled("  systemctl --user restart tonneru  ", Style::default().fg(text_dim())),
        ]),
        Line::from(vec![
            Span::styled("  journalctl --user -u tonneru -f   ", Style::default().fg(text_dim())),
        ]),
        Line::from(""),
        Line::from(Span::styled("═══ Security ═══", Style::default().fg(header()).add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::raw("  • Uses dedicated 'tonneru' group (not wheel)"),
        ]),
        Line::from(vec![
            Span::raw("  • Single auditable helper script for privileged ops"),
        ]),
        Line::from(vec![
            Span::styled("  • Logs: ", Style::default()),
            Span::styled("journalctl -t tonneru-sudo", Style::default().fg(text_dim())),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(text_dim())),
            Span::styled("h", Style::default().fg(accent())),
            Span::styled("/", Style::default().fg(text_dim())),
            Span::styled("?", Style::default().fg(accent())),
            Span::styled("/", Style::default().fg(text_dim())),
            Span::styled("Esc", Style::default().fg(accent())),
            Span::styled(" to close", Style::default().fg(text_dim())),
        ]),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(Span::styled(" 󰋖 tonneru Help ", Style::default().fg(accent())))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent())),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(help, popup_area);
}

fn draw_confirm_popup(f: &mut Frame, app: &App) {
    let popup_area = centered_rect(40, 20, f.area());

    f.render_widget(Clear, popup_area);

    let message = app.status_message.as_deref().unwrap_or("Confirm?");

    let confirm = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(message, Style::default().fg(warning()))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  y", Style::default().fg(success()).add_modifier(Modifier::BOLD)),
            Span::raw(" Yes   "),
            Span::styled("n", Style::default().fg(danger()).add_modifier(Modifier::BOLD)),
            Span::raw(" No"),
        ]),
    ])
    .block(
        Block::default()
            .title(Span::styled(" Confirm ", Style::default().fg(warning())))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(warning())),
    )
    .alignment(Alignment::Center);

    f.render_widget(confirm, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

