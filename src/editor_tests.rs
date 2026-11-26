//! End-to-end tests: feed key events into the Editor like a terminal would.
use crate::buffer::*;
use crate::editor::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
        "S-TAB" => KeyCode::BackTab,
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
                "RET" | "DEL" | "TAB" | "S-TAB" | "SPC" | "ESC" | "<left>" | "<right>" | "<up>" | "<down>"
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
fn insert_completion_uses_project_source_files() {
    let dir = tmpdir("project-completion");
    let current = dir.join("main.rs");
    std::fs::write(&current, "").unwrap();
    std::fs::write(
        dir.join("helpers.rs"),
        "fn project_context_helper() {}\n",
    )
    .unwrap();

    let mut e = editor_in(&dir);
    let index = e.open_path(&current).unwrap();
    e.switch_to(index);
    keys(&mut e, "project_con");

    assert!(e.completion.items.contains(&"project_context_helper".to_string()));
    keys(&mut e, "RET");
    assert_eq!(e.buf().lines, vec!["project_con", ""]);
}

#[test]
fn python_completion_includes_editor_builtins_without_file_context() {
    let dir = tmpdir("python-builtins");
    let current = dir.join("main.py");
    std::fs::write(&current, "").unwrap();

    let mut e = editor_in(&dir);
    let index = e.open_path(&current).unwrap();
    e.switch_to(index);
    keys(&mut e, "pri");

    assert!(e.completion.items.contains(&"print".to_string()));
}

#[test]
fn insert_completion_enter_inserts_newline_without_accepting() {
    let dir = tmpdir("completion-enter-newline");
    let mut e = editor_in(&dir);
    e.buf_mut().kind = BufKind::File;
    keys(&mut e, "pri");
    assert!(!e.completion.items.is_empty());

    keys(&mut e, "RET");

    assert_eq!(e.buf().lines, vec!["pri", ""]);
    assert_eq!(e.buf().cursor, Pos::new(1, 0));
}

#[test]
fn completion_uses_javascript_typescript_c_and_generic_words() {
    let dir = tmpdir("language-builtins");
    let mut e = editor_in(&dir);
    for (filename, prefix, expected) in [
        ("main.js", "con", "console"),
        ("main.ts", "func", "function"),
        ("main.c", "pri", "printf"),
        ("Main.java", "fun", "function"),
    ] {
        let path = dir.join(filename);
        std::fs::write(&path, "").unwrap();
        let index = e.open_path(&path).unwrap();
        e.switch_to(index);
        keys(&mut e, prefix);

        assert!(
            e.completion.items.iter().any(|item| item == expected),
            "{expected} missing for {filename}: {:?}",
            e.completion.items
        );
    }
}

#[test]
fn insert_completion_can_choose_another_local_suggestion() {
    let dir = tmpdir("local-completion");
    let mut e = editor_in(&dir);
    e.buf_mut().kind = BufKind::File;
    e.buf_mut().lines = vec![
        "project_context_candy project_context_donut".to_string(),
        String::new(),
    ];
    e.buf_mut().cursor = Pos::new(1, 0);
    keys(&mut e, "project_con");
    keys(&mut e, "TAB");
    assert_eq!(e.buf().lines[1], "project_context_candy");
    keys(&mut e, "TAB");
    assert_eq!(e.buf().lines[1], "project_context_donut");
    keys(&mut e, "S-TAB");
    assert_eq!(e.buf().lines[1], "project_context_candy");
    keys(&mut e, "RET");

    assert_eq!(e.buf().lines[1], "project_context_candy");
    assert_eq!(e.buf().lines[2], "");
}

#[test]
fn insert_completion_is_cancelled_by_escape() {
    let dir = tmpdir("cancel-completion");
    let mut e = editor_in(&dir);
    e.buf_mut().kind = BufKind::File;
    e.buf_mut().lines = vec!["contextual_name".to_string(), "con".to_string()];
    e.buf_mut().cursor = Pos::new(1, 3);
    keys(&mut e, "t");
    assert!(!e.completion.items.is_empty());
    keys(&mut e, "TAB");
    let preview = e.buf().lines[1].clone();
    assert_ne!(preview, "cont");
    keys(&mut e, "ESC");

    assert!(e.completion.items.is_empty());
    assert_eq!(e.vim_mode, VimMode::Normal);
    assert_eq!(e.buf().lines[1], "cont");
}

#[test]
fn insert_tab_still_indents_without_completion() {
    let dir = tmpdir("tab-indent");
    let mut e = editor_in(&dir);
    e.buf_mut().kind = BufKind::File;
    keys(&mut e, "TAB");

    assert_eq!(e.buf().lines[0], "    ");
}

