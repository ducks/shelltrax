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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppScreen {
    Library,
    Browser,
}

pub struct App {
    pub screen: AppScreen,
    pub browser: BrowserState,
    pub library: Arc<Mutex<LibraryState>>,
    pub player: Arc<Mutex<Player>>,
    pub play_queue: Vec<PathBuf>,
    pub queue_index: usize,
    pub autoplay_enabled: bool,
    pub current_track: Option<LibraryTrack>,

    /// Playback duration in seconds
    pub playback_duration: u64,

    /// Playback start
    pub playback_start: Option<Instant>,

    pub paused_at: Option<Instant>,
    pub paused_duration: Duration,
}

impl App {
    pub fn new() -> Self {
        let artists = persistence::load_library().unwrap_or_else(|_| vec![]);

        let library = Arc::new(Mutex::new(LibraryState::new()));
        library.lock().unwrap().artists = artists;
        library.lock().unwrap().rebuild_visible_rows(); // Make sure UI stays in sync

        Self {
            screen: AppScreen::Browser,
            browser: BrowserState::new(),
            library,
            player: Arc::new(Mutex::new(Player::new())),
            play_queue: Vec::new(),
            queue_index: 0,
            autoplay_enabled: true,
            current_track: None,
            playback_duration: 0,
            playback_start: None,
            paused_at: None,
            paused_duration: Duration::from_secs(0),
        }
    }

    pub fn player_mut(&self) -> std::sync::MutexGuard<'_, Player> {
        self.player.lock().unwrap()
    }

    pub fn library_mut(&self) -> std::sync::MutexGuard<'_, LibraryState> {
        self.library.lock().unwrap()
    }

    pub fn update(&mut self) {
        if self.autoplay_enabled
            && self.player_mut().is_loaded()
            && self.player_mut().is_done()
            && !self.player_mut().is_playing
        {
            self.play_next_track();
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

    pub fn set_play_queue(&mut self, tracks: Vec<PathBuf>, start_index: usize) {
        self.play_queue = tracks;
        self.queue_index = start_index;
    }

    pub fn pause(&mut self) {
        let mut player = self.player.lock().unwrap();
        player.set_paused(true);
        self.paused_at = Some(Instant::now());
    }

    pub fn resume(&mut self) {
        let mut player = self.player.lock().unwrap();
        player.set_paused(false);

        if let Some(paused_at) = self.paused_at {
            self.paused_duration += paused_at.elapsed();
        }

        self.paused_at = None;
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
        // optional: if you rely on app.playback_duration elsewhere
        self.playback_duration = track.duration.unwrap_or(0);
    }

    /// Fallback: reset timers when we don't have a concrete track object yet.
    pub fn reset_playback_timers(&mut self) {
        self.playback_start = Some(Instant::now());
        self.paused_duration = Duration::ZERO;
        self.paused_at = None;
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

        let track = create_test_track("test", 180);
        app.begin_playback(&track);

        assert!(app.playback_start.is_some());
        assert_eq!(app.paused_duration, Duration::ZERO);
        assert!(app.paused_at.is_none());
        assert_eq!(app.playback_duration, 180);
        assert_eq!(app.current_track.as_ref().unwrap().title, "test");
    }

    #[test]
    fn test_pause_sets_paused_at() {
        let mut app = App::new();

        assert!(app.paused_at.is_none());

        app.pause();

        assert!(app.paused_at.is_some());
        assert!(app.player_mut().is_paused);
    }

    #[test]
    fn test_resume_accumulates_paused_duration() {
        let mut app = App::new();

        let start = Instant::now();
        app.paused_at = Some(start);
        app.paused_duration = Duration::from_secs(5);

        std::thread::sleep(Duration::from_millis(100));

        app.resume();

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
