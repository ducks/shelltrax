# TUI Music Player (Rust)

A terminal-based music player written in Rust using
[ratatui](https://github.com/ratatui-org/ratatui) and
[crossterm](https://github.com/crossterm-rs/crossterm).

Navigate your filesystem, browse audio files, and play music — all in the
terminal.

## Features

- TUI interface with multiple screens
  - Library view
  - File browser
  - Playlist (coming soon)
- Navigate directories using keyboard
- Dotfiles are hidden by default
- Modular, extensible codebase
- Persistent library

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

## Keybindings

Shelltrax supports both arrow keys and vim-style navigation.

### Global

| Key           | Action                          |
|---------------|---------------------------------|
| `q`           | Quit                            |
| `1`           | Go to Library                   |
| `5`           | Go to Browser                   |

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
| `Backspace`   | Go up a directory               |
| `Enter`       | Open directory                  |

### Library View

| Key           | Action                          |
|---------------|---------------------------------|
| `Tab`         | Toggle focus left/right         |
| `Enter`       | Play selected track             |
| `Space`       | Toggle artist/album expanded    |
| `c`           | Toggle pause/resume             |
| `b`           | Next track                      |
| `z`           | Previous track                  |
| `p`           | Toggle autoplay                 |

## Planned Features / TODO

- [x] Hide dotfiles
- [x] Prevent duplication
- [x] Library persistence
- [x] Audio playback via `rodio`
- [x] Autoplay
- [x] Footer bar with now playing info and progress
- [ ] Sort directories before files
- [ ] Playlist screen with queue
- [ ] Config file for keybindings and paths
- [ ] Save/restore last visited directory
- [ ] Match more `cmus` keybindings and behaviors (e.g. `v`, `:`)

