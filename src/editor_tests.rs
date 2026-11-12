//! End-to-end tests: feed key events into the Editor like a terminal would.
use crate::buffer::*;
use crate::editor::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn ev(k: &str) -> KeyEvent {
    let (mods, rest) = if let Some(r) = k.strip_prefix("C-M-") {
        (KeyModifiers::CONTROL | KeyModifiers::ALT, r)
    } else if let Some(r) = k.strip_prefix("C-") {
        (KeyModifiers::CONTROL, r)
    } else if let Some(r) = k.strip_prefix("M-") {
        (KeyModifiers::ALT, r)
    } else {
        (KeyModifiers::NONE, k)
    };
    let code = match rest {
        "RET" => KeyCode::Enter,
        "DEL" => KeyCode::Backspace,
        "TAB" => KeyCode::Tab,
        "SPC" => KeyCode::Char(' '),
        "ESC" => KeyCode::Esc,
        "<left>" => KeyCode::Left,
        "<right>" => KeyCode::Right,
        "<up>" => KeyCode::Up,
        "<down>" => KeyCode::Down,
        r if r.chars().count() == 1 => KeyCode::Char(r.chars().next().unwrap()),
        r => panic!("bad key {r}"),
    };
    KeyEvent::new(code, mods)
}

/// "C-x C-s" style sequence; plain words are typed character by character.
fn keys(e: &mut Editor, seq: &str) {
    for k in seq.split(' ') {
        let named = k.starts_with("C-")
            || k.starts_with("M-")
            || matches!(
                k,
                "RET" | "DEL" | "TAB" | "SPC" | "ESC" | "<left>" | "<right>" | "<up>" | "<down>"
            );
        if named {
            e.on_key(ev(k));
        } else {
            for c in k.chars() {
                e.on_key(ev(&c.to_string()));
            }
        }
    }
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mgrs-ed-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(d).unwrap()
}

fn editor_in(dir: &PathBuf) -> Editor {
    let mut e = Editor::new();
    e.cwd = dir.clone();
    e.vim_mode = VimMode::Insert;
    e
}

