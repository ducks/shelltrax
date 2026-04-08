mod actions;
mod app;
mod browser;
mod keybindings;
mod library;
mod list;
mod persistence;
mod player;
mod screens;
mod scrobbler;
mod theme;
mod ui;

use actions::Action;
use app::App;
use keybindings::{KeyBinding, KeyMap};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{backend::CrosstermBackend, prelude::*};
use std::io::{Result, stdout};

use simplelog::*;
use std::fs::File;
use std::sync::atomic::Ordering;

#[tokio::main]
async fn main() -> Result<()> {
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Trace,
        Config::default(),
        File::create("debug.log").unwrap(),
    )])
    .unwrap();

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let keymap = KeyMap::with_defaults();

    loop {
        app.update();

        if app
            .player
            .lock()
            .unwrap()
            .autoplay_trigger
            .swap(false, Ordering::SeqCst)
        {
            use app::RepeatMode;

            // Handle autoplay based on repeat mode
            match app.repeat_mode {
                RepeatMode::Track => {
                    // Replay same track
                    let current_path = {
                        let plyr = app.player.lock().unwrap();
                        plyr.current_path.clone()
                    };

                    if let Some(path) = current_path {
                        {
                            let mut plyr = app.player.lock().unwrap();
                            if let Err(e) = plyr.play(&path) {
                                log::error!("Playback failed: {e}");
                                continue;
                            }
                        }

                        let track = {
                            let lib = app.library.lock().unwrap();
                            lib.track_by_path(&path).cloned()
                        };

                        if let Some(track) = track {
                            app.begin_playback(&track);
                            log::debug!("Repeat Track: replaying {}", track.title);
                        }
                    }
                }
                RepeatMode::All if app.autoplay_enabled => {
                    // Play next track from queue
                    app.play_next_track();
                }
                RepeatMode::Off => {
                    // Stop playback
                    log::debug!("Repeat off - stopping playback");
                }
                _ => {}
            }
        }

        log::debug!(
            "Drawing track: {:?}",
            app.current_track.as_ref().map(|t| &t.title)
        );
        terminal.draw(|f| ui::draw_ui(f, &mut app))?;

        if event::poll(std::time::Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
        {
            // Handle search mode input
            if app.search_active {
                use crossterm::event::KeyCode;
                match key.code {
                    KeyCode::Char(c) => {
                        app.search_query.push(c);
                        // Jump to match as user types
                        match app.screen {
                            app::AppScreen::Library => {
                                app.library_mut().jump_to_match(&app.search_query);
                            }
                            app::AppScreen::Browser => {
                                app.browser.jump_to_match(&app.search_query);
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        app.search_query.pop();
                        // Re-search with updated query
                        match app.screen {
                            app::AppScreen::Library => {
                                app.library_mut().jump_to_match(&app.search_query);
                            }
                            app::AppScreen::Browser => {
                                app.browser.jump_to_match(&app.search_query);
                            }
                        }
                    }
                    KeyCode::Esc | KeyCode::Enter => {
                        // Exit search mode, keeping current position
                        app.search_active = false;
                        app.search_query.clear();
                    }
                    _ => {}
                }
            } else {
                // Normal mode: handle keybindings
                let binding = KeyBinding::new(key.code);

                if let Some(action) = keymap.get_action(&binding) {
                    if matches!(action, Action::Quit) {
                        break;
                    }
                    action.execute(&mut app);
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}
