use std::fs::File;
use std::path::{Path, PathBuf};

use ratatui::widgets::{ListItem, ListState};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;


use id3::Tag as Id3Tag;
use symphonia::core::{
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use symphonia::default::get_probe;

use crate::persistence;

#[derive(Debug, Clone)]
pub enum VisibleRow {
    Artist {
        artist_index: usize,
    },
    Album {
        artist_index: usize,
        album_index: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySelection {
    Artist {
        artist_index: usize,
    },
    Album {
        artist_index: usize,
        album_index: usize,
    },
}

pub struct LibraryState {
    pub artists: Vec<ArtistNode>,
    pub selection: Option<LibrarySelection>,
    pub state: ListState,
    pub focus: LibraryFocus,
    pub track_index: usize,
    pub visible_rows: Vec<VisibleRow>,
    pub tracks: Vec<LibraryTrack>
}

impl LibraryState {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));

        Self {
            artists: Vec::new(),
            selection: Some(LibrarySelection::Artist { artist_index: 0 }),
            state,
            focus: LibraryFocus::Left,
            track_index: 0,
            visible_rows: Vec::new(),
            tracks: Vec::new(),
        }
    }

    pub fn add_tracks(&mut self, tracks: Vec<LibraryTrack>) {
        for track in tracks {
            // Check if artist exists
            if let Some(artist) = self.artists.iter_mut().find(|a| a.name == track.album_artist) {
                // Check if album exists
                if let Some(album) = artist.albums.iter_mut().find(|a| a.name == track.album) {
                    // Check for duplicate by path
                    let already_exists = album.tracks.iter().any(|t| t.path == track.path);
                    if !already_exists {
                        album.tracks.push(track);
                        album.tracks.sort_by_key(|t| t.track_number.unwrap_or(999));
                    }
                } else {
                    artist.albums.push(AlbumNode {
                        name: track.album.clone(),
                        tracks: vec![track],
                    });
                }
            } else {
                self.artists.push(ArtistNode {
                    name: track.album_artist.clone(),
                    albums: vec![AlbumNode {
                        name: track.album.clone(),
                        tracks: vec![track],
                    }],
                    expanded: false,
                });
            }
        }

        self.artists.sort_by_key(|a| a.name.clone());

        self.rebuild_visible_rows(); // <-- Important

        // Optional: auto-select the first row
        if self.selection.is_none() && !self.visible_rows.is_empty() {
            self.selection = Some(Self::row_to_selection(&self.visible_rows[0]));
            self.state.select(Some(0));
        }

        persistence::save_library(&self.artists).ok();
    }

    pub fn move_down(&mut self) {
        let visual_rows = Self::build_visible_rows(&self.artists);

        let current_index = Self::selected_index(&visual_rows, self.selection);
        let next_index = (current_index + 1).min(visual_rows.len().saturating_sub(1));
        self.selection = visual_rows.get(next_index).map(Self::row_to_selection);
    }

    pub fn move_up(&mut self) {
        let visual_rows = Self::build_visible_rows(&self.artists);

        let current_index = Self::selected_index(&visual_rows, self.selection);
        let next_index = current_index.saturating_sub(1);
        self.selection = visual_rows.get(next_index).map(Self::row_to_selection);
    }

    pub fn go_to_top(&mut self) {
        let visual_rows = Self::build_visible_rows(&self.artists);
        self.selection = visual_rows.first().map(Self::row_to_selection);
    }

    pub fn go_to_bottom(&mut self) {
        let visual_rows = Self::build_visible_rows(&self.artists);
        self.selection = visual_rows.last().map(Self::row_to_selection);
    }

    pub fn toggle_expanded(&mut self) {
        if let Some(LibrarySelection::Artist { artist_index }) = self.selection
            && let Some(artist) = self.artists.get_mut(artist_index) {
                artist.expanded = !artist.expanded;
                self.rebuild_visible_rows();
            }
    }

    fn build_visible_rows(artists: &[ArtistNode]) -> Vec<VisibleRow> {
        let mut rows = Vec::new();
        for (artist_index, artist) in artists.iter().enumerate() {
            rows.push(VisibleRow::Artist { artist_index });
            if artist.expanded {
                for (album_index, _album) in artist.albums.iter().enumerate() {
                    rows.push(VisibleRow::Album {
                        artist_index,
                        album_index,
                    });
                }
            }
        }
        rows
    }

    fn selected_index(rows: &[VisibleRow], selection: Option<LibrarySelection>) -> usize {
        rows.iter()
            .position(|row| match (row, selection) {
                (
                    VisibleRow::Artist { artist_index, .. },
                    Some(LibrarySelection::Artist { artist_index: ai }),
                ) => *artist_index == ai,
                (
                    VisibleRow::Album {
                        artist_index,
                        album_index,
                        ..
                    },
                    Some(LibrarySelection::Album {
                        artist_index: ai,
                        album_index: bi,
                    }),
                ) => *artist_index == ai && *album_index == bi,
                _ => false,
            })
            .unwrap_or(0)
    }

    pub fn row_to_selection(row: &VisibleRow) -> LibrarySelection {
        match row {
            VisibleRow::Artist { artist_index } => LibrarySelection::Artist {
                artist_index: *artist_index,
            },
            VisibleRow::Album {
                artist_index,
                album_index,
            } => LibrarySelection::Album {
                artist_index: *artist_index,
                album_index: *album_index,
            },
        }
    }

    pub fn tab_focus(&mut self) {
        self.focus = match self.focus {
            LibraryFocus::Left => LibraryFocus::Right,
            LibraryFocus::Right => LibraryFocus::Left,
        };
    }

    pub fn move_track_up(&mut self) {
        if self.track_index > 0 {
            self.track_index -= 1;
        }
    }

    pub fn move_track_down(&mut self, track_count: usize) {
        if self.track_index + 1 < track_count {
            self.track_index += 1;
        }
    }

    pub fn track_go_to_top(&mut self) {
        self.track_index = 0;
    }

    pub fn track_go_to_bottom(&mut self, track_count: usize) {
        self.track_index = track_count.saturating_sub(1);
    }

    pub fn visible_tracks(&self) -> Vec<LibraryTrack> {
        match self.selection {
            Some(LibrarySelection::Artist { artist_index }) => self
                .artists
                .get(artist_index)
                .map(|a| a.albums.iter().flat_map(|alb| alb.tracks.clone()).collect())
                .unwrap_or_default(),
                Some(LibrarySelection::Album {
                    artist_index,
                    album_index,
                }) => self
            .artists
                .get(artist_index)
                .and_then(|a| a.albums.get(album_index))
                .map(|alb| alb.tracks.clone())
                .unwrap_or_default(),
            None => vec![],
        }
    }

    pub fn rebuild_visible_rows(&mut self) {
        self.visible_rows.clear();

        for (artist_index, artist) in self.artists.iter().enumerate() {
            self.visible_rows.push(VisibleRow::Artist { artist_index });

            if artist.expanded {
                for (album_index, _) in artist.albums.iter().enumerate() {
                    self.visible_rows.push(VisibleRow::Album {
                        artist_index,
                        album_index,
                    });
                }
            }
        }

        // Restore selection if it was valid
        if self.visible_rows.is_empty() {
            self.selection = None;
            self.state.select(None);
        } else {
            // Default to first item if selection missing or invalid
            let current_index = self
                .visible_rows
                .iter()
                .position(|row| Some(Self::row_to_selection(row)) == self.selection)
                .unwrap_or(0);

            self.selection = Some(Self::row_to_selection(&self.visible_rows[current_index]));
            self.state.select(Some(current_index));
        }
    }

    pub fn right_pane_items(&self) -> (Vec<ListItem<'_>>, Vec<usize>) {
        let tracks = self.visible_tracks();
        let mut items = Vec::new();
        let mut playable_indices = Vec::new();
        let mut last_album: Option<String> = None;

        for track in tracks {
            let album = track.album.clone();

            if last_album.as_deref() != Some(album.as_str()) {
                items.push(ListItem::new(format!("{}:", album)));
                last_album = Some(album);
            }

            playable_indices.push(items.len()); // index where this track will be
            let number = track
                .track_number
                .map_or("--".to_string(), |n| format!("{:02}", n));
            items.push(ListItem::new(format!("  {}. {}", number, track.title)));
        }

        (items, playable_indices)
    }

    pub fn next_track_path(&self, current: &Path) -> Option<PathBuf> {
        let tracks = self.visible_tracks();

        for (i, track) in tracks.iter().enumerate() {
            if track.path == current && i + 1 < tracks.len() {
                return Some(tracks[i + 1].path.clone());
            }
        }

        None
    }

    pub fn select_track_by_path(&mut self, path: &Path) {
        let tracks = self.visible_tracks();
        if let Some(i) = tracks.iter().position(|t| t.path == path) {
            self.track_index = i;
            self.state.select(Some(i));
        }
    }

    pub fn track_by_path(&self, path: &Path) -> Option<&LibraryTrack> {
        for artist in &self.artists {
            for album in &artist.albums {
                if let Some(track) = album.tracks.iter().find(|t| t.path == path) {
                    return Some(track);
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryTrack {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_number: Option<u32>,
    pub album_artist: String,
    pub duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumNode {
    pub name: String,
    pub tracks: Vec<LibraryTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistNode {
    pub name: String,
    pub albums: Vec<AlbumNode>,
    pub expanded: bool,
}

/// Scans a path recursively and parses audio files into LibraryTrack entries.
pub fn scan_path_for_tracks(path: &Path) -> Vec<LibraryTrack> {
    let mut tracks = Vec::new();

    for entry in WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();

        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_ascii_lowercase());

        let (title, artist, album, track_number, album_artist, duration) = match ext.as_deref() {
            Some("mp3") => extract_mp3_tags(path),
            Some("flac") => extract_symphonia_tags(path),
            _ => continue,
        };

        tracks.push(LibraryTrack {
            path: path.to_path_buf(),
            title,
            artist,
            album,
            track_number,
            album_artist,
            duration,
        });
    }

    tracks
}

fn extract_mp3_tags(path: &Path) -> (String, String, String, Option<u32>, String, Option<u64>) {
    // Use id3 for metadata (it handles ID3 tags better)
    let tag = Id3Tag::read_from_path(path).ok();

    let title = tag
        .as_ref()
        .and_then(|t| t.title())
        .unwrap_or("Unknown Title")
        .to_string();
    let artist = tag
        .as_ref()
        .and_then(|t| t.artist())
        .unwrap_or("Unknown Artist")
        .to_string();
    let album = tag
        .as_ref()
        .and_then(|t| t.album())
        .unwrap_or("Unknown Album")
        .to_string();
    let album_artist = tag
        .as_ref()
        .and_then(|t| t.album_artist())
        .unwrap_or(&artist)
        .to_string();
    let track_number = tag.and_then(|t| t.track());

    // Use Symphonia for duration
    let duration = extract_duration_symphonia(path);

    (title, artist, album, track_number, album_artist, duration)
}

fn extract_duration_symphonia(path: &Path) -> Option<u64> {
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let probed = get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;

    if let Some(track) = probed.format.default_track()
        && let Some(tb) = track.codec_params.time_base
            && let Some(n_frames) = track.codec_params.n_frames {
                return Some((n_frames * tb.numer as u64) / tb.denom as u64);
            }

    None
}

fn extract_symphonia_tags(path: &Path) -> (String, String, String, Option<u32>, String, Option<u64>) {
    use symphonia::core::meta::StandardTagKey;

    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => {
            return (
                "Unknown Title".into(),
                "Unknown Artist".into(),
                "Unknown Album".into(),
                None,
                "Unknown Album Artist".into(),
                None,
            );
        }
    };

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut probed = match get_probe().format(
        &Hint::new(),
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    ) {
        Ok(p) => p,
        Err(_) => {
            return (
                "Unknown Title".into(),
                "Unknown Artist".into(),
                "Unknown Album".into(),
                None,
                "Unknown Album Artist".into(),
                None,
            );
        }
    };

    let binding = probed.format.metadata();
    let meta = binding.current();

    let mut title = "Unknown Title".to_string();
    let mut artist = "Unknown Artist".to_string();
    let mut album_artist = "Unknown Album Artist".to_string();
    let mut album = "Unknown Album".to_string();
    let mut track_number = None;

    if let Some(m) = meta {
        for tag in m.tags() {
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) => title = tag.value.to_string(),
                Some(StandardTagKey::Artist) => artist = tag.value.to_string(),
                Some(StandardTagKey::AlbumArtist) => album_artist = tag.value.to_string(),
                Some(StandardTagKey::Album) => album = tag.value.to_string(),
                Some(StandardTagKey::TrackNumber) => {
                    track_number = tag.value.to_string().parse::<u32>().ok();
                }
                _ => {}
            }
        }
    }

    let mut duration = None;

    if let Some(track) = probed.format.default_track()
        && let Some(tb) = track.codec_params.time_base
            && let Some(n_frames) = track.codec_params.n_frames {
                duration = Some((n_frames * tb.numer as u64) / tb.denom as u64);
            }

    (title, artist, album, track_number, album_artist, duration)
}

#[derive(PartialEq)]
pub enum LibraryFocus {
    Left,
    Right,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_track(
        artist: &str,
        album: &str,
        title: &str,
        track_num: u32,
    ) -> LibraryTrack {
        LibraryTrack {
            path: PathBuf::from(format!("/test/{}/{}/{}.mp3", artist, album, title)),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            track_number: Some(track_num),
            album_artist: artist.to_string(),
            duration: Some(180),
        }
    }

    #[test]
    fn test_add_tracks_creates_structure() {
        let mut lib = LibraryState::new();

        let tracks = vec![
            create_test_track("Artist A", "Album 1", "Track 1", 1),
            create_test_track("Artist A", "Album 1", "Track 2", 2),
            create_test_track("Artist B", "Album 2", "Track 3", 1),
        ];

        lib.add_tracks(tracks);

        assert_eq!(lib.artists.len(), 2);
        assert_eq!(lib.artists[0].name, "Artist A");
        assert_eq!(lib.artists[0].albums.len(), 1);
        assert_eq!(lib.artists[0].albums[0].tracks.len(), 2);
        assert_eq!(lib.artists[1].name, "Artist B");
    }

    #[test]
    fn test_track_by_path_finds_track() {
        let mut lib = LibraryState::new();

        let track = create_test_track("Artist", "Album", "Title", 1);
        let path = track.path.clone();

        lib.add_tracks(vec![track]);

        let found = lib.track_by_path(&path);
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "Title");
    }

    #[test]
    fn test_track_by_path_returns_none_for_missing() {
        let lib = LibraryState::new();

        let result = lib.track_by_path(Path::new("/nonexistent/path.mp3"));
        assert!(result.is_none());
    }

    #[test]
    fn test_toggle_expanded() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![create_test_track("Artist", "Album", "Track", 1)]);

        assert!(!lib.artists[0].expanded);

        lib.toggle_expanded();

        assert!(lib.artists[0].expanded);
    }

    #[test]
    fn test_visible_tracks_for_artist() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("Artist", "Album 1", "Track 1", 1),
            create_test_track("Artist", "Album 2", "Track 2", 1),
        ]);

        lib.selection = Some(LibrarySelection::Artist { artist_index: 0 });
        let tracks = lib.visible_tracks();

        assert_eq!(tracks.len(), 2);
    }

    #[test]
    fn test_visible_tracks_for_album() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("Artist", "Album 1", "Track 1", 1),
            create_test_track("Artist", "Album 1", "Track 2", 2),
            create_test_track("Artist", "Album 2", "Track 3", 1),
        ]);

        lib.selection = Some(LibrarySelection::Album {
            artist_index: 0,
            album_index: 0,
        });
        let tracks = lib.visible_tracks();

        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "Track 1");
        assert_eq!(tracks[1].title, "Track 2");
    }
}
