//! Theme colors loaded from Omarchy/Hyprland system theme
//! Reads colors from ~/.config/omarchy/current/theme/kitty.conf

use ratatui::style::Color;
use std::collections::HashMap;
use std::fs;

/// Theme colors for the UI
#[derive(Debug, Clone)]
pub struct Theme {
    pub accent: Color,           // Active borders, highlights (color2/green - often amber in Omarchy)
    pub accent_bright: Color,    // Brighter accent (color10)
    pub danger: Color,           // Errors, warnings (color1/red)
    #[allow(dead_code)]
    pub danger_bright: Color,    // Bright red (color9) - reserved for future use
    pub success: Color,          // Success indicators (using accent in matte-black)
    pub warning: Color,          // Warnings (color4/blue - often orange in Omarchy)
    pub text: Color,             // Primary text (foreground)
    pub text_dim: Color,         // Dimmed text (color8/bright black)
    #[allow(dead_code)]
    pub bg: Color,               // Background - reserved for future use
    pub bg_selected: Color,      // Selection background
    pub inactive: Color,         // Inactive borders
    pub header: Color,           // Header text (using danger for contrast)
}

impl Default for Theme {
    fn default() -> Self {
        // Fallback to Catppuccin-inspired colors if theme can't be loaded
        Self {
            accent: Color::Rgb(250, 179, 135),
            accent_bright: Color::Rgb(245, 194, 231),
            danger: Color::Rgb(243, 139, 168),
            danger_bright: Color::Rgb(243, 139, 168),
            success: Color::Rgb(166, 218, 149),
            warning: Color::Rgb(250, 179, 135),
            text: Color::Rgb(205, 214, 244),
            text_dim: Color::Rgb(147, 153, 178),
            bg: Color::Rgb(30, 30, 46),
            bg_selected: Color::Rgb(69, 71, 90),
            inactive: Color::Rgb(88, 91, 112),
            header: Color::Rgb(243, 139, 168),
        }
    }
}

impl Theme {
    /// Load theme from Omarchy system theme
    pub fn load() -> Self {
        // Try to load from Omarchy theme
        if let Some(theme) = Self::load_omarchy_theme() {
            return theme;
        }

        // Fallback to defaults
        Self::default()
    }

    /// Load colors from Omarchy kitty.conf theme file
    fn load_omarchy_theme() -> Option<Self> {
        let home = dirs::home_dir()?;
        let theme_path = home
            .join(".config/omarchy/current/theme/kitty.conf");

        let content = fs::read_to_string(&theme_path).ok()?;
        let colors = Self::parse_kitty_conf(&content);

        if colors.is_empty() {
            return None;
        }

        // Map kitty colors to our theme
        // Omarchy Matte Black uses unconventional color mappings:
        // - color2 (green) = accent/gold (#FFC107)
        // - color4 (blue) = warning/orange (#e68e0d)
        // - color1 (red) = danger (#D35F5F)
        
        let accent = colors.get("color2").or(colors.get("color10"))
            .copied().unwrap_or(Color::Rgb(255, 193, 7));  // #FFC107
        
        let accent_bright = colors.get("color10").or(colors.get("color2"))
            .copied().unwrap_or(Color::Rgb(255, 193, 7));
        
        let danger = colors.get("color1")
            .copied().unwrap_or(Color::Rgb(211, 95, 95));  // #D35F5F
        
        let danger_bright = colors.get("color9")
            .copied().unwrap_or(Color::Rgb(185, 28, 28));  // #B91C1C
        
        let warning = colors.get("color4").or(colors.get("color12"))
            .copied().unwrap_or(Color::Rgb(230, 142, 13));  // #e68e0d
        
        let text = colors.get("foreground")
            .copied().unwrap_or(Color::Rgb(190, 190, 190));  // #bebebe
        
        let text_dim = colors.get("color8")
            .copied().unwrap_or(Color::Rgb(138, 138, 141));  // #8a8a8d
        
        let bg = colors.get("background")
            .copied().unwrap_or(Color::Rgb(18, 18, 18));  // #121212
        
        let bg_selected = colors.get("selection_background").or(colors.get("color0"))
            .copied().unwrap_or(Color::Rgb(51, 51, 51));  // #333333
        
        let inactive = colors.get("inactive_border_color").or(colors.get("color8"))
            .copied().unwrap_or(Color::Rgb(89, 89, 89));  // #595959

        Some(Self {
            accent,
            accent_bright,
            danger,
            danger_bright,
            success: accent,  // Use accent as success color in matte-black
            warning,
            text,
            text_dim,
            bg,
            bg_selected,
            inactive,
            header: danger,  // Use red/danger for headers (contrast)
        })
    }

    /// Parse kitty.conf format: `key value` or `key #hexcolor`
    fn parse_kitty_conf(content: &str) -> HashMap<String, Color> {
        let mut colors = HashMap::new();

        for line in content.lines() {
            let line = line.trim();
            
            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse "key value" format
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() == 2 {
                let key = parts[0].trim();
                let value = parts[1].trim();
                
                // Parse hex color
                if let Some(color) = Self::parse_hex_color(value) {
                    colors.insert(key.to_string(), color);
                }
            }
        }

        colors
    }

    /// Parse a hex color string (#RRGGBB or #RGB)
    fn parse_hex_color(s: &str) -> Option<Color> {
        let s = s.trim().trim_start_matches('#');
        
        if s.len() == 6 {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        } else if s.len() == 3 {
            let r = u8::from_str_radix(&s[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&s[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&s[2..3], 16).ok()? * 17;
            Some(Color::Rgb(r, g, b))
        } else {
            None
        }
    }
}


