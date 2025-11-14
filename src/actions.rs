use crate::app::{App, AppScreen};
use crate::browser::BrowserItem;
use crate::library::{LibraryFocus, scan_path_for_tracks};
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
  // Navigation
  MoveUp,
  MoveDown,
  MoveLeft,
  MoveRight,
  GoToTop,
  GoToBottom,

  // Screens
  GoToLibrary,
  GoToBrowser,

  // Playback
  PlaySelected,
  TogglePause,
  NextTrack,
  PreviousTrack,
  ToggleAutoplay,
  CycleRepeat,

  // Library
  ToggleExpanded,
  SwitchFocus,
  AddToLibrary,

  // Browser
  GoUpDirectory,

  // Global
  Quit,
}

impl Action {
  pub fn execute(&self, app: &mut App) {
    match self {
      Action::Quit => {
        // Handled in main loop
      }
      Action::MoveUp => {
        match app.screen {
          AppScreen::Browser => app.browser.move_up(),
          AppScreen::Library => {
            let mut lib = app.library_mut();
            match lib.focus {
              crate::library::LibraryFocus::Left => lib.move_up(),
              crate::library::LibraryFocus::Right => lib.move_track_up(),
            }
          }
        }
      }
      Action::MoveDown => {
        match app.screen {
          AppScreen::Browser => app.browser.move_down(),
          AppScreen::Library => {
            let mut lib = app.library_mut();
            match lib.focus {
              crate::library::LibraryFocus::Left => lib.move_down(),
              crate::library::LibraryFocus::Right => {
                let count = lib.visible_tracks().len();
                lib.move_track_down(count);
              }
            }
          }
        }
      }
      Action::GoToLibrary => app.goto_screen(AppScreen::Library),
      Action::GoToBrowser => app.goto_screen(AppScreen::Browser),
      Action::TogglePause => app.toggle_pause(),
      Action::NextTrack => app.play_next_track(),
      Action::ToggleAutoplay => {
        app.autoplay_enabled = !app.autoplay_enabled;
      }
      Action::CycleRepeat => {
        app.repeat_mode = app.repeat_mode.next();
      }
      Action::ToggleExpanded => {
        if app.screen == AppScreen::Library {
          app.library_mut().toggle_expanded();
        }
      }
      Action::SwitchFocus => {
        app.library_mut().tab_focus();
      }
      Action::GoUpDirectory => {
        if app.screen == AppScreen::Browser {
          app.browser.go_up();
        }
      }
      Action::GoToTop => {
        match app.screen {
          AppScreen::Browser => app.browser.go_to_top(),
          AppScreen::Library => {
            let mut lib = app.library_mut();
            match lib.focus {
              LibraryFocus::Left => lib.go_to_top(),
              LibraryFocus::Right => lib.track_go_to_top(),
            }
          }
        }
      }
      Action::GoToBottom => {
        match app.screen {
          AppScreen::Browser => app.browser.go_to_bottom(),
          AppScreen::Library => {
            let mut lib = app.library_mut();
            match lib.focus {
              LibraryFocus::Left => lib.go_to_bottom(),
              LibraryFocus::Right => {
                let count = lib.visible_tracks().len();
                lib.track_go_to_bottom(count);
              }
            }
          }
        }
      }
      Action::AddToLibrary => {
        let mut lib = app.library_mut();

        if app.screen == AppScreen::Browser
          && let Some(BrowserItem::Entry(path)) = app.browser.list.selected_item() {
            let tracks = scan_path_for_tracks(path);
            lib.tracks = tracks.clone();
            lib.add_tracks(tracks);
          }
      }
      Action::PlaySelected => {
        if app.screen == AppScreen::Browser {
          app.browser.open_selected();
        }

        let lib = app.library_mut();

        if app.screen == AppScreen::Library && lib.focus == LibraryFocus::Right {
          let (selected, all_tracks, selected_index) = {
            let lib = lib;
            let tracks = lib.visible_tracks();
            let selected = tracks.get(lib.track_index).cloned();
            let all_paths: Vec<_> = tracks.iter().map(|t| t.path.clone()).collect();
            (selected, all_paths, lib.track_index)
          };

          if let Some(track) = selected {
            // Populate the play queue with all visible tracks
            app.play_queue = all_tracks;
            app.queue_index = selected_index;

            let player = Arc::clone(&app.player);
            let mut should_unpause = false;

            // Stop current playback and play selected track
            {
              let mut plyr = player.lock().unwrap();
              let was_paused = plyr.paused_flag.load(Ordering::SeqCst);

              plyr.stop();
              plyr.play(&track.path);

              if was_paused {
                should_unpause = true;
              }
            }

            app.begin_playback(&track);

            if should_unpause {
              app.toggle_pause();
            }
          }
        }
      }
      _ => {}
    }
  }
}
