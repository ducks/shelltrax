mod actions;
mod app;
mod browser;
mod keybindings;
mod library;
mod list;
mod persistence;
mod player;
mod screens;
mod theme;
mod ui;

use app::App;
use actions::Action;
use keybindings::{KeyMap, KeyBinding};


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

fn main() -> Result<()> {
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

            let next_track = {
                let mut plyr = app.player.lock().unwrap();
                let mut lib = app.library.lock().unwrap();

                let mut result = None;

                if let Some(current_path) = plyr.current_path.clone() {
                    match app.repeat_mode {
                        RepeatMode::Track => {
                            // Replay same track
                            plyr.play(&current_path);
                            if let Some(track) = lib.track_by_path(&current_path) {
                                result = Some(track.clone());
                            }
                        }
                        RepeatMode::All if app.autoplay_enabled => {
                            // Play next track
                            if let Some(next_path) = lib.next_track_path(&current_path) {
                                lib.select_track_by_path(&next_path);
                                plyr.play(&next_path);

                                if let Some(next_track) = lib.track_by_path(&next_path) {
                                    result = Some(next_track.clone());
                                }
                            }
                        }
                        RepeatMode::Off => {
                            // Stop playback
                            log::debug!("Repeat off - stopping playback");
                        }
                        _ => {}
                    }
                }

                result
            };

            if let Some(track) = next_track {
                app.begin_playback(&track);

                log::debug!(
                    "Repeat mode: {:?}, playing: {} - {}",
                    app.repeat_mode,
                    track.album_artist,
                    track.title
                );
                log::debug!("playback_start: {:?}", app.playback_start);
            }
        }

        log::debug!("Drawing track: {:?}", app.current_track.as_ref().map(|t| &t.title));
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
