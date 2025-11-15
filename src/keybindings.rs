use crossterm::event::{KeyCode, KeyModifiers};
use std::collections::HashMap;
use crate::actions::Action;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyBinding {
  pub code: KeyCode,
  pub modifiers: KeyModifiers,
}

impl KeyBinding {
  pub fn new(code: KeyCode) -> Self {
    Self {
      code,
      modifiers: KeyModifiers::empty(),
    }
  }
}

pub struct KeyMap {
  bindings: HashMap<KeyBinding, Action>,
}

impl KeyMap {
  pub fn new() -> Self {
    Self {
      bindings: HashMap::new(),
    }
  }

  pub fn with_defaults() -> Self {
    let mut map = Self::new();
    map.load_defaults();
    map
  }

  pub fn bind(&mut self, key: KeyBinding, action: Action) {
    self.bindings.insert(key, action);
  }

  pub fn get_action(&self, key: &KeyBinding) -> Option<&Action> {
    self.bindings.get(key)
  }

  pub fn load_defaults(&mut self) {
    // Global
    self.bind(KeyBinding::new(KeyCode::Char('q')), Action::Quit);

    // Navigation - Arrow keys
    self.bind(KeyBinding::new(KeyCode::Up), Action::MoveUp);
    self.bind(KeyBinding::new(KeyCode::Down), Action::MoveDown);

    // Navigation - Vim keys
    self.bind(KeyBinding::new(KeyCode::Char('k')), Action::MoveUp);
    self.bind(KeyBinding::new(KeyCode::Char('j')), Action::MoveDown);
    self.bind(KeyBinding::new(KeyCode::Char('h')), Action::MoveLeft);
    self.bind(KeyBinding::new(KeyCode::Char('l')), Action::MoveRight);
    self.bind(KeyBinding::new(KeyCode::Char('g')), Action::GoToTop);
    self.bind(KeyBinding::new(KeyCode::Char('G')), Action::GoToBottom);

    // Screens - cmus style
    self.bind(KeyBinding::new(KeyCode::Char('1')), Action::GoToLibrary);
    self.bind(KeyBinding::new(KeyCode::Char('5')), Action::GoToBrowser);

    // Playback - cmus style
    self.bind(KeyBinding::new(KeyCode::Enter), Action::PlaySelected);
    self.bind(KeyBinding::new(KeyCode::Char('c')), Action::TogglePause);
    self.bind(KeyBinding::new(KeyCode::Char('b')), Action::NextTrack);
    self.bind(KeyBinding::new(KeyCode::Char('z')), Action::PreviousTrack);
    self.bind(KeyBinding::new(KeyCode::Char('p')), Action::ToggleAutoplay);
    self.bind(KeyBinding::new(KeyCode::Char('r')), Action::CycleRepeat);

    // Library
    self.bind(KeyBinding::new(KeyCode::Char(' ')), Action::ToggleExpanded);
    self.bind(KeyBinding::new(KeyCode::Tab), Action::SwitchFocus);
    self.bind(KeyBinding::new(KeyCode::Char('a')), Action::AddToLibrary);

    // Browser
    self.bind(KeyBinding::new(KeyCode::Backspace), Action::GoUpDirectory);

    // Search
    self.bind(KeyBinding::new(KeyCode::Char('/')), Action::EnterSearch);
    self.bind(KeyBinding::new(KeyCode::Esc), Action::ExitSearch);
  }

  // Future: load from config file
  #[allow(dead_code)]
  pub fn load_from_config(&mut self, _path: &str) -> std::io::Result<()> {
    // TODO: implement config loading
    // Parse toml/json and populate bindings
    Ok(())
  }
}

impl Default for KeyMap {
  fn default() -> Self {
    Self::with_defaults()
  }
}
