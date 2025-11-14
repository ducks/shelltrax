use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
  // General UI colors
  pub border: String,
  pub border_focused: String,
  pub text: String,
  pub text_selected: String,
  pub background_selected: String,

  // Progress bar / footer
  pub progress_bar: String,
  pub footer_text: String,

  // Library view
  pub artist_text: String,
  pub album_text: String,
  pub track_text: String,
}

impl Default for Theme {
  fn default() -> Self {
    Self {
      border: "white".to_string(),
      border_focused: "green".to_string(),
      text: "white".to_string(),
      text_selected: "black".to_string(),
      background_selected: "green".to_string(),
      progress_bar: "green".to_string(),
      footer_text: "white".to_string(),
      artist_text: "cyan".to_string(),
      album_text: "yellow".to_string(),
      track_text: "white".to_string(),
    }
  }
}

impl Theme {
  pub fn load() -> Self {
    let config_path = Self::config_path();

    if let Ok(contents) = fs::read_to_string(&config_path) {
      match toml::from_str(&contents) {
        Ok(theme) => {
          log::info!("Loaded theme from {:?}", config_path);
          return theme;
        }
        Err(e) => {
          log::warn!("Failed to parse theme config: {}. Using default theme.", e);
        }
      }
    }

    log::info!("Using default theme");
    Self::default()
  }

  fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
      .join(".config")
      .join("shelltrax")
      .join("theme.toml")
  }

  #[allow(dead_code)]
  pub fn save_example(&self) -> std::io::Result<()> {
    let config_path = Self::config_path();
    if let Some(parent) = config_path.parent() {
      fs::create_dir_all(parent)?;
    }

    let toml_str = toml::to_string_pretty(self).map_err(std::io::Error::other)?;

    fs::write(&config_path, toml_str)?;
    log::info!("Saved example theme to {:?}", config_path);
    Ok(())
  }

  pub fn parse_color(&self, color_str: &str) -> Color {
    match color_str.to_lowercase().as_str() {
      "black" => Color::Black,
      "red" => Color::Red,
      "green" => Color::Green,
      "yellow" => Color::Yellow,
      "blue" => Color::Blue,
      "magenta" => Color::Magenta,
      "cyan" => Color::Cyan,
      "white" => Color::White,
      "gray" | "grey" => Color::Gray,
      "darkgray" | "darkgrey" => Color::DarkGray,
      "lightred" => Color::LightRed,
      "lightgreen" => Color::LightGreen,
      "lightyellow" => Color::LightYellow,
      "lightblue" => Color::LightBlue,
      "lightmagenta" => Color::LightMagenta,
      "lightcyan" => Color::LightCyan,
      _ => {
        // Try to parse as RGB hex (#RRGGBB)
        if color_str.starts_with('#') && color_str.len() == 7
          && let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&color_str[1..3], 16),
            u8::from_str_radix(&color_str[3..5], 16),
            u8::from_str_radix(&color_str[5..7], 16),
          ) {
            return Color::Rgb(r, g, b);
          }
        Color::White // fallback
      }
    }
  }
}