#[test]
fn typing_kill_yank_undo() {
    let dir = tmpdir("kill");
    let mut e = editor_in(&dir);
    keys(&mut e, "hello SPC world RET second");
    assert_eq!(e.buf().lines, vec!["hello world", "second"]);
    keys(&mut e, "M-b C-k"); // kill "second"
    assert_eq!(e.buf().lines, vec!["hello world", ""]);
    keys(&mut e, "C-y C-y");
    assert_eq!(e.buf().lines[1], "secondsecond");
    keys(&mut e, "C-_"); // one undo group per command
    assert_eq!(e.buf().lines[1], "second");
    keys(&mut e, "C-_ C-_");
    assert_eq!(e.buf().lines, vec!["hello world", "second"]);
    keys(&mut e, "M-_");
    assert_eq!(e.buf().lines[1], "");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn consecutive_kills_append() {
    let dir = tmpdir("kills");
    let mut e = editor_in(&dir);
    keys(&mut e, "a RET b RET c C-p C-p C-a C-k C-k C-k C-k C-k");
    assert_eq!(e.kill_ring.last().unwrap(), "a\nb\nc");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn search_and_replace() {
    let dir = tmpdir("search");
    let mut e = editor_in(&dir);
    keys(&mut e, "foo SPC bar SPC foo SPC baz M-<");
    keys(&mut e, "C-s foo C-s RET");
    assert_eq!(e.buf().cursor, Pos::new(0, 11));
    keys(&mut e, "M-< M-% foo RET qux RET !");
    assert_eq!(e.buf().lines[0], "qux bar qux baz");
    keys(&mut e, "C-_");
    assert_eq!(e.buf().lines[0], "foo bar foo baz");
    // query-replace one at a time
    keys(&mut e, "M-< M-% foo RET X RET y n");
    assert_eq!(e.buf().lines[0], "X bar foo baz");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn auto_indent_and_brackets() {
    let dir = tmpdir("indent");
    let mut e = editor_in(&dir);
    e.auto_pair = false;
    keys(&mut e, "fn SPC f() SPC { RET x;");
    assert_eq!(e.buf().lines, vec!["fn f() {", "    x;"]);
    keys(&mut e, "RET } ");
    assert_eq!(e.buf().lines[2], "    }");
    // cursor between braces splits into three lines
    let mut e = editor_in(&dir);
    e.auto_pair = false;
    keys(&mut e, "{}");
    keys(&mut e, "<left> RET");
    assert_eq!(e.buf().lines, vec!["{", "    ", "}"]);
    assert_eq!(e.buf().cursor, Pos::new(1, 4));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn file_roundtrip_and_comment_toggle() {
    let dir = tmpdir("file");
    std::fs::write(dir.join("a.py"), "x = 1\ny = 2\n").unwrap();
    let mut e = editor_in(&dir);
    e.find_file("a.py");
    assert_eq!(e.buf().hl.name, "Python");
    keys(&mut e, "M-;");
    assert_eq!(e.buf().lines[0], "# x = 1");
    keys(&mut e, "M-;");
    assert_eq!(e.buf().lines[0], "x = 1");
    keys(&mut e, "M-; C-x C-s");
    assert_eq!(std::fs::read_to_string(dir.join("a.py")).unwrap(), "# x = 1\ny = 2\n");
    assert!(!e.buf().modified);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn read_only_special_buffer() {
    let dir = tmpdir("ro");
    let mut e = editor_in(&dir);
    keys(&mut e, "C-h b");
    assert_eq!(e.buf().kind, BufKind::Help);
    let before = e.buf().lines.clone();
    keys(&mut e, "x");
    assert_eq!(e.buf().lines, before);
    keys(&mut e, "q");
    assert_eq!(e.buf().kind, BufKind::File);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn git_workflow_through_the_editor() {
    let dir = tmpdir("git");
    let repo = git2::Repository::init(&dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "T").unwrap();
    cfg.set_str("user.email", "t@e.x").unwrap();
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(dir.join("b.rs"), "fn main() {}\n").unwrap();

    let mut e = editor_in(&dir);
    // status shows both files as untracked; stage a.txt and commit
    keys(&mut e, "C-x g");
    assert_eq!(e.buf().kind, BufKind::GitStatus);
    assert!(e.buf().lines.iter().any(|l| l.contains("a.txt")));
    keys(&mut e, "s s"); // stage first file line, then the next
    assert!(e.buf().lines.iter().any(|l| l.starts_with("Staged changes (2)")), "{:?}", e.buf().lines);
    keys(&mut e, "c first SPC commit RET");
    assert_eq!(repo.head().unwrap().peel_to_commit().unwrap().summary().unwrap(), Some("first commit"));
    assert!(e.buf().lines.iter().any(|l| l.contains("clean")));
    keys(&mut e, "q");

    // edit through the editor: gutter marks + diff + hunk navigation
    e.find_file("a.txt");
    keys(&mut e, "C-n C-e RET three");
    e.refresh_marks();
    assert_eq!(e.buf().git_marks, vec![0, 0, 1]);
    keys(&mut e, "M-< C-x v n");
    assert_eq!(e.buf().cursor.line, 2);
    keys(&mut e, "C-x C-s C-x v =");
    assert_eq!(e.buf().kind, BufKind::GitDiff);
    assert!(e.buf().lines.iter().any(|l| l == "+three"), "{:?}", e.buf().lines);
    keys(&mut e, "q C-x v a C-x v c add SPC three RET");
    assert_eq!(repo.head().unwrap().peel_to_commit().unwrap().summary().unwrap(), Some("add three"));

    // log + show
    keys(&mut e, "C-x v l");
    assert_eq!(e.buf().lines.len(), 2);
    keys(&mut e, "RET");
    assert!(e.buf().lines.iter().any(|l| l == "+three"));
    keys(&mut e, "q q");

    // project find-file / grep
    keys(&mut e, "C-x p f b.rs RET");
    assert_eq!(e.buf().name, "b.rs");
    keys(&mut e, "C-x p g t w o RET");
    assert_eq!(e.buf().kind, BufKind::Grep);
    assert!(e.buf().lines.iter().any(|l| l.starts_with("a.txt:2:")), "{:?}", e.buf().lines);
    keys(&mut e, "M-g n");
    assert_eq!(e.buf().name, "a.txt");
    assert_eq!(e.buf().cursor.line, 1);

    // branch create + switch
    keys(&mut e, "C-x v o -b SPC topic RET");
    assert_eq!(crate::git::branch_name(&repo), "topic");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn restore_file_with_confirmation() {
    let dir = tmpdir("restore");
    let repo = git2::Repository::init(&dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "T").unwrap();
    cfg.set_str("user.email", "t@e.x").unwrap();
    std::fs::write(dir.join("a.txt"), "keep\n").unwrap();
    crate::git::stage(&repo, "a.txt").unwrap();
    crate::git::commit(&repo, "c", false).unwrap();
    let mut e = editor_in(&dir);
    e.find_file("a.txt");
    keys(&mut e, "junk C-x C-s");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "junkkeep\n");
    keys(&mut e, "C-x v r n"); // declined
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "junkkeep\n");
    keys(&mut e, "C-x v r y");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "keep\n");
    assert_eq!(e.buf().lines, vec!["keep"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn window_splitting_and_navigation() {
    let dir = tmpdir("windows");
    let mut e = editor_in(&dir);
    assert_eq!(e.windows.len(), 1);

    // Split horizontally (C-x 2)
    keys(&mut e, "C-x 2");
    assert_eq!(e.windows.len(), 2);

    let w0 = e.windows[0].id;
    let w1 = e.windows[1].id;
    assert_eq!(e.active_win_id, w0);

    // Switch to other window (C-x o)
    keys(&mut e, "C-x o");
    assert_eq!(e.active_win_id, w1);

    // Split vertically (C-x 3)
    keys(&mut e, "C-x 3");
    assert_eq!(e.windows.len(), 3);

    // Delete current window (C-x 0)
    keys(&mut e, "C-x 0");
    assert_eq!(e.windows.len(), 2);

    // Delete all other windows (C-x 1)
    keys(&mut e, "C-x 1");
    assert_eq!(e.windows.len(), 1);
    assert_eq!(e.active_win_id, e.windows[0].id);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_motions_and_editing() {
    let dir = tmpdir("vimmotions");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    assert_eq!(e.vim_mode, VimMode::Normal);

    // Type text in Insert mode
    keys(&mut e, "i hello SPC world RET second SPC line ESC");
    assert_eq!(e.vim_mode, VimMode::Normal);
    assert_eq!(e.buf().lines, vec!["hello world", "second line"]);

    // Move with h, j, k, l, w, b, 0, $
    keys(&mut e, "k 0");
    assert_eq!(e.buf().cursor, Pos::new(0, 0));
    keys(&mut e, "w");
    assert_eq!(e.buf().cursor, Pos::new(0, 6)); // start of 'world'
    keys(&mut e, "b");
    assert_eq!(e.buf().cursor, Pos::new(0, 0)); // back to 'hello'

    // Replace character: rX
    keys(&mut e, "r X");
    assert_eq!(e.buf().lines[0], "Xello world");

    // Delete character: x
    keys(&mut e, "x");
    assert_eq!(e.buf().lines[0], "ello world");

    // Undo: u
    keys(&mut e, "u");
    assert_eq!(e.buf().lines[0], "Xello world");

    // Delete word: dw
    keys(&mut e, "d w");
    assert_eq!(e.buf().lines[0], "world");

    // Delete line: dd
    keys(&mut e, "d d");
    assert_eq!(e.buf().lines, vec!["second line"]);

    // Open line below: o
    keys(&mut e, "o third ESC");
    assert_eq!(e.buf().lines, vec!["second line", "third"]);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_autopairing() {
    let dir = tmpdir("vimautopair");
    let mut e = Editor::new();
    e.cwd = dir.clone();

    // Type '(' in insert mode -> inserts '()' with cursor between
    keys(&mut e, "i (");
    assert_eq!(e.buf().lines[0], "()");
    assert_eq!(e.buf().cursor, Pos::new(0, 1));

    // Typing DEL deletes both delimiters
    keys(&mut e, "DEL");
    assert_eq!(e.buf().lines[0], "");

    // Test braces and brackets
    keys(&mut e, "{");
    assert_eq!(e.buf().lines[0], "{}");
    keys(&mut e, "DEL [");
    assert_eq!(e.buf().lines[0], "[]");

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_leader_keybindings() {
    let dir = tmpdir("vimleader");
    let test_file = dir.join("test.txt");
    std::fs::write(&test_file, "hello\n").unwrap();

    let mut e = Editor::new();
    e.cwd = dir.clone();
    e.open_path(&test_file).unwrap();

    // Split vertically via <leader>wv (SPC w v)
    keys(&mut e, "SPC w v");
    assert_eq!(e.windows.len(), 2);

    // Split horizontally via <leader>ws (SPC w s)
    keys(&mut e, "SPC w s");
    assert_eq!(e.windows.len(), 3);

    // Other window via <leader>wh (SPC w h)
    let w0 = e.active_win_id;
    keys(&mut e, "SPC w h");
    assert_ne!(e.active_win_id, w0);

    // Save via <leader>ww (SPC w w)
    keys(&mut e, "SPC w w");

    // Close window via <leader>wq (SPC w q) closes current window, keeping other windows and not killing the buffer
    let buf_count_before = e.bufs.len();
    keys(&mut e, "SPC w q");
    assert_eq!(e.windows.len(), 2);
    assert_eq!(e.bufs.len(), buf_count_before);
    assert!(!e.quit);

    // Close another window
    keys(&mut e, "SPC w q");
    assert_eq!(e.windows.len(), 1);
    assert!(!e.quit);

    // Closing the last remaining window quits the editor
    keys(&mut e, "SPC w q");
    assert!(e.quit);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_visual_mode_and_move_lines() {
    let dir = tmpdir("vimvisual");
    let mut e = Editor::new();
    e.cwd = dir.clone();

    keys(&mut e, "i line1 RET line2 RET line3 ESC");
    assert_eq!(e.buf().lines, vec!["line1", "line2", "line3"]);

    // Move to line2
    keys(&mut e, "k 0");
    assert_eq!(e.buf().cursor.line, 1);

    // Select line2 in visual mode
    keys(&mut e, "V");
    assert_eq!(e.vim_mode, VimMode::VisualLine);

    // Move selected line up with K
    keys(&mut e, "K");
    assert_eq!(e.buf().lines, vec!["line2", "line1", "line3"]);

    // Move selected line down with J
    keys(&mut e, "J");
    assert_eq!(e.buf().lines, vec!["line1", "line2", "line3"]);

    // Delete selection with d
    keys(&mut e, "d");
    assert_eq!(e.buf().lines, vec!["line1", "line3"]);
    assert_eq!(e.vim_mode, VimMode::Normal);

    std::fs::remove_dir_all(dir).ok();
}


#[test]
fn vim_counts_operators_and_text_objects() {
    let dir = tmpdir("vim-operators");
    let mut e = editor_in(&dir);
    keys(&mut e, "one two three four");
    keys(&mut e, "ESC");
    keys(&mut e, "0 2dw");
    assert_eq!(e.buf().lines[0], "three four");

    // Inner-word change and insertion preserve the surrounding whitespace.
    keys(&mut e, "0ciw");
    keys(&mut e, "ONE ESC");
    assert_eq!(e.buf().lines[0], "ONE four");
}

#[test]
fn vim_linewise_count_and_gg_g() {
    let dir = tmpdir("vim-lines");
    let mut e = editor_in(&dir);
    keys(&mut e, "one RET two RET three RET four");
    keys(&mut e, "ESC");
    keys(&mut e, "2gg");
    assert_eq!(e.buf().cursor.line, 1);
    keys(&mut e, "2dd");
    assert_eq!(e.buf().lines, vec!["one", "four"]);
    keys(&mut e, "G");
    assert_eq!(e.buf().cursor.line, 1);
}
