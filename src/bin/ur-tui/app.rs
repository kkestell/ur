//! TUI application state and event handling.
//!
//! `App` owns all mutable UI state: the input buffer, the message log,
//! the workspace reference, and the active mode. `handle_event` drives
//! state transitions from raw crossterm events.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tracing::{debug, info};
use ur::manifest::ManifestEntry;
use ur::workspace::UrWorkspace;

use crate::commands;

/// Current interaction mode of the TUI.
#[derive(Debug, Clone)]
pub enum Mode {
    /// Default input mode.
    Normal,
    /// A read-only overlay modal is visible.
    Modal {
        /// Title shown in the modal border.
        title: String,
        /// Body text displayed inside the modal.
        content: String,
    },
}

/// A single entry in the message log.
#[derive(Debug, Clone)]
pub struct Message {
    /// Display text for the message.
    pub text: String,
}

/// All mutable state for a running TUI session.
pub struct App {
    /// Current interaction mode.
    pub mode: Mode,
    /// Raw text currently typed in the input bar.
    pub input: String,
    /// Byte-offset of the cursor within `input`.
    pub cursor_pos: usize,
    /// Accumulated message log shown in the main area.
    pub messages: Vec<Message>,
    /// Workspace reference used to execute commands.
    pub workspace: UrWorkspace,
    /// Set to `true` to break out of the event loop.
    pub should_quit: bool,
}

impl App {
    /// Creates a new `App` bound to the given workspace.
    pub fn new(workspace: UrWorkspace) -> Self {
        Self {
            mode: Mode::Normal,
            input: String::new(),
            cursor_pos: 0,
            messages: Vec::new(),
            workspace,
            should_quit: false,
        }
    }

    /// Dispatches a raw crossterm event to the appropriate handler.
    pub fn handle_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            match &self.mode {
                Mode::Normal => self.handle_key_normal(*key),
                Mode::Modal { .. } => self.handle_key_modal(*key),
            }
        }
    }

    fn handle_key_normal(&mut self, key: KeyEvent) {
        match key.code {
            // Ctrl+C exits from any state.
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Enter => {
                let input = std::mem::take(&mut self.input);
                self.cursor_pos = 0;
                self.submit_input(&input);
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    let prev = self.input[..self.cursor_pos]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i);
                    self.input.remove(prev);
                    self.cursor_pos = prev;
                }
            }
            KeyCode::Delete => {
                if self.cursor_pos < self.input.len() {
                    self.input.remove(self.cursor_pos);
                }
            }
            KeyCode::Left => {
                if self.cursor_pos > 0 {
                    self.cursor_pos = self.input[..self.cursor_pos]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i);
                }
            }
            KeyCode::Right => {
                if self.cursor_pos < self.input.len() {
                    self.cursor_pos = self.input[self.cursor_pos..]
                        .char_indices()
                        .nth(1)
                        .map_or(self.input.len(), |(i, _)| self.cursor_pos + i);
                }
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
            }
            KeyCode::End => {
                self.cursor_pos = self.input.len();
            }
            KeyCode::Char(ch) => {
                self.input.insert(self.cursor_pos, ch);
                self.cursor_pos += ch.len_utf8();
            }
            _ => {}
        }
    }

    fn handle_key_modal(&mut self, key: KeyEvent) {
        // Escape dismisses the modal; all other keys are ignored.
        if key.code == KeyCode::Esc {
            self.mode = Mode::Normal;
        }
    }

    fn submit_input(&mut self, input: &str) {
        if input.trim().is_empty() {
            return;
        }

        info!(input = input, "command submitted");
        match commands::parse(input) {
            Some(commands::Command::Quit) => {
                info!("quit requested");
                self.should_quit = true;
            }
            Some(commands::Command::Extensions) => {
                debug!("opening extensions modal");
                let content = format_extensions(self.workspace.list_extensions());
                self.mode = Mode::Modal {
                    title: "Extensions".to_owned(),
                    content,
                };
            }
            Some(commands::Command::Unknown(cmd)) => {
                debug!(command = cmd.as_str(), "unknown command");
                self.messages.push(Message {
                    text: format!("Unknown command: {cmd}"),
                });
            }
            None => {
                self.messages.push(Message {
                    text: format!("{input}  (LLM turns not yet implemented)"),
                });
            }
        }
    }
}

/// Formats the extension list as a multi-line string for the modal.
fn format_extensions(entries: &[ManifestEntry]) -> String {
    use std::fmt::Write;

    if entries.is_empty() {
        return "(no extensions)".to_owned();
    }

    let mut out = String::new();
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let status = if e.enabled { "enabled" } else { "disabled" };
        let _ = write!(out, "[{status}] {} \u{2014} {}", e.id, e.name);
    }
    out
}
