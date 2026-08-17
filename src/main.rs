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
use std::io::{self, Result, Write, stdout};

use simplelog::*;
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

struct CappedLogWriter {
    file: std::fs::File,
    written: u64,
    limit: u64,
}

impl Write for CappedLogWriter {
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        let remaining = self.limit.saturating_sub(self.written) as usize;
        if remaining > 0 {
            let to_write = buffer.len().min(remaining);
            self.file.write_all(&buffer[..to_write])?;
            self.written += to_write as u64;
        }
        // Logging must never become an unbounded disk-pressure source. Report
        // the whole write as consumed once this session reaches its cap.
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<()> {
        self.file.flush()
    }
}

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
        .write(true)
        .truncate(true)
        .open("debug.log")?;
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Debug,
        Config::default(),
        CappedLogWriter {
            file: log_file,
            written: 0,
            limit: MAX_LOG_BYTES,
        },
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

                    if let Some(path) = current_path
                        && app.play_path(&path)
                    {
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

    // Tear audio down while the TUI is still visibly alive. In particular,
    // never make a blocked backend shutdown look like a successful exit.
    app.player.lock().unwrap().stop();
    drop(terminal);
    terminal_session.restore()
}

#[cfg(test)]
mod logging_tests {
    use super::*;

    #[test]
    fn capped_writer_never_exceeds_its_limit() {
        let path = std::env::temp_dir().join(format!(
            "shelltrax-capped-log-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let mut writer = CappedLogWriter {
            file,
            written: 0,
            limit: 8,
        };

        assert_eq!(writer.write(b"1234567890").unwrap(), 10);
        assert_eq!(writer.write(b"more").unwrap(), 4);
        writer.flush().unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 8);

        std::fs::remove_file(path).unwrap();
    }
}
