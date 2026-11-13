//! ow: a Micro GNU Emacs clone in Rust with built-in git and wide syntax highlighting.
mod buffer;
mod clipboard;
mod editor;
#[cfg(test)]
mod editor_tests;
mod git;
mod keys;
mod langs;
mod syntax;
mod ui;

use editor::Editor;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
};
use ratatui::crossterm::execute;
use ratatui::DefaultTerminal;
use std::io::{self, Write};
use std::time::Duration;

const HELP: &str = "ow - Micro GNU Emacs in Rust, with built-in git and syntax highlighting

Usage: ow [+LINE] [FILE...]
       ow --list-languages | --version | --help

Keys (Emacs): C-x C-f find-file, C-x C-s save, C-x C-c quit, C-s/C-r search,
M-% query-replace, M-x commands, C-h b all bindings.
Git:     C-x g status, C-x v = diff, C-x v l log, C-x v g blame, C-x v a stage,
         C-x v c commit, C-x v o checkout, C-x v n/p hunks, C-x v F/P pull/push.
Project: C-x p f find file in project, C-x p g grep project, M-g n/p next/prev match.
Env:     OW_THEME=light|dark|<syntect theme name>";

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Copy to the system clipboard through the terminal (OSC 52).
fn osc52(text: &str) {
    if text.len() > 100_000 {
        return;
    }
    let mut out = io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
}

fn run(term: &mut DefaultTerminal, ed: &mut Editor) -> io::Result<()> {
    loop {
        term.draw(|f| ui::render(f, ed))?;
        if ed.quit {
            return Ok(());
        }
        if let Some(c) = ed.clip.take() {
            osc52(&c);
            clipboard::set_clipboard(&c);
        }
        if event::poll(Duration::from_millis(250))? {
            loop {
                match event::read()? {
                    Event::Key(k) => ed.on_key(k),
                    Event::Paste(s) => ed.on_paste(&s),
                    Event::Mouse(m) => ed.on_mouse(m),
                    _ => {}
                }
                if ed.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        } else {
            ed.tick();
        }
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version") {
        println!("ow {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--list-languages") {
        for n in syntax::Highlighter::list_names() {
            println!("{n}");
        }
        return Ok(());
    }

    let mut ed = Editor::new();
    let has_files = args.iter().any(|a| !a.starts_with('+'));
    if !has_files { ed.restore_session(); }
    let mut line: Option<usize> = None;
    let mut first = true;
    for a in &args {
        if let Some(n) = a.strip_prefix('+').and_then(|n| n.parse::<usize>().ok()) {
            line = Some(n);
            continue;
        }
        ed.find_file(a);
        if first {
            first = false;
        }
        if let Some(n) = line.take() {
            ed.goto_line_in_cur(n, 0);
        }
    }
    // show the first file given
    if args.iter().any(|a| !a.starts_with('+')) {
        let msg = ed.msg.clone();
        let first_idx = ed.bufs.iter().position(|b| b.kind == buffer::BufKind::File).unwrap_or(0);
        ed.switch_to(first_idx);
        ed.msg = msg;
    }
    ed.refresh_git_info();
    ed.refresh_marks();

    let mut term = ratatui::init();
    let _ = execute!(io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    let res = run(&mut term, &mut ed);
    ed.save_session();
    let _ = execute!(io::stdout(), DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
    res
}