#[test]
fn leader_number_jumps_to_corresponding_quick_file() {
    let dir = tmpdir("quick-bookmark");
    let first = dir.join("first.rs");
    let second = dir.join("second.rs");
    std::fs::write(&first, "").unwrap();
    std::fs::write(&second, "").unwrap();

    let mut e = editor_in(&dir);
    e.vim_mode = VimMode::Normal;
    let first_idx = e.open_path(&first).unwrap();
    e.open_path(&second).unwrap();
    e.bookmarks = vec![first.clone()];
    e.quick_files = vec![first, second.clone()];
    e.switch_to(first_idx);

    keys(&mut e, "SPC 2");

    assert_eq!(e.buf().path.as_deref(), Some(second.as_path()));
    assert!(e.vim_pending.is_empty());

    keys(&mut e, "SPC A");
    assert!(matches!(
        &e.mode,
        Mode::Prompt(prompt) if matches!(&prompt.kind, PKind::QuickFiles)
    ));
}

#[test]
fn dd_removes_the_entire_last_line() {
    let dir = tmpdir("dd-last-line");
    let mut e = editor_in(&dir);
    e.buf_mut().set_text("first\nlast");
    e.vim_mode = VimMode::Normal;
    e.buf_mut().cursor = Pos::new(1, 0);

    keys(&mut e, "d d");

    assert_eq!(e.buf().lines, vec!["first"]);
    assert_eq!(e.buf().cursor, Pos::new(0, 0));

    e.buf_mut().set_text("only");
    e.vim_mode = VimMode::Normal;
    keys(&mut e, "d d");
    assert_eq!(e.buf().lines, vec![""]);
}

#[test]
fn ctrl_e_opens_switch_buffer_prompt() {
    let mut e = Editor::new();

    keys(&mut e, "C-e");

    assert!(matches!(
        &e.mode,
        Mode::Prompt(prompt) if matches!(&prompt.kind, PKind::Switch)
    ));
}

#[test]
fn selecting_home_directory_updates_spc_x_search_root() {
    let initial = tmpdir("home-root-initial");
    let selected = tmpdir("home-root-selected");
    let initial_child = initial.join("initial-child");
    let selected_child = selected.join("selected-child");
    let nested_child = selected_child.join("nested-child");
    let hidden_child = selected.join(".hidden");
    let config_dir = selected.join(".config");
    let config_hidden_child = config_dir.join(".hidden-config");
    std::fs::create_dir(&initial_child).unwrap();
    std::fs::create_dir(&selected_child).unwrap();
    std::fs::create_dir(&nested_child).unwrap();
    std::fs::create_dir(&hidden_child).unwrap();
    std::fs::create_dir(&config_dir).unwrap();
    std::fs::create_dir(&config_hidden_child).unwrap();

    let mut e = editor_in(&initial);
    e.start_prompt(Prompt::new(
        "Select home directory: ",
        PKind::WorkspaceDirectory,
        Some(vec![selected.display().to_string()]),
    ));
    keys(&mut e, "RET");

    assert_eq!(e.cwd, selected);
    assert!(matches!(e.buf().kind, BufKind::Dired));
    assert_eq!(e.proj_root, None);

    e.vim_mode = VimMode::Normal;
    keys(&mut e, "SPC x");
    let target_index = {
        let Mode::Prompt(prompt) = &e.mode else {
            panic!("SPC x should open the directory picker");
        };
        let choices = prompt.choices.as_ref().unwrap();
        assert!(choices.contains(&selected_child.display().to_string()));
        assert!(choices.contains(&config_dir.display().to_string()));
        assert!(!choices.contains(&initial_child.display().to_string()));
        assert!(!choices.contains(&hidden_child.display().to_string()));
        assert!(!choices.contains(&config_hidden_child.display().to_string()));
        prompt
            .matches
            .iter()
            .position(|choice| choice == &selected_child.display().to_string())
            .unwrap()
    };
    for _ in 0..target_index {
        keys(&mut e, "TAB");
    }
    keys(&mut e, "RET");
    assert_eq!(e.cwd, selected);
    assert!(matches!(e.buf().kind, BufKind::Dired));

    keys(&mut e, "SPC x");
    let Mode::Prompt(prompt) = &e.mode else {
        panic!("SPC x should continue using the selected workspace root");
    };
    let choices = prompt.choices.as_ref().unwrap();
    assert!(choices.contains(&config_dir.display().to_string()));
    assert!(choices.contains(&nested_child.display().to_string()));
    assert!(!choices.contains(&initial_child.display().to_string()));

    std::fs::remove_dir_all(initial).ok();
    std::fs::remove_dir_all(selected).ok();
}

