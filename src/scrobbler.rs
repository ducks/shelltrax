use last_fm_rs::Client;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone)]
pub struct ScrobblerConfig {
    pub enabled: bool,
    pub server_url: String,
    pub token: String,
}

impl ScrobblerConfig {
    pub fn from_env() -> Option<Self> {
        let server_url = std::env::var("SCROB_SERVER_URL").ok()?;
        let token = std::env::var("SCROB_TOKEN").ok()?;

        Some(Self {
            enabled: true,
            server_url,
            token,
        })
    }
}

pub struct Scrobbler {
    client: Option<Arc<Client>>,
}

impl Scrobbler {
    pub fn new(config: Option<ScrobblerConfig>) -> Self {
        let client = config.and_then(|cfg| {
            if !cfg.enabled {
                return None;
            }

            match Client::with_token(&cfg.server_url, &cfg.token) {
                Ok(client) => {
                    log::info!("Scrobbler initialized with server: {}", cfg.server_url);
                    Some(Arc::new(client))
                }
                Err(e) => {
                    log::error!("Failed to create scrobble client: {}", e);
                    None
                }
            }
        });

        Self { client }
    }

    pub fn is_enabled(&self) -> bool {
        self.client.is_some()
    }

    pub fn update_now_playing(&self, artist: &str, track: &str, album: Option<&str>) {
        let Some(ref client) = self.client else {
            return;
        };

        let now_playing = last_fm_rs::NowPlaying {
            artist: artist.to_string(),
            track: track.to_string(),
            album: album.map(|s| s.to_string()),
            album_artist: None,
            duration: None,
            track_number: None,
        };

        let client = Arc::clone(client);
        let artist_owned = artist.to_string();
        let track_owned = track.to_string();

        tokio::spawn(async move {
            match client.update_now_playing(&now_playing).await {
                Ok(_) => log::info!("Now playing updated: {} - {}", artist_owned, track_owned),
                Err(e) => log::error!("Failed to update now playing: {}", e),
            }
        });
    }

    pub fn scrobble(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
        duration: Option<u64>,
        timestamp: SystemTime,
    ) {
        let Some(ref client) = self.client else {
            return;
        };

        // Convert SystemTime to Unix timestamp
        let timestamp_u64 = timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let scrobble = last_fm_rs::Scrobble {
            artist: artist.to_string(),
            track: track.to_string(),
            timestamp: timestamp_u64,
            album: album.map(|s| s.to_string()),
            album_artist: None,
            duration,
            track_number: None,
        };

        let client = Arc::clone(client);
        let artist_owned = artist.to_string();
        let track_owned = track.to_string();

        tokio::spawn(async move {
            match client.scrobble(&[scrobble]).await {
                Ok(_) => log::info!("Scrobbled: {} - {}", artist_owned, track_owned),
                Err(e) => log::error!("Failed to scrobble: {}", e),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrobbler_config_from_env() {
        unsafe {
            // Save original values
            let orig_url = std::env::var("SCROB_SERVER_URL").ok();
            let orig_token = std::env::var("SCROB_TOKEN").ok();

            // Test missing env vars
            std::env::remove_var("SCROB_SERVER_URL");
            std::env::remove_var("SCROB_TOKEN");

            let config = ScrobblerConfig::from_env();
            assert!(config.is_none());

            // Test with env vars set
            std::env::set_var("SCROB_SERVER_URL", "http://localhost:3000/graphql");
            std::env::set_var("SCROB_TOKEN", "test-token-123");

            let config = ScrobblerConfig::from_env().unwrap();
            assert_eq!(config.server_url, "http://localhost:3000/graphql");
            assert_eq!(config.token, "test-token-123");
            assert!(config.enabled);

            // Restore original values
            match orig_url {
                Some(url) => std::env::set_var("SCROB_SERVER_URL", url),
                None => std::env::remove_var("SCROB_SERVER_URL"),
            }
            match orig_token {
                Some(token) => std::env::set_var("SCROB_TOKEN", token),
                None => std::env::remove_var("SCROB_TOKEN"),
            }
        }
    }

    #[test]
    fn test_scrobbler_disabled_when_no_config() {
        let scrobbler = Scrobbler::new(None);
        assert!(!scrobbler.is_enabled());
    }

    #[test]
    fn test_scrobbler_disabled_when_config_disabled() {
        let config = ScrobblerConfig {
            enabled: false,
            server_url: "http://localhost:3000/graphql".to_string(),
            token: "test-token".to_string(),
        };

        let scrobbler = Scrobbler::new(Some(config));
        assert!(!scrobbler.is_enabled());
    }

    #[test]
    fn test_scrobbler_calls_dont_panic_when_disabled() {
        let scrobbler = Scrobbler::new(None);

        // These should not panic
        scrobbler.update_now_playing("Artist", "Track", Some("Album"));
        scrobbler.scrobble(
            "Artist",
            "Track",
            Some("Album"),
            Some(180),
            SystemTime::now(),
        );
    }
}
