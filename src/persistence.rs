use crate::library::ArtistNode;
use std::{fs, path::Path};

#[cfg(not(test))]
fn get_save_path() -> &'static str {
    "library.json"
}

#[cfg(test)]
fn get_save_path() -> &'static str {
    "library.test.json"
}

pub fn save_library(artists: &[ArtistNode]) -> std::io::Result<()> {
    let data = serde_json::to_string_pretty(artists)?;
    fs::write(get_save_path(), data)?;
    Ok(())
}

pub fn load_library() -> std::io::Result<Vec<ArtistNode>> {
    let path = get_save_path();
    if Path::new(path).exists() {
        let data = fs::read_to_string(path)?;
        let artists = serde_json::from_str(&data)?;
        Ok(artists)
    } else {
        Ok(vec![]) // start empty if no file
    }
}
