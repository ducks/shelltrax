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
    cursor::Show,
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{backend::CrosstermBackend, prelude::*};
use std::io::{self, Result, stdout};

use simplelog::*;
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicBool, Ordering};

static TERMINAL_ACTIVE: AtomicBool = AtomicBool::new(false);

struct TerminalSession {
    active: bool,
}

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        TERMINAL_ACTIVE.store(true, Ordering::SeqCst);
        Ok(Self { active: true })
    }

    fn restore(mut self) -> Result<()> {
        self.active = false;
        restore_terminal()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.active {
            let _ = restore_terminal();
        }
    }
}

fn restore_terminal() -> Result<()> {
    if !TERMINAL_ACTIVE.swap(false, Ordering::SeqCst) {
        return Ok(());
    }
    let raw_mode_result = disable_raw_mode();
    let screen_result = execute!(stdout(), LeaveAlternateScreen, Show);
    raw_mode_result.and(screen_result)
}

fn install_panic_hook() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        previous_hook(panic_info);
    }));
}

fn init_logging() -> Result<()> {
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.log")?;
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Trace,
        Config::default(),
        log_file,
    )])
    .map_err(|error| io::Error::other(error.to_string()))
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging()?;
    install_panic_hook();
    let terminal_session = TerminalSession::enter()?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

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

    drop(terminal);
    terminal_session.restore()
}