#[test]
fn leader_search_keeps_match_highlight_while_navigating() {
    let mut e = Editor::new();
    e.buf_mut().set_text("word one word two");

    keys(&mut e, "SPC s w o r d RET");
    assert_eq!(
        e.search_highlight,
        Some((0, Pos::new(0, 0), Pos::new(0, 4)))
    );

    keys(&mut e, "n");
    assert_eq!(
        e.search_highlight,
        Some((0, Pos::new(0, 9), Pos::new(0, 13)))
    );
}

#[test]
fn leader_search_escape_clears_match_highlight() {
    let mut e = Editor::new();
    e.buf_mut().set_text("word one");

    keys(&mut e, "SPC s w o r d ESC");

    assert_eq!(e.search_highlight, None);
    assert_eq!(e.buf().cursor, Pos::new(0, 4));
}

#[test]
fn meta_q_quits_like_c_x_c_c_and_prompts_for_modified_files() {
    let mut plain = Editor::new();
    keys(&mut plain, "C-x C-c");
    assert!(plain.quit);

    let dir = tmpdir("meta-q");
    let path = dir.join("modified.txt");
    std::fs::write(&path, "text\n").unwrap();
    let mut e = Editor::new();
    let index = e.open_path(&path).unwrap();
    e.switch_to(index);
    e.buf_mut().modified = true;

    keys(&mut e, "M-q");
    assert!(matches!(e.mode, Mode::YesNo(_)));
    assert!(!e.quit);
    keys(&mut e, "y");
    assert!(e.quit);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn reloads_open_buffer_after_external_file_change() {
    let dir = tmpdir("external-change");
    let path = dir.join("external.txt");
    std::fs::write(&path, "before\n").unwrap();

    let mut e = editor_in(&dir);
    let index = e.open_path(&path).unwrap();
    e.switch_to(index);
    std::fs::write(&path, "updated externally\n").unwrap();
    e.disk_stamp = Instant::now() - Duration::from_secs(1);

    assert!(e.tick());
    assert_eq!(e.buf().text(), "updated externally\n");
    assert!(!e.buf().modified);

    std::fs::remove_dir_all(dir).ok();
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
    keys(&mut e, "C-n M-f RET three");
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
    assert!(matches!(
        e.buf().meta.get(2),
        Some(LineMeta::Grep { match_start: 0, match_end: 3, .. })
    ));
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
fn leader_left_and_right_resize_vertical_split_in_matching_direction() {
    let mut e = Editor::new();
    e.vim_mode = VimMode::Normal;
    e.split_window_right();

    let ratio = |tree: &WindowTree| match tree {
        WindowTree::Split {
            dir: SplitDir::Vertical,
            ratio,
            ..
        } => *ratio,
        _ => panic!("expected a vertical split"),
    };

    let initial = ratio(&e.win_tree);
    keys(&mut e, "SPC <right>");
    assert!(ratio(&e.win_tree) < initial);
    keys(&mut e, "SPC <left>");
    assert!((ratio(&e.win_tree) - initial).abs() < f32::EPSILON);
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
fn leader_window_move_swaps_the_current_pane_position() {
    let dir = tmpdir("window-move");
    let left_file = dir.join("left.txt");
    let right_file = dir.join("right.txt");
    std::fs::write(&left_file, "left\n").unwrap();
    std::fs::write(&right_file, "right\n").unwrap();

    let mut vertical = editor_in(&dir);
    vertical.vim_mode = VimMode::Normal;
    let left_buffer = vertical.open_path(&left_file).unwrap();
    vertical.switch_to(left_buffer);
    vertical.split_window_right();
    let moving_id = vertical.active_win_id;
    let other_id = vertical.windows.iter().find(|window| window.id != moving_id).unwrap().id;
    let right_buffer = vertical.open_path(&right_file).unwrap();
    vertical.switch_to(right_buffer);

    assert_eq!(vertical.win_tree.leaves(), vec![moving_id, other_id]);
    keys(&mut vertical, "SPC w c l");
    assert_eq!(vertical.win_tree.leaves(), vec![other_id, moving_id]);
    assert_eq!(vertical.active_win_id, moving_id);
    assert_eq!(
        vertical.windows.iter().find(|window| window.id == moving_id).unwrap().buf_idx,
        right_buffer
    );
    keys(&mut vertical, "SPC w c h");
    assert_eq!(vertical.win_tree.leaves(), vec![moving_id, other_id]);
    keys(&mut vertical, "SPC w c h");
    assert_eq!(vertical.win_tree.leaves(), vec![moving_id, other_id]);

    let mut horizontal = editor_in(&dir);
    horizontal.vim_mode = VimMode::Normal;
    let buffer = horizontal.open_path(&left_file).unwrap();
    horizontal.switch_to(buffer);
    horizontal.split_window_below();
    let top_id = horizontal.active_win_id;
    let bottom_id = horizontal.windows.iter().find(|window| window.id != top_id).unwrap().id;
    keys(&mut horizontal, "SPC w c j");
    assert_eq!(horizontal.win_tree.leaves(), vec![bottom_id, top_id]);
    keys(&mut horizontal, "SPC w c k");
    assert_eq!(horizontal.win_tree.leaves(), vec![top_id, bottom_id]);
    keys(&mut horizontal, "SPC w c k");
    assert_eq!(horizontal.win_tree.leaves(), vec![top_id, bottom_id]);

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
    keys(&mut e, "one SPC two SPC three SPC four");
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


#[test]
fn vim_repeat_last_insert_replays_inserted_text() {
    let dir = tmpdir("vim-dot-repeat");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    keys(&mut e, "iabc ESC");
    keys(&mut e, "0 iX ESC .");
    assert_eq!(e.buf().lines[0], "XXabc");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_visual_block_deletes_rectangle() {
    let dir = tmpdir("vim-block");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    keys(&mut e, "iabc RET def RET ghi ESC gg 0 C-v j l d");
    assert_eq!(e.buf().lines, vec!["c", "f", "ghi"]);
    assert_eq!(e.vim_mode, VimMode::Normal);
    std::fs::remove_dir_all(dir).ok();
}


#[test]
fn vim_ctrl_n_inserts_at_all_word_occurrences() {
    let dir = tmpdir("vim-multicursor");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    keys(&mut e, "ifoo SPC foo SPC foo ESC gg 0 C-n X ESC");
    assert_eq!(e.buf().lines[0], "Xfoo Xfoo Xfoo");
    assert_eq!(e.vim_mode, VimMode::Normal);
    std::fs::remove_dir_all(dir).ok();
}


#[test]
fn vim_insert_session_is_one_undo_step() {
    let dir = tmpdir("vim-insert-undo");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    keys(&mut e, "ihello world ESC u");
    assert_eq!(e.buf().lines, vec![""]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_multiline_delimiter_text_object() {
    let dir = tmpdir("vim-multiline-object");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    e.buf_mut().set_text("before (\ninside\n)\nafter");
    e.buf_mut().cursor = Pos::new(1, 1);
    keys(&mut e, "d i (");
    assert_eq!(e.buf().lines, vec!["before ()", "after"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_resolves_conflict_keeping_ours() {
    let dir = tmpdir("vim-conflict");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    e.buf_mut().set_text("start\n<<<<<<< ours\nlocal\n=======\nremote\n>>>>>>> theirs\nend");
    e.buf_mut().cursor = Pos::new(2, 0);
    keys(&mut e, "SPC g C");
    assert_eq!(e.buf().lines, vec!["start", "local", "end"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_dot_repeats_open_line_change() {
    let dir = tmpdir("vim-dot-open-line");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    keys(&mut e, "iabc ESC oX ESC .");
    assert_eq!(e.buf().lines, vec!["abc", "X", "X"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_change_operator_and_insert_undo_together() {
    let dir = tmpdir("vim-change-undo");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    keys(&mut e, "ihello SPC world ESC 0 c w X ESC u");
    assert_eq!(e.buf().lines, vec!["hello world"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_leader_f_matches_project_find_file() {
    let dir = tmpdir("vimspcf");
    std::fs::write(dir.join("alpha.txt"), "a\n").unwrap();
    std::fs::write(dir.join("beta.rs"), "b\n").unwrap();

    // SPC f in Vim normal mode opens the same prompt as C-x p f.
    let mut e = Editor::new();
    e.cwd = dir.clone();
    e.open_path(&dir.join("alpha.txt")).unwrap();
    e.vim_mode = VimMode::Normal;
    keys(&mut e, "SPC f");
    assert!(matches!(e.mode, Mode::Prompt(_)), "SPC f did not open the prompt");
    keys(&mut e, "b e t a RET");
    assert_eq!(e.buf().name, "beta.rs");

    // Other leader chords whose last key collides with a Vim command (SPC b m vs `m`).
    e.vim_mode = VimMode::Normal;
    keys(&mut e, "SPC b m");
    assert!(e.vim_pending.is_empty(), "pending: {:?}", e.vim_pending);
}

#[test]
fn vim_normal_enter_and_backspace_are_motions() {
    let dir = tmpdir("vim-normal-special-keys");
    let mut e = editor_in(&dir);
    e.buf_mut().set_text("first\nsecond\nthird");
    e.vim_mode = VimMode::Normal;
    e.buf_mut().cursor = Pos::new(0, 1);

    keys(&mut e, "RET");
    assert_eq!(e.buf().cursor, Pos::new(1, 1));
    keys(&mut e, "DEL");
    assert_eq!(e.buf().cursor, Pos::new(1, 0));
    assert_eq!(e.buf().lines, vec!["first", "second", "third"]);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn dired_allows_vim_leader_and_meta_commands() {
    let dir = tmpdir("dired-leader-meta");
    let mut e = Editor::new();
    e.cwd = dir.clone();
    e.open_dir(&dir);
    e.vim_mode = VimMode::Normal;

    keys(&mut e, "SPC e");
    assert_eq!(e.buf().kind, BufKind::Dired);
    assert!(e.vim_pending.is_empty());

    keys(&mut e, "M-x");
    assert!(matches!(e.mode, Mode::Prompt(_)));
    e.mode = Mode::Normal;
    keys(&mut e, ":");
    assert!(matches!(
        &e.mode,
        Mode::Prompt(prompt) if matches!(&prompt.kind, PKind::Command)
    ));
    keys(&mut e, "ESC");
    keys(&mut e, "C-e");
    assert!(matches!(
        &e.mode,
        Mode::Prompt(prompt) if matches!(&prompt.kind, PKind::Switch)
    ));
    e.mode = Mode::Normal;
    keys(&mut e, "C-x 2");
    assert_eq!(e.windows.len(), 2);
    keys(&mut e, "C-h b");
    assert_eq!(e.buf().kind, BufKind::Help);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn extended_command_suggestions_hide_motion_and_internal_commands() {
    let dir = tmpdir("m-x-command-list");
    let mut e = editor_in(&dir);
    e.vim_mode = VimMode::Normal;
    e.command_history = vec![
        "forward-char".into(),
        "forward-paragraph".into(),
        "git-status".into(),
        "not-a-command".into(),
    ];

    keys(&mut e, "M-x");

    let Mode::Prompt(prompt) = &e.mode else {
        panic!("M-x should open the command prompt");
    };
    let choices = prompt.choices.as_ref().unwrap();
    assert!(choices.contains(&"git-status".to_string()));
    assert!(choices.contains(&"run".to_string()));
    assert!(choices.contains(&"save-buffer".to_string()));
    assert!(!choices.contains(&"forward-char".to_string()));
    assert!(!choices.contains(&"forward-paragraph".to_string()));
    assert!(!choices.contains(&"self-insert-command".to_string()));
    assert!(!choices.contains(&"not-a-command".to_string()));

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn run_command_streams_output_and_opens_results() {
    let dir = tmpdir("run-command");
    let source = dir.join("source.rs");
    std::fs::write(&source, "let value = 1;\n").unwrap();
    let mut e = editor_in(&dir);
    let source_buffer = e.open_path(&source).unwrap();
    e.switch_to(source_buffer);

    e.run("run");
    keys(&mut e, "C-u echo SPC compiling; SPC sleep SPC 0.4; SPC echo SPC source.rs:1: SPC error RET");
    assert_eq!(e.windows.len(), 2);
    assert_eq!(e.bufs[e.windows[0].buf_idx].path.as_deref(), Some(source.as_path()));
    assert_eq!(e.buf().kind, BufKind::Run);

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_elapsed_time = false;
    let mut saw_live_output = false;
    while !e.buf().lines.iter().any(|line| line.starts_with("Command finished in "))
        && Instant::now() < deadline
    {
        e.tick();
        saw_elapsed_time |= e.buf().lines.get(2).is_some_and(|line| {
            line.starts_with("Command running... (") && !line.ends_with("(0.0s)")
        });
        saw_live_output |= e.buf().lines.iter().any(|line| line == "compiling")
            && e.buf().lines.get(2).is_some_and(|line| line.starts_with("Command running..."));
        std::thread::sleep(Duration::from_millis(5));
    }

    assert_eq!(e.buf().kind, BufKind::Run);
    assert!(e
        .buf()
        .lines
        .iter()
        .any(|line| line == "$ echo compiling; sleep 0.4; echo source.rs:1: error"));
    assert!(e.buf().lines.iter().any(|line| line == "compiling"));
    assert!(e.buf().lines.iter().any(|line| line == "Process exited with code 0"));
    assert!(e.buf().lines.iter().any(|line| line.starts_with("Command finished in ")));
    assert!(!e.buf().lines.iter().any(|line| line.starts_with("Elapsed:")));
    assert!(saw_elapsed_time);
    assert!(saw_live_output);
    assert!(e.buf().meta.iter().any(|meta| matches!(meta, LineMeta::Grep { .. })));
    let compilation_buffer = e.cur;
    e.vim_mode = VimMode::Normal;
    keys(&mut e, "q");
    assert_eq!(e.windows.len(), 1);
    assert_eq!(e.buf().path.as_deref(), Some(source.as_path()));

    e.split_window_below();
    e.set_active_window(e.next_win_id - 1);
    e.switch_to(compilation_buffer);
    keys(&mut e, "SPC w q");
    assert_eq!(e.windows.len(), 1);
    assert_eq!(e.buf().path.as_deref(), Some(source.as_path()));

    e.run("next-error");
    assert_eq!(e.buf().path.as_deref(), Some(source.as_path()));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn run_command_can_be_stopped_with_control_c_twice() {
    let dir = tmpdir("stop-run-command");
    let mut e = editor_in(&dir);

    e.run("run");
    keys(&mut e, "C-u sleep SPC 10 SPC & SPC echo SPC started; SPC wait RET");
    assert_eq!(e.buf().kind, BufKind::Run);
    let start_deadline = Instant::now() + Duration::from_secs(3);
    while !e.buf().lines.iter().any(|line| line == "started")
        && Instant::now() < start_deadline
    {
        e.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(e.buf().lines.iter().any(|line| line == "started"));
    keys(&mut e, "C-c C-c");

    let deadline = Instant::now() + Duration::from_secs(3);
    while !e.buf().lines.iter().any(|line| line == "Process stopped by user")
        && Instant::now() < deadline
    {
        e.tick();
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(e.buf().lines.iter().any(|line| line == "Process stopped by user"));
    assert!(e.buf().lines.iter().any(|line| line.starts_with("Command finished in ")));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn leader_z_opens_project_tree_and_each_file_in_a_new_window() {
    let dir = tmpdir("project-tree");
    let nested = dir.join("src");
    std::fs::create_dir(&nested).unwrap();
    let first = nested.join("first.rs");
    let second = nested.join("second.rs");
    std::fs::write(&first, "fn first() {}\n").unwrap();
    std::fs::write(&second, "fn second() {}\n").unwrap();

    let mut e = editor_in(&dir);
    e.vim_mode = VimMode::Normal;
    keys(&mut e, "SPC z");
    assert_eq!(e.windows.len(), 1);
    assert!(e.project_tree_open);
    assert!(e.project_tree_focused);
    assert_eq!(e.buf().name, "*scratch*");
    assert_eq!(e.project_tree_root().as_deref(), Some(dir.as_path()));

    keys(&mut e, "% new-root.txt RET");
    assert!(nested.join("new-root.txt").is_file());

    let src_line = e
        .project_tree
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.path == nested)
        .unwrap();
    e.project_tree.as_mut().unwrap().cursor = src_line;
    keys(&mut e, "RET");
    assert!(e.project_tree.as_ref().unwrap().entries.iter().any(|entry| entry.path == first));
    assert!(e.project_tree.as_ref().unwrap().entries.iter().any(|entry| entry.path == second));

    for path in [&first, &second] {
        let line = e
            .project_tree
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .position(|entry| entry.path == *path)
            .unwrap();
        e.project_tree.as_mut().unwrap().cursor = line;
        e.project_tree_focused = true;
        keys(&mut e, "RET");
        assert_eq!(e.buf().path.as_deref(), Some(path.as_path()));
        assert_eq!(e.windows.len(), if path == &first { 2 } else { 3 });
    }

    keys(&mut e, "SPC z");
    assert!(!e.project_tree_open);
    assert_eq!(e.windows.len(), 3);
    assert_eq!(e.buf().path.as_deref(), Some(second.as_path()));

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn leader_and_ctrl_x_shortcuts_work_while_project_tree_is_focused() {
    let dir = tmpdir("project-tree-shortcuts");
    std::fs::write(dir.join("file.txt"), "contents\n").unwrap();
    std::fs::create_dir(dir.join("child")).unwrap();

    let mut e = editor_in(&dir);
    e.vim_mode = VimMode::Normal;
    keys(&mut e, "SPC z");
    assert!(e.project_tree_focused);

    keys(&mut e, "SPC x");
    assert!(matches!(e.mode, Mode::Prompt(_)), "SPC x was handled as a tree command");
    keys(&mut e, "ESC");
    assert!(e.project_tree_focused);

    keys(&mut e, "C-x C-f");
    assert!(matches!(e.mode, Mode::Prompt(_)), "C-x C-f did not open the file prompt");

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn leader_window_navigation_moves_between_project_tree_and_editor() {
    let dir = tmpdir("project-tree-window-navigation");
    let mut e = editor_in(&dir);
    e.vim_mode = VimMode::Normal;
    keys(&mut e, "SPC z");
    assert!(e.project_tree_focused);

    let active_window = e.active_win_id;
    keys(&mut e, "SPC w l");
    assert!(!e.project_tree_focused);
    assert_eq!(e.active_win_id, active_window);

    keys(&mut e, "SPC w h");
    assert!(e.project_tree_focused);
    assert_eq!(e.active_win_id, active_window);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn escape_cancels_pending_key_sequences() {
    let dir = tmpdir("escape-cancel-sequences");
    let mut e = editor_in(&dir);
    e.vim_mode = VimMode::Normal;

    keys(&mut e, "SPC");
    assert_eq!(e.vim_pending, "SPC");
    keys(&mut e, "ESC");
    assert!(e.vim_pending.is_empty());
    assert!(!e.meta_pending);

    keys(&mut e, "M-g");
    assert_eq!(e.pending, vec!["M-g"]);
    keys(&mut e, "ESC");
    assert!(e.pending.is_empty());
    assert!(!e.meta_pending);
    assert_eq!(e.msg, "Quit");

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_equal_operator_reindents_lines() {
    let dir = tmpdir("vim-equal-indent");
    let mut e = editor_in(&dir);
    e.buf_mut().set_text("fn main() {\nwrong\n}\n");
    e.vim_mode = VimMode::Normal;
    e.buf_mut().cursor = Pos::new(1, 0);

    keys(&mut e, "==");
    assert_eq!(e.buf().lines, vec!["fn main() {", "    wrong", "}"]);

    e.buf_mut().set_text("fn main() {\nwrong\n}\n");
    e.buf_mut().cursor = Pos::new(1, 0);
    keys(&mut e, "=j");
    assert_eq!(e.buf().lines, vec!["fn main() {", "    wrong", "}"]);

    e.buf_mut().set_text("fn main() {\nwrong\n}\n");
    e.vim_mode = VimMode::VisualLine;
    e.buf_mut().mark = Some(Pos::new(0, 0));
    e.buf_mut().mark_active = true;
    e.buf_mut().cursor = Pos::new(1, 0);
    keys(&mut e, "=");
    assert_eq!(e.buf().lines, vec!["fn main() {", "    wrong", "}"]);
    assert_eq!(e.vim_mode, VimMode::Normal);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn saving_does_not_reindent_the_buffer() {
    let dir = tmpdir("save-no-reindent");
    let path = dir.join("sample.rs");
    std::fs::write(&path, "").unwrap();
    let mut e = editor_in(&dir);
    e.open_path(&path).unwrap();
    e.buf_mut().set_text("fn main() {\nwrong\n}\n");

    e.save_current(None);

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "fn main() {\nwrong\n}\n"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn vim_visual_indent_keeps_the_selection() {
    let dir = tmpdir("vim-visual-indent");
    let mut e = editor_in(&dir);
    e.buf_mut().set_text("alpha\nbeta\ngamma");
    e.vim_mode = VimMode::Visual;
    e.buf_mut().mark = Some(Pos::new(0, 0));
    e.buf_mut().mark_active = true;
    e.buf_mut().cursor = Pos::new(1, 1);

    keys(&mut e, ">");
    assert_eq!(e.buf().lines, vec!["    alpha", "    beta", "gamma"]);
    assert_eq!(e.vim_mode, VimMode::Visual);
    assert!(e.buf().mark_active);

    keys(&mut e, "<");
    assert_eq!(e.buf().lines, vec!["alpha", "beta", "gamma"]);
    assert_eq!(e.vim_mode, VimMode::Visual);
    assert!(e.buf().mark_active);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn leader_lw_toggles_line_wrap() {
    let dir = tmpdir("leader-line-wrap");
    let mut e = editor_in(&dir);
    e.vim_mode = VimMode::Normal;

    keys(&mut e, "SPC l w");
    assert!(e.wrap);
    keys(&mut e, "SPC l w");
    assert!(!e.wrap);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn wrap_segments_split_display_columns() {
    assert_eq!(crate::editor::wrap_segments("abcdefgh", 8, 4), vec![(0, 4), (4, 8), (8, 8)]);
    assert_eq!(crate::editor::wrap_segments("ab\tc", 4, 4), vec![(0, 4), (4, 5)]);
}

#[test]
fn wrapped_scroll_keeps_cursor_visible() {
    let dir = tmpdir("wrapped-scroll");
    let mut e = editor_in(&dir);
    e.buf_mut().set_text("abcdefghijklmnopqrst");
    e.buf_mut().cursor = Pos::new(0, 19);
    e.wrap = true;
    e.text_w = 6;
    e.text_h = 2;

    e.fix_scroll();

    assert_eq!(e.buf().left, 12);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn mouse_hover_does_not_change_window_focus() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut e = Editor::new();
    e.screen_w = 80;
    e.screen_h = 24;
    e.split_window_right();
    let other_id = e.windows.iter().find(|w| w.id != e.active_win_id).unwrap().id;

    e.on_mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 60,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });
    assert_ne!(e.active_win_id, other_id);

    e.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 60,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(e.active_win_id, other_id);
}

#[test]
fn mouse_scroll_does_not_change_focus_and_scrolls_hovered_window() {
    use ratatui::crossterm::event::{MouseEvent, MouseEventKind};

    let mut e = Editor::new();
    e.screen_w = 80;
    e.screen_h = 24;
    e.buf_mut().set_text(&(0..100).map(|line| line.to_string()).collect::<Vec<_>>().join("\n"));
    e.split_window_right();
    let active_id = e.active_win_id;
    let other_id = e.windows.iter().find(|window| window.id != active_id).unwrap().id;

    e.on_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 60,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(e.active_win_id, active_id);
    assert!(e.windows.iter().find(|window| window.id == other_id).unwrap().top > 0);
}

#[test]
fn mouse_selects_and_opens_grep_results() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let dir = tmpdir("mouse-grep-result");
    std::fs::write(dir.join("a.txt"), "skip\nneedle here\n").unwrap();
    let mut e = editor_in(&dir);
    e.screen_w = 80;
    e.screen_h = 24;
    keys(&mut e, "C-x p g needle RET");
    assert_eq!(e.buf().kind, BufKind::Grep);
    let result_line = e.buf().meta.iter().position(|meta| matches!(meta, LineMeta::Grep { .. })).unwrap();

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: result_line as u16,
        modifiers: KeyModifiers::NONE,
    };
    e.on_mouse(click);
    assert_eq!(e.buf().cursor.line, result_line);
    assert_eq!(e.buf().kind, BufKind::Grep);

    e.on_mouse(click);
    assert_eq!(e.buf().path.as_deref(), Some(dir.join("a.txt").as_path()));
    assert_eq!(e.buf().cursor.line, 1);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn dragging_window_dividers_resizes_splits() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut vertical = Editor::new();
    vertical.screen_w = 80;
    vertical.screen_h = 24;
    vertical.split_window_right();
    let WindowTree::Split {
        ratio: before,
        dir: SplitDir::Vertical,
        ..
    } = &vertical.win_tree
    else {
        panic!("expected a vertical split");
    };
    let before = *before;

    vertical.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 40,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });
    vertical.on_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 48,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });
    vertical.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 48,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });

    let WindowTree::Split { ratio: after, .. } = &vertical.win_tree else {
        panic!("expected the split to remain");
    };
    assert!(*after > before);
    assert!(vertical.mouse_resize.is_none());

    let mut horizontal = Editor::new();
    horizontal.screen_w = 80;
    horizontal.screen_h = 24;
    horizontal.split_window_below();
    let WindowTree::Split {
        ratio: before,
        dir: SplitDir::Horizontal,
        ..
    } = &horizontal.win_tree
    else {
        panic!("expected a horizontal split");
    };
    let before = *before;

    horizontal.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 12,
        modifiers: KeyModifiers::NONE,
    });
    horizontal.on_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 10,
        row: 16,
        modifiers: KeyModifiers::NONE,
    });
    horizontal.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 10,
        row: 16,
        modifiers: KeyModifiers::NONE,
    });

    let WindowTree::Split { ratio: after, .. } = &horizontal.win_tree else {
        panic!("expected the split to remain");
    };
    assert!(*after > before);
    assert!(horizontal.mouse_resize.is_none());
}
