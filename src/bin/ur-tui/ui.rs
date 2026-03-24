// Rust guideline compliant 2026-02-21

//! Ratatui rendering for the TUI.
//!
//! `render` is the single entry point: it draws the message log,
//! the input bar, and (when active) a centred modal overlay.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{App, Mode};

/// Renders the full TUI for one frame.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Min(1),    // message log fills remaining height
        Constraint::Length(3), // input bar: border (1) + text (1) + border (1)
    ])
    .split(frame.area());

    render_messages(frame, app, chunks[0]);
    render_input(frame, app, chunks[1]);

    if let Mode::Modal { title, content } = &app.mode {
        render_modal(frame, title, content);
    }
}

// --- Private helpers ---

fn render_messages(frame: &mut Frame, app: &App, area: Rect) {
    let text = app
        .messages
        .iter()
        .map(|m| m.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });

    // Scroll to the bottom so the latest message is always visible.
    // Clamp to u16::MAX — terminals will never have more lines than this.
    let line_count = u16::try_from(app.messages.len()).unwrap_or(u16::MAX);
    let visible_height = area.height;
    let scroll = line_count.saturating_sub(visible_height);

    frame.render_widget(paragraph.scroll((scroll, 0)), area);
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Input");
    let inner = block.inner(area);

    frame.render_widget(block, area);

    let paragraph = Paragraph::new(app.input.as_str());
    frame.render_widget(paragraph, inner);

    // Position the terminal cursor within the input area.
    // Clamp to inner.width — cursor cannot be further right than the widget.
    let cursor_x = inner.x + u16::try_from(app.cursor_pos).unwrap_or(inner.width);
    let cursor_y = inner.y;
    if cursor_x < inner.x + inner.width {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn render_modal(frame: &mut Frame, title: &str, content: &str) {
    let area = centred_rect(60, 60, frame.area());

    // Blank the area behind the modal first.
    frame.render_widget(Clear, area);

    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let paragraph = Paragraph::new(content).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Returns a `Rect` centred in `r`, using the given percentage dimensions.
fn centred_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let width = r.width * percent_x / 100;
    let height = r.height * percent_y / 100;
    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
