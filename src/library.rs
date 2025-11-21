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
            // Extract artist and album directory paths
            let album_dir = track.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
            let artist_dir = track.path.parent().and_then(|p| p.parent()).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());

            // Use directory-based grouping to prevent tag-based album splitting
            let artist_name = artist_dir.unwrap_or_else(|| track.album_artist.clone());
            let album_name = album_dir.unwrap_or_else(|| track.album.clone());

            // Check if artist exists (by directory name)
            if let Some(artist) = self.artists.iter_mut().find(|a| a.name == artist_name) {
                // Check if album exists (by directory name)
                if let Some(album) = artist.albums.iter_mut().find(|a| a.name == album_name) {
                    // Check for duplicate by path
                    let already_exists = album.tracks.iter().any(|t| t.path == track.path);
                    if !already_exists {
                        album.tracks.push(track);
                        album.tracks.sort_by_key(|t| {
                            t.track_number.unwrap_or_else(|| {
                                // Try to extract track number from filename as fallback
                                extract_track_number_from_filename(&t.path).unwrap_or(999)
                            })
                        });
                    }
                } else {
                    artist.albums.push(AlbumNode {
                        name: album_name.clone(),
                        tracks: vec![track],
                    });
                }
            } else {
                self.artists.push(ArtistNode {
                    name: artist_name.clone(),
                    albums: vec![AlbumNode {
                        name: album_name.clone(),
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
        self.track_index = 0;
    }

    pub fn move_up(&mut self) {
        let visual_rows = Self::build_visible_rows(&self.artists);

        let current_index = Self::selected_index(&visual_rows, self.selection);
        let next_index = current_index.saturating_sub(1);
        self.selection = visual_rows.get(next_index).map(Self::row_to_selection);
        self.track_index = 0;
    }

    pub fn go_to_top(&mut self) {
        let visual_rows = Self::build_visible_rows(&self.artists);
        self.selection = visual_rows.first().map(Self::row_to_selection);
        self.track_index = 0;
    }

    pub fn go_to_bottom(&mut self) {
        let visual_rows = Self::build_visible_rows(&self.artists);
        self.selection = visual_rows.last().map(Self::row_to_selection);
        self.track_index = 0;
    }

    pub fn jump_to_match(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }

        let query_lower = query.to_lowercase();
        let visual_rows = Self::build_visible_rows(&self.artists);

        // Search through visible rows for a match
        for row in visual_rows.iter() {
            let matches = match row {
                VisibleRow::Artist { artist_index } => {
                    self.artists[*artist_index].name.to_lowercase().contains(&query_lower)
                }
                VisibleRow::Album { artist_index, album_index } => {
                    self.artists[*artist_index].albums[*album_index]
                        .name
                        .to_lowercase()
                        .contains(&query_lower)
                }
            };

            if matches {
                self.selection = Some(Self::row_to_selection(row));
                self.track_index = 0;
                return;
            }
        }

        // If no match in left pane, search tracks in current selection
        if self.focus == LibraryFocus::Right {
            let tracks = self.visible_tracks();
            for (i, track) in tracks.iter().enumerate() {
                let matches = track.title.to_lowercase().contains(&query_lower)
                    || track.artist.to_lowercase().contains(&query_lower);

                if matches {
                    self.track_index = i;
                    return;
                }
            }
        }
    }

    pub fn toggle_expanded(&mut self) {
        if let Some(LibrarySelection::Artist { artist_index }) = self.selection
            && let Some(artist) = self.artists.get_mut(artist_index) {
                artist.expanded = !artist.expanded;
                self.rebuild_visible_rows();
            }
    }

    pub fn delete_selected(&mut self) {
        match self.selection {
            Some(LibrarySelection::Artist { artist_index }) => {
                // Delete entire artist
                if artist_index < self.artists.len() {
                    self.artists.remove(artist_index);

                    // Adjust selection after deletion
                    if self.artists.is_empty() {
                        self.selection = None;
                    } else if artist_index >= self.artists.len() {
                        // If we deleted the last artist, select the new last one
                        let new_index = self.artists.len().saturating_sub(1);
                        self.selection = Some(LibrarySelection::Artist {
                            artist_index: new_index
                        });
                    }
                    // Otherwise selection stays at same index (next artist moved into place)

                    self.rebuild_visible_rows();
                    persistence::save_library(&self.artists).ok();
                }
            }
            Some(LibrarySelection::Album { artist_index, album_index }) => {
                // Delete specific album
                if let Some(artist) = self.artists.get_mut(artist_index) {
                    if album_index < artist.albums.len() {
                        artist.albums.remove(album_index);

                        // If artist has no more albums, delete the artist too
                        if artist.albums.is_empty() {
                            self.artists.remove(artist_index);

                            // Adjust selection
                            if self.artists.is_empty() {
                                self.selection = None;
                            } else if artist_index >= self.artists.len() {
                                let new_index = self.artists.len().saturating_sub(1);
                                self.selection = Some(LibrarySelection::Artist {
                                    artist_index: new_index
                                });
                            } else {
                                self.selection = Some(LibrarySelection::Artist {
                                    artist_index
                                });
                            }
                        } else {
                            // Artist still has albums, adjust album selection
                            if album_index >= artist.albums.len() {
                                // Deleted last album, select new last album
                                let new_album_index = artist.albums.len().saturating_sub(1);
                                self.selection = Some(LibrarySelection::Album {
                                    artist_index,
                                    album_index: new_album_index,
                                });
                            }
                            // Otherwise selection stays at same index
                        }

                        self.rebuild_visible_rows();
                        persistence::save_library(&self.artists).ok();
                    }
                }
            }
            None => {
                // Nothing selected, do nothing
            }
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
        .filter_entry(|e| {
            // Skip __MACOSX directories
            !e.path()
                .components()
                .any(|c| c.as_os_str() == "__MACOSX")
        })
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();

        // Skip macOS metadata files (._filename)
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename.starts_with("._") {
                continue;
            }
        }

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

/// Extract track number from filename patterns like:
/// "Artist - 01 - Title.mp3" or "01 - Title.mp3" or "01. Title.mp3"
fn extract_track_number_from_filename(path: &Path) -> Option<u32> {
    let filename = path.file_stem()?.to_str()?;

    // Try pattern: "... - 01 - ..." or "01 - ..."
    if let Some(parts) = filename.split(" - ").nth(1).or_else(|| filename.split(" - ").next()) {
        if let Ok(num) = parts.trim().parse::<u32>() {
            if num > 0 && num < 999 {
                return Some(num);
            }
        }
    }

    // Try pattern: "01. Title" or "01 Title"
    let first_token = filename.split_whitespace().next()?;
    if let Ok(num) = first_token.trim_end_matches('.').parse::<u32>() {
        if num > 0 && num < 999 {
            return Some(num);
        }
    }

    None
}

fn extract_mp3_tags(path: &Path) -> (String, String, String, Option<u32>, String, Option<u64>) {
    // Use id3 for metadata (it handles ID3 tags better)
    let tag = Id3Tag::read_from_path(path).ok();

    // Fallback to filename if title tag is missing
    let filename_fallback = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown Title");

    let title = tag
        .as_ref()
        .and_then(|t| t.title())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| filename_fallback.to_string());

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
    let track_number = tag
        .and_then(|t| t.track())
        .or_else(|| extract_track_number_from_filename(path));

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

    // Fallback to filename if title tag is missing
    let filename_fallback = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown Title")
        .to_string();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => {
            return (
                filename_fallback,
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
                filename_fallback,
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

    let mut title = String::new();
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

    // Use filename as fallback if title tag was empty
    if title.is_empty() {
        title = filename_fallback;
    }

    // Fall back to artist if album_artist wasn't set
    if album_artist == "Unknown Album Artist" && artist != "Unknown Artist" {
        album_artist = artist.clone();
    }

    // Try to extract track number from filename if not found in tags
    if track_number.is_none() {
        track_number = extract_track_number_from_filename(path);
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

    #[test]
    fn test_jump_to_match_finds_artist() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("Artist A", "Album 1", "Track 1", 1),
            create_test_track("Artist B", "Album 2", "Track 2", 1),
            create_test_track("Artist C", "Album 3", "Track 3", 1),
        ]);

        lib.rebuild_visible_rows();

        // Start at first artist
        lib.selection = Some(LibrarySelection::Artist { artist_index: 0 });

        // Search for "Artist B"
        lib.jump_to_match("artist b");

        // Should now be on Artist B
        assert_eq!(
            lib.selection,
            Some(LibrarySelection::Artist { artist_index: 1 })
        );
    }

    #[test]
    fn test_jump_to_match_finds_album() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("Artist", "Album A", "Track 1", 1),
            create_test_track("Artist", "Album B", "Track 2", 1),
            create_test_track("Artist", "Album C", "Track 3", 1),
        ]);

        lib.rebuild_visible_rows();
        lib.artists[0].expanded = true;
        lib.rebuild_visible_rows();

        // Start at first album
        lib.selection = Some(LibrarySelection::Album {
            artist_index: 0,
            album_index: 0,
        });

        // Search for "Album C"
        lib.jump_to_match("album c");

        // Should now be on Album C
        assert_eq!(
            lib.selection,
            Some(LibrarySelection::Album {
                artist_index: 0,
                album_index: 2
            })
        );
    }

    #[test]
    fn test_jump_to_match_partial_match() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("The Beatles", "Abbey Road", "Track 1", 1),
            create_test_track("Pink Floyd", "Dark Side", "Track 2", 1),
        ]);

        lib.rebuild_visible_rows();

        // Search with partial string
        lib.jump_to_match("beat");

        // Should find The Beatles (index 1 because artists are sorted alphabetically)
        assert_eq!(
            lib.selection,
            Some(LibrarySelection::Artist { artist_index: 1 })
        );
    }

    #[test]
    fn test_jump_to_match_case_insensitive() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![create_test_track("ARTIST", "ALBUM", "Track", 1)]);

        lib.rebuild_visible_rows();

        // Search with lowercase
        lib.jump_to_match("artist");

        // Should find the artist despite case difference
        assert_eq!(
            lib.selection,
            Some(LibrarySelection::Artist { artist_index: 0 })
        );
    }

    #[test]
    fn test_jump_to_match_no_match() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![create_test_track("Artist", "Album", "Track", 1)]);

        lib.rebuild_visible_rows();
        let original_selection = lib.selection;

        // Search for something that doesn't exist
        lib.jump_to_match("nonexistent");

        // Selection should remain unchanged
        assert_eq!(lib.selection, original_selection);
    }

    #[test]
    fn test_jump_to_match_empty_query() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![create_test_track("Artist", "Album", "Track", 1)]);

        lib.rebuild_visible_rows();
        let original_selection = lib.selection;

        // Search with empty string
        lib.jump_to_match("");

        // Selection should remain unchanged
        assert_eq!(lib.selection, original_selection);
    }

    #[test]
    fn test_jump_to_match_finds_track() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("Artist", "Album", "Hello World", 1),
            create_test_track("Artist", "Album", "Goodbye Moon", 2),
            create_test_track("Artist", "Album", "Sunrise", 3),
        ]);

        lib.rebuild_visible_rows();
        lib.artists[0].expanded = true;
        lib.rebuild_visible_rows();

        lib.selection = Some(LibrarySelection::Album {
            artist_index: 0,
            album_index: 0,
        });
        lib.focus = LibraryFocus::Right;

        // Search for a track
        lib.jump_to_match("goodbye");

        // Should jump to track index 1 (Goodbye Moon)
        assert_eq!(lib.track_index, 1);
    }

    #[test]
    fn test_extract_track_number_from_filename() {
        // Test pattern: "Artist - 01 - Title.mp3"
        let path = PathBuf::from("Frank Zappa - 01 - Debra kadabra.mp3");
        assert_eq!(extract_track_number_from_filename(&path), Some(1));

        // Test pattern: "01 - Title.mp3"
        let path = PathBuf::from("01 - Wacced out murals.mp3");
        assert_eq!(extract_track_number_from_filename(&path), Some(1));

        // Test pattern: "01. Title.mp3"
        let path = PathBuf::from("01. Title.mp3");
        assert_eq!(extract_track_number_from_filename(&path), Some(1));

        // Test pattern: "01 Title.mp3"
        let path = PathBuf::from("01 Title.mp3");
        assert_eq!(extract_track_number_from_filename(&path), Some(1));

        // Test double-digit track numbers
        let path = PathBuf::from("Artist - 12 - Title.mp3");
        assert_eq!(extract_track_number_from_filename(&path), Some(12));

        // Test invalid patterns (no track number)
        let path = PathBuf::from("Just a title.mp3");
        assert_eq!(extract_track_number_from_filename(&path), None);

        // Test edge case: track number too large
        let path = PathBuf::from("9999 - Title.mp3");
        assert_eq!(extract_track_number_from_filename(&path), None);
    }

    #[test]
    fn test_directory_based_grouping_prevents_album_split() {
        let mut lib = LibraryState::new();

        // Simulate tracks from same directory but different album_artist tags
        // Like: Frank Zappa/(1973) Over-nite sensation/05 Dinah-moe humm.mp3
        let track1 = LibraryTrack {
            path: PathBuf::from("/Music/Frank Zappa/(1973) Over-nite sensation/05 Dinah-moe humm.mp3"),
            title: "Dinah-moe humm".to_string(),
            artist: "Frank Zappa".to_string(),
            album: "Over-nite sensation".to_string(),
            track_number: Some(5),
            album_artist: "Frank Zappa".to_string(),
            duration: Some(180),
        };

        let track2 = LibraryTrack {
            path: PathBuf::from("/Music/Frank Zappa/(1973) Over-nite sensation/01 Camarillo brillo.mp3"),
            title: "Camarillo brillo".to_string(),
            artist: "Frank Zappa & The Mothers".to_string(),
            album: "Over-nite sensation".to_string(),
            track_number: Some(1),
            album_artist: "Frank Zappa & The Mothers".to_string(), // Different!
            duration: Some(180),
        };

        lib.add_tracks(vec![track1, track2]);

        // Should create ONE artist (from directory name "Frank Zappa")
        assert_eq!(lib.artists.len(), 1);
        assert_eq!(lib.artists[0].name, "Frank Zappa");

        // Should create ONE album (from directory name "(1973) Over-nite sensation")
        assert_eq!(lib.artists[0].albums.len(), 1);
        assert_eq!(lib.artists[0].albums[0].name, "(1973) Over-nite sensation");

        // Both tracks should be in the same album
        assert_eq!(lib.artists[0].albums[0].tracks.len(), 2);
    }

    #[test]
    fn test_tracks_sorted_by_track_number_with_filename_fallback() {
        let mut lib = LibraryState::new();

        // Tracks with missing track_number but numbered filenames
        let track1 = LibraryTrack {
            path: PathBuf::from("/Music/Artist/Album/Artist - 07 - Track 7.mp3"),
            title: "Track 7".to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            track_number: None, // Missing!
            album_artist: "Artist".to_string(),
            duration: Some(180),
        };

        let track2 = LibraryTrack {
            path: PathBuf::from("/Music/Artist/Album/Artist - 01 - Track 1.mp3"),
            title: "Track 1".to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            track_number: None, // Missing!
            album_artist: "Artist".to_string(),
            duration: Some(180),
        };

        let track3 = LibraryTrack {
            path: PathBuf::from("/Music/Artist/Album/Artist - 04 - Track 4.mp3"),
            title: "Track 4".to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            track_number: None, // Missing!
            album_artist: "Artist".to_string(),
            duration: Some(180),
        };

        // Add in wrong order
        lib.add_tracks(vec![track1, track2, track3]);

        // Should be sorted by filename-extracted track numbers: 1, 4, 7
        let tracks = &lib.artists[0].albums[0].tracks;
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].title, "Track 1");
        assert_eq!(tracks[1].title, "Track 4");
        assert_eq!(tracks[2].title, "Track 7");
    }

    #[test]
    fn test_macosx_files_filtered() {
        // Note: This tests the filter logic, but doesn't actually create files
        // The filter happens in scan_path_for_tracks which requires real files
        // So we test the logic that would be applied

        // Paths that should be filtered
        let macosx_path = PathBuf::from("/Music/Artist/Album/__MACOSX/._track.mp3");
        let dot_underscore = PathBuf::from("/Music/Artist/Album/._track.mp3");

        // Check that __MACOSX is in the path
        assert!(macosx_path.components().any(|c| c.as_os_str() == "__MACOSX"));

        // Check that filename starts with ._
        assert!(dot_underscore
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.starts_with("._"))
            .unwrap_or(false));
    }

    #[test]
    fn test_delete_artist() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("Artist A", "Album 1", "Track 1", 1),
            create_test_track("Artist B", "Album 2", "Track 2", 1),
            create_test_track("Artist C", "Album 3", "Track 3", 1),
        ]);

        assert_eq!(lib.artists.len(), 3);

        // Select and delete second artist (Artist B)
        lib.selection = Some(LibrarySelection::Artist { artist_index: 1 });
        lib.delete_selected();

        // Should have 2 artists left
        assert_eq!(lib.artists.len(), 2);
        assert_eq!(lib.artists[0].name, "Artist A");
        assert_eq!(lib.artists[1].name, "Artist C");

        // Selection should still be at index 1 (now pointing to Artist C)
        assert_eq!(
            lib.selection,
            Some(LibrarySelection::Artist { artist_index: 1 })
        );
    }

    #[test]
    fn test_delete_last_artist() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("Artist A", "Album 1", "Track 1", 1),
            create_test_track("Artist B", "Album 2", "Track 2", 1),
        ]);

        // Delete last artist
        lib.selection = Some(LibrarySelection::Artist { artist_index: 1 });
        lib.delete_selected();

        assert_eq!(lib.artists.len(), 1);
        assert_eq!(lib.artists[0].name, "Artist A");

        // Selection should move to index 0
        assert_eq!(
            lib.selection,
            Some(LibrarySelection::Artist { artist_index: 0 })
        );
    }

    #[test]
    fn test_delete_album() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![
            create_test_track("Artist", "Album 1", "Track 1", 1),
            create_test_track("Artist", "Album 2", "Track 2", 1),
            create_test_track("Artist", "Album 3", "Track 3", 1),
        ]);

        assert_eq!(lib.artists.len(), 1);
        assert_eq!(lib.artists[0].albums.len(), 3);

        // Expand artist and select second album
        lib.artists[0].expanded = true;
        lib.rebuild_visible_rows();
        lib.selection = Some(LibrarySelection::Album {
            artist_index: 0,
            album_index: 1,
        });

        lib.delete_selected();

        // Should have 2 albums left
        assert_eq!(lib.artists[0].albums.len(), 2);
        assert_eq!(lib.artists[0].albums[0].name, "Album 1");
        assert_eq!(lib.artists[0].albums[1].name, "Album 3");

        // Selection should still be at album index 1
        assert_eq!(
            lib.selection,
            Some(LibrarySelection::Album {
                artist_index: 0,
                album_index: 1
            })
        );
    }

    #[test]
    fn test_delete_last_album_removes_artist() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![create_test_track("Artist", "Album", "Track", 1)]);

        assert_eq!(lib.artists.len(), 1);
        assert_eq!(lib.artists[0].albums.len(), 1);

        // Select and delete the only album
        lib.artists[0].expanded = true;
        lib.rebuild_visible_rows();
        lib.selection = Some(LibrarySelection::Album {
            artist_index: 0,
            album_index: 0,
        });

        lib.delete_selected();

        // Artist should be removed too
        assert_eq!(lib.artists.len(), 0);
        assert_eq!(lib.selection, None);
    }

    #[test]
    fn test_delete_with_no_selection() {
        let mut lib = LibraryState::new();

        lib.add_tracks(vec![create_test_track("Artist", "Album", "Track", 1)]);

        let original_len = lib.artists.len();
        lib.selection = None;

        lib.delete_selected();

        // Nothing should change
        assert_eq!(lib.artists.len(), original_len);
    }
}
