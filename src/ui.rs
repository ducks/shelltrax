use crate::{
    app::{App, AppScreen},
    screens,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};
use std::time::{Duration, Instant};

pub fn draw_ui(frame: &mut Frame, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Main screen
            Constraint::Length(4), // Footer
        ])
        .split(frame.area());

    match app.screen {
        AppScreen::Library => screens::library::draw(frame, app, layout[0]),
        AppScreen::Browser => screens::browser::draw(frame, app, layout[0]),
    }

    render_footer(frame, app, layout[1]);
}

pub fn render_footer(
    f: &mut Frame,
    app: &App,
    area: Rect,
) {
    let block = Block::default().borders(Borders::TOP);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(start) = app.playback_start else { return };

    let now = Instant::now();
    let elapsed = if let Some(paused_at) = app.paused_at {
        paused_at.duration_since(start)
    } else {
        now.duration_since(start)
    };

    let adjusted = elapsed.saturating_sub(app.paused_duration);
    let total = Duration::from_secs(app.playback_duration);

    let has_duration = app.playback_duration > 0;

    let ratio = if has_duration && total.as_secs_f64() > 0.0 {
        adjusted.as_secs_f64() / total.as_secs_f64()
    } else {
        0.0
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(app.theme.parse_color(&app.theme.progress_bar)))
        .ratio(ratio.min(1.0));
    f.render_widget(gauge, layout[0]);

    let elapsed_secs = adjusted.as_secs();
    let time_text = if has_duration {
        let total_secs = total.as_secs();
        format!(
            "{:02}:{:02} / {:02}:{:02}",
            elapsed_secs / 60,
            elapsed_secs % 60,
            total_secs / 60,
            total_secs % 60
        )
    } else {
        format!(
            "{:02}:{:02} / --:--",
            elapsed_secs / 60,
            elapsed_secs % 60
        )
    };

    let time_display = Paragraph::new(time_text);
    f.render_widget(time_display, layout[1]);

    if let Some(track) = &app.current_track {
        let track_info = format!(
            "{} - {} - {}",
            track.artist, track.title, track.album
        );

        let autoplay_status = if app.autoplay_enabled { "on" } else { "off" };
        let status_info = format!(
            "repeat: {} | autoplay: {}",
            app.repeat_mode.as_str(),
            autoplay_status
        );

        // Split the bottom line into left and right sections
        let info_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70),
                Constraint::Percentage(30),
            ])
            .split(layout[2]);

        let track_display = Paragraph::new(track_info);
        f.render_widget(track_display, info_layout[0]);

        let status_display = Paragraph::new(status_info)
            .alignment(Alignment::Right);
        f.render_widget(status_display, info_layout[1]);
    }
}
