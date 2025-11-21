# TUI Music Player (Rust)

A terminal-based music player written in Rust using
[ratatui](https://github.com/ratatui-org/ratatui) and
[crossterm](https://github.com/crossterm-rs/crossterm).

Navigate your filesystem, browse audio files, and play music — all in the
terminal.

## Features

- TUI interface with multiple screens
  - Library view with artist/album hierarchy
  - File browser
  - Playlist (coming soon)
- Navigate directories using keyboard
- Dotfiles are hidden by default
- Modular, extensible codebase
- Persistent library with smart metadata handling
  - Directory-based album grouping (prevents album splitting)
  - Filename fallbacks for missing ID3 tags
  - Automatic track number extraction from filenames
  - Filters out macOS metadata files (__MACOSX, ._ files)
- ZIP import: Extract and import albums directly from zip files
- Search functionality: Quick jump to artists, albums, or tracks
- Delete from library: Remove entries without touching actual files
- Playback controls with autoplay and repeat modes

## Screenshots

<details>
<summary>
Library Screenshot 1
</summary>
<img alt="Shelltrax screenshot 1" src="screenshots/shelltrax-1.png" width="1200"/>
</details>

<details>
<summary>
Library Screenshot 2
</summary>
<img alt="Shelltrax screenshot 2" src="screenshots/shelltrax-2.png" width="1200"/>
</details>

<details>
<summary>
Browser Screenshot 1
</summary>
<img alt="Shelltrax screenshot 3" src="screenshots/shelltrax-3.png" width="1200"/>
</details>

## Installation

```bash
git clone git@github.com:ducks/shelltrax.git
cd shelltrax
cargo run
```

## Configuration

### Theming

Shelltrax supports custom color themes via a TOML configuration file.

1. Copy the example theme file:
```bash
mkdir -p ~/.config/shelltrax
cp theme.toml.example ~/.config/shelltrax/theme.toml
```

2. Edit `~/.config/shelltrax/theme.toml` to customize colors

The theme supports:
- Named colors (e.g., `red`, `green`, `cyan`)
- Hex colors (e.g., `#b16286`)
- See `theme.toml.example` for all available options and examples

## Keybindings

Shelltrax supports both arrow keys and vim-style navigation.

### Global

| Key           | Action                          |
|---------------|---------------------------------|
| `q`           | Quit                            |
| `1`           | Go to Library                   |
| `5`           | Go to Browser                   |
| `/`           | Enter search mode               |
| `Esc`         | Exit search mode                |

### Navigation

| Key           | Action                          |
|---------------|---------------------------------|
| `j` / `Down`  | Move down                       |
| `k` / `Up`    | Move up                         |
| `h`           | Move left (context-dependent)   |
| `l`           | Move right (context-dependent)  |
| `g`           | Go to top                       |
| `G`           | Go to bottom                    |

### Browser View

| Key           | Action                          |
|---------------|---------------------------------|
| `a`           | Add file/dir to library         |
| `x`           | Extract and import zip file     |
| `Backspace`   | Go up a directory               |
| `Enter`       | Open directory                  |

### Library View

| Key           | Action                          |
|---------------|---------------------------------|
| `Tab`         | Toggle focus left/right         |
| `Enter`       | Play selected track             |
| `Space`       | Toggle artist/album expanded    |
| `d`           | Delete from library (files safe)|
| `c`           | Toggle pause/resume             |
| `b`           | Next track                      |
| `z`           | Previous track                  |
| `p`           | Toggle autoplay                 |
| `r`           | Cycle repeat mode (Off/All/Track)|

## Planned Features / TODO

- [x] Hide dotfiles
- [x] Prevent duplication
- [x] Library persistence
- [x] Audio playback via `cpal`
- [x] Autoplay
- [x] Footer bar with now playing info and progress
- [x] Search functionality (jump to artists/albums/tracks)
- [x] Delete from library (without touching files)
- [x] ZIP import and extraction
- [x] Directory-based album grouping
- [x] Filename fallbacks for metadata
- [x] Repeat modes (Off/All/Track)
- [ ] Sort directories before files
- [ ] Playlist screen with queue
- [ ] Config file for keybindings and paths
- [ ] Save/restore last visited directory
- [ ] Match more `cmus` keybindings and behaviors (e.g. `v`, `:`)
- [ ] Gapless playback
- [ ] Audio visualizer

