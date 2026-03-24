// Rust guideline compliant 2026-02-21

//! Slash command parsing for the TUI.
//!
//! Recognises `/quit` and `/extensions` from raw input strings.

/// A parsed slash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Exit the TUI.
    Quit,
    /// Open the extensions modal.
    Extensions,
    /// An unrecognised slash command.
    Unknown(String),
}

/// Parses a raw input string into an optional `Command`.
///
/// Returns `None` if the input does not start with `/`.
pub fn parse(input: &str) -> Option<Command> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    // Match only the command word (before any whitespace) so future
    // subcommands can be added without breaking this parser.
    let word = trimmed.split_whitespace().next().unwrap_or(trimmed);
    match word {
        "/quit" | "/q" => Some(Command::Quit),
        "/extensions" | "/ext" => Some(Command::Extensions),
        _ => Some(Command::Unknown(word.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quit() {
        assert_eq!(parse("/quit"), Some(Command::Quit));
        assert_eq!(parse("/q"), Some(Command::Quit));
        assert_eq!(parse("  /quit  "), Some(Command::Quit));
    }

    #[test]
    fn parse_extensions() {
        assert_eq!(parse("/extensions"), Some(Command::Extensions));
        assert_eq!(parse("/ext"), Some(Command::Extensions));
    }

    #[test]
    fn parse_unknown() {
        assert_eq!(parse("/foo"), Some(Command::Unknown("/foo".to_owned())));
    }

    #[test]
    fn parse_non_slash() {
        assert_eq!(parse("hello"), None);
        assert_eq!(parse(""), None);
    }
}
