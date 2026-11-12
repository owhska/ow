//! Normalize crossterm events into the key names used by the editor.
//!
//! This layer is deliberately independent of the editing model: Vim handles
//! navigation and modal keys, while the command layer handles non-motion
//! commands. Both consume the same normalized names (for example `C-r`,
//! `M-x`, `S-TAB`, and `<left>`).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub name: String,
    /// Plain printable character; used for insertion in Insert mode and prompts.
    pub ch: Option<char>,
}

/// Convert a terminal key event into the editor's canonical key notation.
///
/// Key releases are ignored so terminals that report both press and release
/// do not execute a command twice. Repeated key presses are kept for normal
/// key-repeat behavior.
pub fn parse(ev: &KeyEvent) -> Option<Key> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }

    let modifiers = ev.modifiers;
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    let shift = modifiers.contains(KeyModifiers::SHIFT);

    let mut name = String::new();
    let mut ch = None;

    match ev.code {
        KeyCode::Char(c) => {
            if ctrl {
                name.push_str("C-");
            }
            if alt {
                name.push_str("M-");
            }

            // Terminals differ in how they report Ctrl+Space and Ctrl+_.
            // Normalize the common representations to one stable spelling.
            let base = if ctrl {
                match c {
                    ' ' | '@' | '2' => "SPC".to_string(),
                    // Some terminals report Ctrl+_ (0x1f) as Ctrl+7.
                    '7' | '_' => "_".to_string(),
                    other => other.to_ascii_lowercase().to_string(),
                }
            } else if c == ' ' {
                "SPC".to_string()
            } else {
                c.to_string()
            };

            name.push_str(&base);

            // Modified characters are commands, not text insertion.
            if !ctrl && !alt {
                ch = Some(c);
            }
        }
        code => {
            let base = match code {
                KeyCode::Enter => "RET".to_string(),
                KeyCode::Tab => "TAB".to_string(),
                KeyCode::BackTab => "S-TAB".to_string(),
                KeyCode::Backspace => "DEL".to_string(),
                KeyCode::Delete => "<delete>".to_string(),
                KeyCode::Left => "<left>".to_string(),
                KeyCode::Right => "<right>".to_string(),
                KeyCode::Up => "<up>".to_string(),
                KeyCode::Down => "<down>".to_string(),
                KeyCode::Home => "<home>".to_string(),
                KeyCode::End => "<end>".to_string(),
                KeyCode::PageUp => "<pageup>".to_string(),
                KeyCode::PageDown => "<pagedown>".to_string(),
                KeyCode::Insert => "<insert>".to_string(),
                KeyCode::Esc => "ESC".to_string(),
                KeyCode::F(n) => format!("<f{n}>"),
                _ => return None,
            };

            if ctrl {
                name.push_str("C-");
            }
            if alt {
                name.push_str("M-");
            }
            if shift && code != KeyCode::BackTab {
                name.push_str("S-");
            }
            name.push_str(&base);
        }
    }

    Some(Key { name, ch })
}

/// Add Meta to a key name when ESC was used as a Meta prefix.
pub fn meta_of(key: &str) -> String {
    match key.strip_prefix("C-") {
        Some(rest) => format!("C-M-{rest}"),
        None => format!("M-{key}"),
    }
}

