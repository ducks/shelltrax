use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::browser::BrowserState;

use crate::library::{
    LibraryState,
    LibraryTrack
};

use crate::persistence;

use crate::player::Player;
use crate::scrobbler::{Scrobbler, ScrobblerConfig};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppScreen {
    Library,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    Track,
    All,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::All,
            RepeatMode::All => RepeatMode::Off,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RepeatMode::Off => "off",
            RepeatMode::Track => "track",
            RepeatMode::All => "all",
        }
    }
}

pub struct App {
    pub screen: AppScreen,
    pub browser: BrowserState,
    pub library: Arc<Mutex<LibraryState>>,
    pub player: Arc<Mutex<Player>>,
    pub play_queue: Vec<PathBuf>,
    pub queue_index: usize,
    pub autoplay_enabled: bool,
    pub repeat_mode: RepeatMode,
    pub current_track: Option<LibraryTrack>,
    pub theme: Theme,

    /// Playback duration in seconds
    pub playback_duration: u64,

    /// Playback start
    pub playback_start: Option<Instant>,

    pub paused_at: Option<Instant>,
    pub paused_duration: Duration,

    /// Search mode
    pub search_active: bool,
    pub search_query: String,

    /// Scrobbler
    pub scrobbler: Scrobbler,

    /// Track if we've scrobbled the current track
    pub scrobbled_current: bool,
}

impl App {
    pub fn new() -> Self {
        let artists = persistence::load_library().unwrap_or_else(|_| vec![]);

        let library = Arc::new(Mutex::new(LibraryState::new()));
        library.lock().unwrap().artists = artists;
        library.lock().unwrap().rebuild_visible_rows(); // Make sure UI stays in sync

        let scrobbler_config = ScrobblerConfig::from_env();
        let scrobbler = Scrobbler::new(scrobbler_config);

        #[allow(clippy::arc_with_non_send_sync)]
        Self {
            screen: AppScreen::Browser,
            browser: BrowserState::new(),
            library,
            player: Arc::new(Mutex::new(Player::new())),
            play_queue: Vec::new(),
            queue_index: 0,
            autoplay_enabled: true,
            repeat_mode: RepeatMode::All,
            current_track: None,
            theme: Theme::load(),
            playback_duration: 0,
            playback_start: None,
            paused_at: None,
            paused_duration: Duration::from_secs(0),
            search_active: false,
            search_query: String::new(),
            scrobbler,
            scrobbled_current: false,
        }
    }

