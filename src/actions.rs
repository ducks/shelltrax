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
    DeleteFromLibrary,

    // Browser
    GoUpDirectory,
    ImportZip,

    // Search
    EnterSearch,
    ExitSearch,

    // Global
    Quit,
}

impl Action {
    pub fn execute(&self, app: &mut App) {
        match self {
            Action::Quit => {
                // Handled in main loop
            }
            Action::MoveUp => match app.screen {
                AppScreen::Browser => app.browser.move_up(),
                AppScreen::Library => {
                    let mut lib = app.library_mut();
                    match lib.focus {
                        crate::library::LibraryFocus::Left => lib.move_up(),
                        crate::library::LibraryFocus::Right => lib.move_track_up(),
                    }
                }
            },
            Action::MoveDown => match app.screen {
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
            },
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
            Action::ToggleExpanded if app.screen == AppScreen::Library => {
                app.library_mut().toggle_expanded();
            }
            Action::SwitchFocus => {
                app.library_mut().tab_focus();
            }
            Action::GoUpDirectory if app.screen == AppScreen::Browser => {
                app.browser.go_up();
            }
            Action::GoToTop => match app.screen {
                AppScreen::Browser => app.browser.go_to_top(),
                AppScreen::Library => {
                    let mut lib = app.library_mut();
                    match lib.focus {
                        LibraryFocus::Left => lib.go_to_top(),
                        LibraryFocus::Right => lib.track_go_to_top(),
                    }
                }
            },
            Action::GoToBottom => match app.screen {
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
            },
            Action::AddToLibrary => {
                let mut lib = app.library_mut();

                if app.screen == AppScreen::Browser
                    && let Some(BrowserItem::Entry(path)) = app.browser.list.selected_item()
                {
                    let tracks = scan_path_for_tracks(path);
                    lib.tracks = tracks.clone();
                    lib.add_tracks(tracks);
                }
            }
            Action::DeleteFromLibrary if app.screen == AppScreen::Library => {
                let mut lib = app.library_mut();
                lib.delete_selected();
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
                            if let Err(e) = plyr.play(&track.path) {
                                log::error!("Playback failed for {:?}: {e}", track.path);
                                return;
                            }

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
            Action::EnterSearch => {
                app.search_active = true;
                app.search_query.clear();
            }
            Action::ExitSearch => {
                app.search_active = false;
                app.search_query.clear();
            }
            Action::ImportZip => {
                if app.screen == AppScreen::Browser
                    && let Some(BrowserItem::Entry(path)) = app.browser.list.selected_item()
                    && path.extension().and_then(|s| s.to_str()) == Some("zip")
                {
                    if let Err(e) = import_zip_archive(path, &app.browser.current_dir) {
                        log::error!("Failed to import zip: {}", e);
                    } else {
                        // Refresh browser to show the new folder
                        use crate::browser::BrowserState;
                        let current_dir = app.browser.current_dir.clone();
                        app.browser = BrowserState::new();
                        app.browser.current_dir = current_dir;
                        let entries = crate::browser::read_dir_items(&app.browser.current_dir);
                        app.browser.list.set_entries(entries);
                    }
                }
            }
            _ => {}
        }
    }
}

fn import_zip_archive(
    zip_path: &std::path::Path,
    target_dir: &std::path::Path,
) -> anyhow::Result<()> {
    use std::fs;

    // Open the zip file
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Create a temporary directory for extraction
    let temp_dir = target_dir.join(".temp_extract");
    fs::create_dir_all(&temp_dir)?;

    // Extract all files
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = temp_dir.join(file.mangled_name());

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                fs::create_dir_all(p)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    // Find first audio file and read metadata tags
    let audio_extensions = ["mp3", "flac", "m4a", "ogg", "wav"];
    let mut album_name = None;

    for entry in walkdir::WalkDir::new(&temp_dir) {
        let entry = entry?;
        if let Some(ext) = entry.path().extension().and_then(|s| s.to_str()) {
            let ext_lower = ext.to_lowercase();
            if audio_extensions.contains(&ext_lower.as_str()) {
                // Try to read ID3 tags if it's an MP3
                if ext_lower == "mp3" {
                    if let Ok(tag) = id3::Tag::read_from_path(entry.path())
                        && let Some(album) = tag.album()
                    {
                        album_name = Some(sanitize_folder_name(album));
                        break;
                    }
                }
                // Try to read metadata using symphonia for FLAC and other formats
                else if let Some(album) = read_audio_metadata(entry.path()) {
                    album_name = Some(sanitize_folder_name(&album));
                    break;
                }
            }
        }
    }

    // Use album name or fallback to zip filename
    let folder_name = album_name.unwrap_or_else(|| {
        zip_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported_album")
            .to_string()
    });

    let final_dir = target_dir.join(&folder_name);

    // Move temp directory to final location
    fs::rename(&temp_dir, &final_dir)?;

    log::info!("Imported zip to: {}", final_dir.display());

    Ok(())
}

fn read_audio_metadata(path: &std::path::Path) -> Option<String> {
    use std::fs::File;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }

    let meta_opts: MetadataOptions = Default::default();
    let format_opts = symphonia::core::formats::FormatOptions::default();

    let mut probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &meta_opts)
        .ok()?;

    // Try to get album from metadata
    if let Some(metadata) = probed.format.metadata().current() {
        for tag in metadata.tags() {
            if tag.std_key == Some(symphonia::core::meta::StandardTagKey::Album) {
                return Some(tag.value.to_string());
            }
        }
    }

    // Also check metadata from probe
    if let Some(metadata) = probed.metadata.get()
        && let Some(rev) = metadata.current()
    {
        for tag in rev.tags() {
            if tag.std_key == Some(symphonia::core::meta::StandardTagKey::Album) {
                return Some(tag.value.to_string());
            }
        }
    }

    None
}

fn sanitize_folder_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}
