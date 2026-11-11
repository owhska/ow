//! Translate crossterm key events into Emacs-style key names ("C-x", "M-f", "RET"...).
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Debug)]
pub struct Key {
    pub name: String,
    /// Set when the key is a plain printable character (self-insert candidate).
    pub ch: Option<char>,
}

pub fn parse(ev: &KeyEvent) -> Option<Key> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }
    let m = ev.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    let mut ch = None;
    let mut name = String::new();
    match ev.code {
        KeyCode::Char(c) => {
            if ctrl {
                name.push_str("C-");
            }
            if alt {
                name.push_str("M-");
            }
            let base: String = if ctrl {
                match c {
                    ' ' | '@' | '2' => "SPC".into(),
                    // legacy terminals send 0x1f for C-/ and C-_ ; crossterm reports it as '7'
                    '7' | '_' => "_".into(),
                    c => c.to_ascii_lowercase().to_string(),
                }
            } else if c == ' ' {
                "SPC".into()
            } else {
                c.to_string()
            };
            name.push_str(&base);
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

/// Apply the Meta modifier to an already-named key (used for the ESC prefix).
pub fn meta_of(key: &str) -> String {
    match key.strip_prefix("C-") {
        Some(rest) => format!("C-M-{rest}"),
        None => format!("M-{key}"),
    }
}
