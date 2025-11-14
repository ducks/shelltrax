use ratatui::{prelude::*, widgets::*};

use crate::app::App;

use crate::library::{LibraryFocus, LibrarySelection};

use crate::library::VisibleRow;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let library = app.library_mut();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // ───── Left: Artist/Album list ─────
    let mut left_items = Vec::new();
    let mut selected_idx = 0;

    for (i, row) in library.visible_rows.iter().enumerate() {
        let is_selected = Some(row_to_selection(row)) == library.selection;

        let label = match row {
            VisibleRow::Artist { artist_index } => {
                let artist = &library.artists[*artist_index];
                let marker = if artist.expanded { "▾" } else { "▸" };
                format!("{marker} {}", artist.name)
            }
            VisibleRow::Album {
                artist_index,
                album_index,
            } => {
                let album = &library.artists[*artist_index].albums[*album_index];
                format!("  {}", album.name)
            }
        };

        if is_selected {
            selected_idx = i;
        }

        let style = if is_selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        left_items.push(ListItem::new(label).style(style));
    }

    let mut left_state = ListState::default();
    left_state.select(Some(selected_idx));

    let left_list = List::new(left_items)
        .block(Block::default().title("Library").borders(Borders::ALL))
        .highlight_symbol("➤ ")
        .highlight_style(
            Style::default()
                .bg(app.theme.parse_color(&app.theme.background_selected))
                .fg(app.theme.parse_color(&app.theme.text_selected))
        );

    frame.render_stateful_widget(left_list, chunks[0], &mut left_state);

    // ───── Right: Tracks ─────
    let (right_items, playable_indices) = library.right_pane_items();

    let visual_index = playable_indices
        .get(library.track_index)
        .copied()
        .unwrap_or(0);

    let mut right_state = ListState::default();
    if library.focus == LibraryFocus::Right {
        right_state.select(Some(visual_index));
    }

    let right_list = List::new(right_items)
        .block(Block::default().title("Tracks").borders(Borders::ALL))
        .highlight_symbol("➤ ")
        .highlight_style(
            Style::default()
                .bg(app.theme.parse_color(&app.theme.background_selected))
                .fg(app.theme.parse_color(&app.theme.text_selected))
        );

    if library.focus == LibraryFocus::Right {
        frame.render_stateful_widget(right_list, chunks[1], &mut right_state);
    } else {
        frame.render_widget(right_list, chunks[1]);
    }
}

fn row_to_selection(row: &VisibleRow) -> LibrarySelection {
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