    pub fn player_mut(&self) -> std::sync::MutexGuard<'_, Player> {
        self.player.lock().unwrap()
    }

    pub fn library_mut(&self) -> std::sync::MutexGuard<'_, LibraryState> {
        self.library.lock().unwrap()
    }

    pub fn update(&mut self) {
        // Check if we should scrobble the current track
        self.check_and_scrobble();

        if self.autoplay_enabled
            && self.player_mut().is_loaded()
            && self.player_mut().is_done()
            && !self.player_mut().is_playing
        {
            self.play_next_track();
        }
    }

    fn check_and_scrobble(&mut self) {
        if self.scrobbled_current || !self.scrobbler.is_enabled() {
            return;
        }

        let Some(ref track) = self.current_track else {
            return;
        };

        let Some(playback_start) = self.playback_start else {
            return;
        };

        // Calculate actual play time (excluding paused time)
        let elapsed = playback_start.elapsed();
        let paused = if let Some(paused_at) = self.paused_at {
            self.paused_duration + paused_at.elapsed()
        } else {
            self.paused_duration
        };
        let play_time = elapsed.saturating_sub(paused);

        // Scrobble after 50% of track or 4 minutes, whichever comes first
        let scrobble_threshold = if let Some(duration) = track.duration {
            let half_duration = Duration::from_secs(duration / 2);
            let four_minutes = Duration::from_secs(240);
            half_duration.min(four_minutes)
        } else {
            // If we don't know duration, scrobble after 4 minutes
            Duration::from_secs(240)
        };

        if play_time >= scrobble_threshold {
            let timestamp = std::time::SystemTime::now()
                .checked_sub(play_time)
                .unwrap_or(std::time::SystemTime::now());

            self.scrobbler.scrobble(
                &track.artist,
                &track.title,
                Some(&track.album),
                track.duration,
                timestamp,
            );

            self.scrobbled_current = true;
        }
    }

    pub fn goto_screen(&mut self, screen: AppScreen) {
        self.screen = screen
    }

    pub fn play_next_track(&mut self) {
        if self.queue_index + 1 >= self.play_queue.len() {
            log::debug!("Reached end of queue");
            self.queue_index = 0;
            self.play_queue.clear();
            self.current_track = None;
            self.playback_start = None;
            return;
        }

        self.queue_index += 1;
        let next_path = self.play_queue[self.queue_index].clone();

        {
            let mut lib = self.library.lock().unwrap();
            lib.select_track_by_path(&next_path);
        }

        if let Some(track) = {
            let lib = self.library.lock().unwrap();
            lib.track_by_path(&next_path).cloned()
        } {
            self.begin_playback(&track);
        } else {
            log::warn!("Could not find LibraryTrack for: {:?}", next_path);
            self.playback_start = Some(Instant::now());
            self.paused_duration = Duration::ZERO;
            self.paused_at = None;
        }

        {
            let mut player = self.player.lock().unwrap();
            player.play(&next_path);
        }
    }

    pub fn toggle_pause(&mut self) {
        let mut player = self.player.lock().unwrap();

        if player.is_paused {
            player.set_paused(false);
            if let Some(at) = self.paused_at.take() {
                self.paused_duration += at.elapsed();
            }
        } else {
            player.set_paused(true);
            self.paused_at = Some(Instant::now());
        }
    }

    pub fn begin_playback(&mut self, track: &LibraryTrack) {
        self.current_track = Some(track.clone());
        self.playback_start = Some(std::time::Instant::now());
        self.paused_duration = std::time::Duration::ZERO;
        self.paused_at = None;
        self.playback_duration = track.duration.unwrap_or(0);
        self.scrobbled_current = false;

        // Update now playing
        if self.scrobbler.is_enabled() {
            self.scrobbler.update_now_playing(
                &track.artist,
                &track.title,
                Some(&track.album),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_track(title: &str, duration: u64) -> LibraryTrack {
        LibraryTrack {
            path: PathBuf::from(format!("/test/{}.mp3", title)),
            title: title.to_string(),
            artist: "Test Artist".to_string(),
            album: "Test Album".to_string(),
            track_number: Some(1),
            album_artist: "Test Artist".to_string(),
            duration: Some(duration),
        }
    }

    #[test]
    fn test_begin_playback_resets_timers() {
        let mut app = App::new();

        app.playback_start = Some(Instant::now());
        app.paused_duration = Duration::from_secs(10);
        app.paused_at = Some(Instant::now());
        app.scrobbled_current = true;

        let track = create_test_track("test", 180);
        app.begin_playback(&track);

        assert!(app.playback_start.is_some());
        assert_eq!(app.paused_duration, Duration::ZERO);
        assert!(app.paused_at.is_none());
        assert_eq!(app.playback_duration, 180);
        assert_eq!(app.current_track.as_ref().unwrap().title, "test");
        assert!(!app.scrobbled_current, "scrobbled_current should be reset");
    }

    #[test]
    fn test_toggle_pause_sets_paused_at() {
        let mut app = App::new();

        assert!(app.paused_at.is_none());

        app.toggle_pause();

        assert!(app.paused_at.is_some());
        assert!(app.player_mut().is_paused);
    }

    #[test]
    fn test_toggle_pause_accumulates_paused_duration() {
        let mut app = App::new();

        let start = Instant::now();
        app.paused_at = Some(start);
        app.paused_duration = Duration::from_secs(5);

        app.toggle_pause();

        std::thread::sleep(Duration::from_millis(100));

        app.toggle_pause();

        assert!(app.paused_at.is_none());
        assert!(app.paused_duration > Duration::from_secs(5));
        assert!(!app.player_mut().is_paused);
    }

    #[test]
    fn test_toggle_pause_cycles_state() {
        let mut app = App::new();

        assert!(!app.player_mut().is_paused);

        app.toggle_pause();
        assert!(app.player_mut().is_paused);
        assert!(app.paused_at.is_some());

        app.toggle_pause();
        assert!(!app.player_mut().is_paused);
        assert!(app.paused_at.is_none());
    }
}
