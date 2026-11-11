//! System clipboard integration (arboard, wl-clipboard, xclip).

use std::io::Write;
use std::sync::{LazyLock, Mutex};

static CLIPBOARD: LazyLock<Mutex<Option<arboard::Clipboard>>> = LazyLock::new(|| {
    Mutex::new(arboard::Clipboard::new().ok())
});

pub fn set_clipboard(text: &str) {
    if let Ok(mut guard) = CLIPBOARD.lock() {
        if let Some(cb) = guard.as_mut() {
            let _ = cb.set_text(text);
        } else if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(text);
            *guard = Some(cb);
        }
    }

    // Also try wl-copy and xclip as background fallbacks so external tools have it
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        if let Ok(mut child) = std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
    }
}

pub fn get_clipboard() -> Option<String> {
    if let Ok(mut guard) = CLIPBOARD.lock() {
        if let Some(cb) = guard.as_mut() {
            if let Ok(t) = cb.get_text() {
                if !t.is_empty() {
                    return Some(t);
                }
            }
        } else if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(t) = cb.get_text() {
                *guard = Some(cb);
                if !t.is_empty() {
                    return Some(t);
                }
            } else {
                *guard = Some(cb);
            }
        }
    }

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let Ok(out) = std::process::Command::new("wl-paste").arg("--no-newline").output() {
            if out.status.success() {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        if let Ok(out) = std::process::Command::new("xclip").args(["-selection", "clipboard", "-o"]).output() {
            if out.status.success() {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
        }
    }
    None
}
