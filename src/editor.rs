//! Editor state, key dispatch, prompts and the Emacs-style command set.
use crate::buffer::*;
use crate::git;
use crate::keys::{self, Key};
use crate::syntax::Highlighter;
use ratatui::crossterm::event::KeyEvent;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

// ------------------------------------------------------------------ modes
#[derive(Clone, Debug)]
pub enum PKind {
    FindFile,
    QuickFiles,
    SetTheme,
    WriteFile,
    Switch,
    Kill,
    Command,
    Goto,
    Commit { amend: bool },
    Checkout,
    Run,
    ProjFile,
    ProjGrep,
    ProjGrepResults { root: PathBuf, entries: Vec<(String, LineMeta)> },
    SelectDirectory,
    QrFrom,
    QrTo(String),
    SearchHistory,
    TabWidth,
    SetLang,
    DiredCreate(PathBuf),
    DiredRename { dir: PathBuf, source: PathBuf },
    WorkspaceDirectory,
}

enum RunEvent {
    Output(String),
    Finished(Result<ExitStatus, String>, bool),
}

pub struct Prompt {
    pub label: String,
    pub input: String,
    pub cur: usize,
    pub kind: PKind,
    pub choices: Option<Vec<String>>,
    lower: Vec<String>,
    pub matches: Vec<String>,
    pub sel: usize,
    pub hint: Vec<String>,
}

impl Prompt {
    pub fn new(label: &str, kind: PKind, choices: Option<Vec<String>>) -> Self {
        let lower = choices.as_ref().map(|c| c.iter().map(|s| s.to_lowercase()).collect()).unwrap_or_default();
        let mut p = Prompt {
            label: label.to_string(),
            input: String::new(),
            cur: 0,
            kind,
            choices,
            lower,
            matches: Vec::new(),
            sel: 0,
            hint: Vec::new(),
        };
        p.update();
        p
    }

    pub fn with_input(mut self, s: &str) -> Self {
        self.input = s.to_string();
        self.cur = s.chars().count();
        self.update();
        self
    }

    pub fn update(&mut self) {
        self.sel = 0;
        self.hint.clear();
        let Some(ch) = &self.choices else { return };
        let limit = if matches!(&self.kind, PKind::ProjGrepResults { .. }) { 3000 } else { 200 };
        let q = self.input.to_lowercase();
        if q.trim().is_empty() {
            self.matches = ch.iter().take(limit).cloned().collect();
            return;
        }
        let tokens: Vec<&str> = q.split_whitespace().collect();
        let mut scored: Vec<(i32, usize)> = Vec::new();
        for (i, l) in self.lower.iter().enumerate() {
            // every whitespace-separated token must fuzzy-match ("git log" finds "git-log")
            let mut total = 0;
            let mut ok = true;
            for t in &tokens {
                match fuzzy_score(l, t) {
                    Some(s) => total += s,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                scored.push((total, i));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(ch[a.1].len().cmp(&ch[b.1].len())));
        self.matches = scored.into_iter().take(limit).map(|(_, i)| ch[i].clone()).collect();
    }

    fn byte(&self) -> usize {
        byte_of(&self.input, self.cur)
    }
}

pub fn fuzzy_score(item: &str, q: &str) -> Option<i32> {
    let mut qi = q.chars().peekable();
    let base = item.rfind('/').map(|i| i + 1).unwrap_or(0);
    let (mut score, mut consec, mut prev) = (0i32, false, ' ');
    for (i, c) in item.char_indices() {
        match qi.peek() {
            Some(&qc) if qc == c => {
                score += 1;
                if consec {
                    score += 5;
                }
                if i == 0 || "/_-. ".contains(prev) {
                    score += 8;
                }
                if i >= base {
                    score += 2;
                }
                qi.next();
                consec = true;
            }
            _ => consec = false,
        }
        prev = c;
    }
    if qi.peek().is_some() {
        return None;
    }
    if item.contains(q) {
        score += 20;
    }
    Some(score - (item.len() as i32) / 4)
}

pub struct ISearch {
    pub query: String,
    pub forward: bool,
    pub origin: Pos,
    pub matched: Option<(Pos, Pos)>,
    pub failing: bool,
}

pub struct QReplace {
    pub from: String,
    pub to: String,
    pub count: usize,
    pub cur: Option<(Pos, Pos)>,
}

pub enum YAct {
    Quit,
    KillBuf(usize),
    Restore(String),
}

pub struct YesNo {
    pub q: String,
    pub act: YAct,
}

pub enum Mode {
    Normal,
    Prompt(Prompt),
    ISearch(ISearch),
    QReplace(QReplace),
    YesNo(YesNo),
}

// ----------------------------------------------------------------- windows
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitDir {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug)]
pub struct Window {
    pub id: usize,
    pub buf_idx: usize,
    pub cursor: Pos,
    pub top: usize,
    pub left: usize,
}

impl Window {
    pub fn new(id: usize, buf_idx: usize) -> Self {
        Window {
            id,
            buf_idx,
            cursor: Pos::default(),
            top: 0,
            left: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum WindowTree {
    Leaf(usize),
    Split {
        dir: SplitDir,
        ratio: f32,
        first: Box<WindowTree>,
        second: Box<WindowTree>,
    },
}

#[derive(Clone, Debug)]
pub struct MouseResize {
    path: Vec<bool>,
    dir: SplitDir,
    area: ratatui::layout::Rect,
}

#[derive(Clone, Debug)]
pub struct ProjectTreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

#[derive(Clone, Debug)]
pub struct ProjectTree {
    pub root: PathBuf,
    pub entries: Vec<ProjectTreeEntry>,
    pub cursor: usize,
    pub top: usize,
    pub pending: String,
}

impl WindowTree {
    pub fn leaves(&self) -> Vec<usize> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<usize>) {
        match self {
            WindowTree::Leaf(id) => out.push(*id),
            WindowTree::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
    }

    pub fn split(&mut self, target_id: usize, new_id: usize, dir: SplitDir) -> bool {
        match self {
            WindowTree::Leaf(id) if *id == target_id => {
                *self = WindowTree::Split {
                    dir,
                    ratio: 0.5,
                    first: Box::new(WindowTree::Leaf(target_id)),
                    second: Box::new(WindowTree::Leaf(new_id)),
                };
                true
            }
            WindowTree::Leaf(_) => false,
            WindowTree::Split { first, second, .. } => {
                first.split(target_id, new_id, dir) || second.split(target_id, new_id, dir)
            }
        }
    }

    pub fn remove_leaf(&mut self, target_id: usize) -> bool {
        match self {
            WindowTree::Leaf(_) => false,
            WindowTree::Split { first, second, .. } => {
                if let WindowTree::Leaf(id) = **first {
                    if id == target_id {
                        *self = *second.clone();
                        return true;
                    }
                }
                if let WindowTree::Leaf(id) = **second {
                    if id == target_id {
                        *self = *first.clone();
                        return true;
                    }
                }
                first.remove_leaf(target_id) || second.remove_leaf(target_id)
            }
        }
    }

    pub fn balance(&mut self) {
        if let WindowTree::Split { ratio, first, second, .. } = self {
            *ratio = 0.5;
            first.balance();
            second.balance();
        }
    }

    pub fn contains(&self, target_id: usize) -> bool {
        match self {
            WindowTree::Leaf(id) => *id == target_id,
            WindowTree::Split { first, second, .. } => first.contains(target_id) || second.contains(target_id),
        }
    }

    fn find_leaf_path(&self, target_id: usize, path: &mut Vec<bool>) -> bool {
        match self {
            WindowTree::Leaf(id) => *id == target_id,
            WindowTree::Split { first, second, .. } => {
                path.push(false);
                if first.find_leaf_path(target_id, path) {
                    return true;
                }
                path.pop();
                path.push(true);
                if second.find_leaf_path(target_id, path) {
                    return true;
                }
                path.pop();
                false
            }
        }
    }

    fn set_leaf_at_path(&mut self, path: &[bool], id: usize) -> bool {
        match self {
            WindowTree::Leaf(current) if path.is_empty() => {
                *current = id;
                true
            }
            WindowTree::Split { first, second, .. } => match path.split_first() {
                Some((true, rest)) => second.set_leaf_at_path(rest, id),
                Some((false, rest)) => first.set_leaf_at_path(rest, id),
                None => false,
            },
            WindowTree::Leaf(_) => false,
        }
    }

    pub fn swap_leaf_positions(&mut self, first_id: usize, second_id: usize) -> bool {
        if first_id == second_id {
            return false;
        }
        let mut first_path = Vec::new();
        let mut second_path = Vec::new();
        if !self.find_leaf_path(first_id, &mut first_path)
            || !self.find_leaf_path(second_id, &mut second_path)
        {
            return false;
        }
        self.set_leaf_at_path(&first_path, second_id)
            && self.set_leaf_at_path(&second_path, first_id)
    }

    pub fn resize_split(&mut self, target_id: usize, dir: SplitDir, delta: f32) -> bool {
        match self {
            WindowTree::Leaf(_) => false,
            WindowTree::Split { dir: s_dir, ratio, first, second } => {
                if *s_dir == dir && (first.contains(target_id) || second.contains(target_id)) {
                    if first.contains(target_id) {
                        *ratio = (*ratio + delta).clamp(0.1, 0.9);
                    } else {
                        *ratio = (*ratio - delta).clamp(0.1, 0.9);
                    }
                    return true;
                }
                first.resize_split(target_id, dir, delta) || second.resize_split(target_id, dir, delta)
            }
        }
    }

    fn split_at(
        &self,
        area: ratatui::layout::Rect,
        column: u16,
        row: u16,
        path: &mut Vec<bool>,
    ) -> Option<(Vec<bool>, SplitDir, ratatui::layout::Rect)> {
        use ratatui::layout::Rect;
        let WindowTree::Split {
            dir,
            ratio,
            first,
            second,
        } = self
        else {
            return None;
        };

        let (first_area, second_area, boundary) = match dir {
            SplitDir::Horizontal if area.height > 3 => {
                let mut h1 = (area.height as f32 * ratio).round() as u16;
                h1 = h1.clamp(2, area.height.saturating_sub(2));
                let boundary = area.y + h1;
                (
                    Rect::new(area.x, area.y, area.width, h1),
                    Rect::new(area.x, boundary, area.width, area.height - h1),
                    boundary,
                )
            }
            SplitDir::Vertical if area.width > 10 => {
                let rem_w = area.width.saturating_sub(1);
                let mut w1 = (rem_w as f32 * ratio).round() as u16;
                w1 = w1.clamp(4, rem_w.saturating_sub(4));
                let boundary = area.x + w1;
                (
                    Rect::new(area.x, area.y, w1, area.height),
                    Rect::new(boundary + 1, area.y, rem_w - w1, area.height),
                    boundary,
                )
            }
            _ => return None,
        };

        let on_boundary = match dir {
            SplitDir::Horizontal => row.abs_diff(boundary) <= 1 && column >= area.x && column < area.x + area.width,
            SplitDir::Vertical => column == boundary && row >= area.y && row < area.y + area.height,
        };
        if on_boundary {
            return Some((path.clone(), *dir, area));
        }

        path.push(false);
        if let Some(found) = first.split_at(first_area, column, row, path) {
            return Some(found);
        }
        path.pop();
        path.push(true);
        if let Some(found) = second.split_at(second_area, column, row, path) {
            return Some(found);
        }
        path.pop();
        None
    }

    fn set_ratio_at_path(&mut self, path: &[bool], ratio: f32) -> bool {
        match self {
            WindowTree::Split {
                ratio: current,
                first,
                second,
                ..
            } => {
                if let Some((next, rest)) = path.split_first() {
                    if *next {
                        second.set_ratio_at_path(rest, ratio)
                    } else {
                        first.set_ratio_at_path(rest, ratio)
                    }
                } else {
                    *current = ratio.clamp(0.1, 0.9);
                    true
                }
            }
            WindowTree::Leaf(_) => false,
        }
    }

    pub fn compute_rects(
        &self,
        area: ratatui::layout::Rect,
        out: &mut Vec<(usize, ratatui::layout::Rect)>,
        seps: &mut Vec<(u16, u16, u16)>,
    ) {
        use ratatui::layout::Rect;
        match self {
            WindowTree::Leaf(id) => {
                out.push((*id, area));
            }
            WindowTree::Split { dir, ratio, first, second } => match dir {
                SplitDir::Horizontal => {
                    let total_h = area.height;
                    if total_h <= 3 {
                        first.compute_rects(area, out, seps);
                    } else {
                        let mut h1 = (total_h as f32 * ratio).round() as u16;
                        h1 = h1.clamp(2, total_h.saturating_sub(2));
                        let h2 = total_h.saturating_sub(h1);
                        let r1 = Rect::new(area.x, area.y, area.width, h1);
                        let r2 = Rect::new(area.x, area.y + h1, area.width, h2);
                        first.compute_rects(r1, out, seps);
                        second.compute_rects(r2, out, seps);
                    }
                }
                SplitDir::Vertical => {
                    let total_w = area.width;
                    if total_w <= 10 {
                        first.compute_rects(area, out, seps);
                    } else {
                        let rem_w = total_w.saturating_sub(1);
                        let mut w1 = (rem_w as f32 * ratio).round() as u16;
                        w1 = w1.clamp(4, rem_w.saturating_sub(4));
                        let w2 = rem_w.saturating_sub(w1);
                        let sep_x = area.x + w1;
                        seps.push((sep_x, area.y, area.height));
                        let r1 = Rect::new(area.x, area.y, w1, area.height);
                        let r2 = Rect::new(sep_x + 1, area.y, w2, area.height);
                        first.compute_rects(r1, out, seps);
                        second.compute_rects(r2, out, seps);
                    }
                }
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    VisualBlock,
}

pub struct Completion {
    pub items: Vec<String>,
    pub selected: usize,
    pub range: Option<(Pos, Pos)>,
    prefix: String,
    original: Option<String>,
    original_cursor: Option<Pos>,
    preview: Option<String>,
}

impl Completion {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            range: None,
            prefix: String::new(),
            original: None,
            original_cursor: None,
            preview: None,
        }
    }

    fn clear(&mut self) {
        self.items.clear();
        self.selected = 0;
        self.range = None;
        self.prefix.clear();
        self.original = None;
        self.original_cursor = None;
        self.preview = None;
    }
}

fn directional_window_neighbor(
    rects: &[(usize, ratatui::layout::Rect)],
    current_id: usize,
    direction: char,
) -> Option<usize> {
    let current = rects.iter().find(|(id, _)| *id == current_id)?.1;
    let current_left = u32::from(current.x);
    let current_top = u32::from(current.y);
    let current_right = current_left + u32::from(current.width);
    let current_bottom = current_top + u32::from(current.height);
    let current_center_x = current_left + u32::from(current.width) / 2;
    let current_center_y = current_top + u32::from(current.height) / 2;

    rects
        .iter()
        .filter_map(|(id, candidate)| {
            if *id == current_id {
                return None;
            }
            let left = u32::from(candidate.x);
            let top = u32::from(candidate.y);
            let right = left + u32::from(candidate.width);
            let bottom = top + u32::from(candidate.height);
            let center_x = left + u32::from(candidate.width) / 2;
            let center_y = top + u32::from(candidate.height) / 2;
            let overlaps_horizontally = left < current_right && current_left < right;
            let overlaps_vertically = top < current_bottom && current_top < bottom;

            let (distance, alignment) = match direction {
                'h' if overlaps_vertically && right <= current_left + 1 => {
                    (current_left.saturating_sub(right), center_y.abs_diff(current_center_y))
                }
                'l' if overlaps_vertically && current_right <= left + 1 => {
                    (left.saturating_sub(current_right), center_y.abs_diff(current_center_y))
                }
                'k' if overlaps_horizontally && bottom <= current_top + 1 => {
                    (current_top.saturating_sub(bottom), center_x.abs_diff(current_center_x))
                }
                'j' if overlaps_horizontally && current_bottom <= top + 1 => {
                    (top.saturating_sub(current_bottom), center_x.abs_diff(current_center_x))
                }
                _ => return None,
            };
            Some(((distance, alignment), *id))
        })
        .min_by_key(|(score, _)| *score)
        .map(|(_, id)| id)
}

// ----------------------------------------------------------------- editor
pub struct Editor {
    pub bufs: Vec<Buffer>,
    pub cur: usize,
    pub prev_cur: Option<usize>,
    pub windows: Vec<Window>,
    pub active_win_id: usize,
    pub next_win_id: usize,
    pub win_tree: WindowTree,
    pub mouse_resize: Option<MouseResize>,
    pub tree_expanded: HashSet<PathBuf>,
    pub project_tree: Option<ProjectTree>,
    pub project_tree_open: bool,
    pub project_tree_focused: bool,
    pub vim_mode: VimMode,
    vim_goal_col: Option<usize>,
    pub last_click: Option<(Instant, usize, usize)>,
    pub vim_pending: String,
    pub vim_op: Option<char>,
    pub vim_replace_next: bool,
    pub vim_count: usize,
    pub vim_operator_count: usize,
    pub vim_marks: HashMap<char, Pos>,
    pub vim_registers: HashMap<char, String>,
    pub vim_register: Option<char>,
    pub vim_last_find: Option<(char, char)>,
    pub vim_replace_mode: bool,
    pub vim_recording_insert: bool,
    pub vim_insert_record: Vec<Key>,
    pub vim_last_insert: Vec<Key>,
    pub vim_insert_prefix: Option<char>,
    pub vim_last_insert_prefix: Option<char>,
    pub vim_multi_cursors: Vec<Pos>,
    pub completion: Completion,
    completion_project_root: Option<PathBuf>,
    completion_project_stamp: Option<Instant>,
    completion_project_language: String,
    completion_project_words: HashMap<String, usize>,
    pub auto_pair: bool,
    pub kill_ring: Vec<String>,
    pub mode: Mode,
    pub msg: String,
    pub pending: Vec<String>,
    pub quit: bool,
    pub text_h: usize,
    pub text_w: usize,
    pub git: Option<git::Info>,
    pub git_stamp: Instant,
    pub keymap: HashMap<String, &'static str>,
    pub prefixes: HashSet<String>,
    pub line_numbers: bool,
    pub wrap: bool,
    pub auto_indent: bool,
    pub cwd: PathBuf,
    pub proj_root: Option<PathBuf>,
    search_root: Option<PathBuf>,
    pub last_search: String,
    pub last_search_forward: bool,
    pub recent_files: Vec<PathBuf>,
    pub bookmarks: Vec<PathBuf>,
    pub quick_files: Vec<PathBuf>,
    pub search_history: Vec<String>,
    pub command_history: Vec<String>,
    pub run_command: String,
    run_rx: Option<Receiver<RunEvent>>,
    run_stop_tx: Option<Sender<()>>,
    run_stopping: bool,
    run_dir: Option<PathBuf>,
    run_started: Option<Instant>,
    run_elapsed_tenths: u64,
    run_output: Vec<String>,
    last_cmd: String,
    prev_kill: bool,
    pub this_kill: bool,
    pub last_char: Option<char>,
    pub meta_pending: bool,
    pub arg: Option<usize>,
    pub arg_typing: bool,
    pub quote_next: bool,
    pub yank_n: usize,
    pub yank_span: Option<(Pos, Pos)>,
    pub clip: Option<String>,
    pub last_clip: Option<String>,
    pub file_clipboard: Option<PathBuf>,
    pub file_clipboard_cut: bool,
    pub arg_digits: bool,
    pub grep_fresh: bool,
    pub search_highlight: Option<(usize, Pos, Pos)>,
    pub screen_w: u16,
    pub screen_h: u16,
    pub disk_stamp: Instant,
}




impl Editor {
    pub fn new() -> Self {
        let mut keymap = HashMap::new();
        let mut prefixes = HashSet::new();
        for (k, c) in BINDINGS {
            keymap.insert(k.to_string(), *c);
            let parts: Vec<&str> = k.split(' ').collect();
            for i in 1..parts.len() {
                prefixes.insert(parts[..i].join(" "));
            }
        }
        let mut scratch = Buffer::new("*scratch*");
        scratch.hl = Highlighter::plain();
        let init_win = Window::new(1, 0);
        Editor {
            bufs: vec![scratch],
            cur: 0,
            prev_cur: None,
            windows: vec![init_win],
            active_win_id: 1,
            next_win_id: 2,
            win_tree: WindowTree::Leaf(1),
            mouse_resize: None,
            tree_expanded: HashSet::new(),
            project_tree: None,
            project_tree_open: false,
            project_tree_focused: false,
            vim_mode: VimMode::Normal,
            vim_goal_col: None,
            last_click: None,
            vim_pending: String::new(),
            vim_op: None,
            vim_replace_next: false,
            vim_count: 0,
            vim_operator_count: 0,
            vim_marks: HashMap::new(),
            vim_registers: HashMap::new(),
            vim_register: None,
            vim_last_find: None,
            vim_replace_mode: false,
            vim_recording_insert: false,
            vim_insert_record: Vec::new(),
            vim_last_insert: Vec::new(),
            vim_insert_prefix: None,
            vim_last_insert_prefix: None,
            vim_multi_cursors: Vec::new(),
            completion: Completion::new(),
            completion_project_root: None,
            completion_project_stamp: None,
            completion_project_language: String::new(),
            completion_project_words: HashMap::new(),
            auto_pair: true,
            kill_ring: Vec::new(),
            mode: Mode::Normal,
            msg: String::new(),
            pending: Vec::new(),
            quit: false,
            text_h: 24,
            text_w: 80,
            git: None,
            git_stamp: Instant::now(),
            disk_stamp: Instant::now(),
            keymap,
            prefixes,
            line_numbers: false,
            wrap: false,
            auto_indent: true,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            proj_root: None,
            search_root: None,
            last_search: String::new(),
            last_search_forward: true,
            recent_files: Vec::new(),
            bookmarks: Vec::new(),
            quick_files: Vec::new(),
            search_history: Vec::new(),
            command_history: Vec::new(),
            run_command: String::new(),
            run_rx: None,
            run_stop_tx: None,
            run_stopping: false,
            run_dir: None,
            run_started: None,
            run_elapsed_tenths: 0,
            run_output: Vec::new(),
            last_cmd: String::new(),
            prev_kill: false,
            this_kill: false,
            last_char: None,
            meta_pending: false,
            arg: None,
            arg_typing: false,
            quote_next: false,
            yank_n: 0,
            yank_span: None,
            clip: None,
            last_clip: None,
            file_clipboard: None,
            file_clipboard_cut: false,
            arg_digits: false,
            grep_fresh: false,
            search_highlight: None,
            screen_w: 80,
            screen_h: 24,
        }
    }

    pub fn buf(&self) -> &Buffer {
        &self.bufs[self.cur]
    }
    pub fn buf_mut(&mut self) -> &mut Buffer {
        &mut self.bufs[self.cur]
    }
    pub fn say(&mut self, s: impl Into<String>) {
        self.msg = s.into();
    }

    // ------------------------------------------------------ files/buffers
    pub fn norm_path(&self, s: &str) -> PathBuf {
        let s = if let Some(rest) = s.strip_prefix("~/") {
            match std::env::var_os("HOME") {
                Some(h) => PathBuf::from(h).join(rest).to_string_lossy().to_string(),
                None => s.to_string(),
            }
        } else {
            s.to_string()
        };
        let p = PathBuf::from(&s);
        let p = if p.is_absolute() { p } else { self.cwd.join(p) };
        if let Ok(c) = std::fs::canonicalize(&p) {
            return c;
        }
        match (p.parent().and_then(|d| std::fs::canonicalize(d).ok()), p.file_name()) {
            (Some(d), Some(f)) => d.join(f),
            _ => p,
        }
    }

    pub fn switch_to(&mut self, idx: usize) {
        if idx != self.cur {
            self.vim_goal_col = None;
            self.prev_cur = Some(self.cur);
            self.search_highlight = None;
        }
        self.cur = idx;
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == self.active_win_id) {
            w.buf_idx = idx;
            if idx < self.bufs.len() {
                let b = &self.bufs[idx];
                w.cursor = b.cursor;
                w.top = b.top;
                w.left = b.left;
            }
        }
        self.refresh_git_info();
    }

    pub fn open_path(&mut self, path: &Path) -> Result<usize, String> {
        if let Some(i) = self.bufs.iter().position(|b| b.kind == BufKind::File && b.path.as_deref() == Some(path)) {
            self.remember_recent(path);
            return Ok(i);
        }
        let b = if path.is_dir() {
            return Err("is a directory".into());
        } else if path.exists() {
            Buffer::load(path).map_err(|e| format!("{}: {e}", path.display()))?
        } else {
            let mut b = Buffer::new(&path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            b.path = Some(path.to_path_buf());
            b.redetect();
            b
        };
        // drop an untouched *scratch* when the first real file is opened
        if self.bufs.len() == 1 && self.bufs[0].name == "*scratch*" && !self.bufs[0].modified && self.bufs[0].lines == vec![String::new()] {
            self.bufs.clear();
            self.cur = 0;
        }
        self.bufs.push(b);
        self.remember_recent(path);
        Ok(self.bufs.len() - 1)
    }

    fn remember_recent(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(30);
        let file = ow_config_dir().join("recent");
        if let Some(parent) = file.parent() { let _ = std::fs::create_dir_all(parent); }
        let data = self.recent_files.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>().join("\n");
        let _ = std::fs::write(file, data);
    }

    pub fn recent_files_cmd(&mut self) {
        if self.recent_files.is_empty() {
            self.recent_files = std::fs::read_to_string(ow_config_dir().join("recent"))
                .map(|s| s.lines().map(PathBuf::from).filter(|p| p.exists()).collect())
                .unwrap_or_default();
        }
        let choices = self.recent_files.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>();
        if choices.is_empty() { self.say("No recent files"); return; }
        self.start_prompt(Prompt::new("Recent file: ", PKind::FindFile, Some(choices)));
    }

    pub fn bookmark_current_file(&mut self) {
        let Some(path) = self.buf().path.clone() else { self.say("Current buffer is not a file"); return; };
        if self.bookmarks.is_empty() {
            self.bookmarks = std::fs::read_to_string(ow_config_dir().join("bookmarks"))
                .map(|s| s.lines().map(PathBuf::from).collect()).unwrap_or_default();
        }
        if !self.bookmarks.contains(&path) { self.bookmarks.push(path); }
        let file = ow_config_dir().join("bookmarks");
        if let Some(parent) = file.parent() { let _ = std::fs::create_dir_all(parent); }
        let data = self.bookmarks.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>().join("\n");
        let _ = std::fs::write(file, data);
        self.say("File bookmarked");
    }

    pub fn bookmark_list(&mut self) {
        if self.bookmarks.is_empty() {
            self.bookmarks = std::fs::read_to_string(ow_config_dir().join("bookmarks"))
                .map(|s| s.lines().map(PathBuf::from).filter(|p| p.exists()).collect())
                .unwrap_or_default();
        }
        let choices = self.bookmarks.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>();
        if choices.is_empty() { self.say("No file bookmarks"); return; }
        self.start_prompt(Prompt::new("Bookmark: ", PKind::FindFile, Some(choices)));
    }

    pub fn remove_current_file_bookmark(&mut self) {
        let Some(path) = self.buf().path.clone() else { self.say("Current buffer is not a file"); return; };
        if self.bookmarks.is_empty() {
            self.bookmarks = std::fs::read_to_string(ow_config_dir().join("bookmarks"))
                .map(|s| s.lines().map(PathBuf::from).collect())
                .unwrap_or_default();
        }
        let original_len = self.bookmarks.len();
        self.bookmarks.retain(|bookmark| bookmark != &path);
        if self.bookmarks.len() == original_len {
            self.say("Current file is not bookmarked");
            return;
        }

        let file = ow_config_dir().join("bookmarks");
        if let Some(parent) = file.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                self.say(format!("Could not save bookmarks: {error}"));
                return;
            }
        }
        let data = self.bookmarks.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>().join("\n");
        if let Err(error) = std::fs::write(file, data) {
            self.say(format!("Could not save bookmarks: {error}"));
            return;
        }
        self.say("File bookmark removed");
    }

    fn load_quick_files(&mut self) -> Result<(), String> {
        if !self.quick_files.is_empty() {
            return Ok(());
        }
        let file = ow_config_dir().join("quick-files");
        match std::fs::read_to_string(file) {
            Ok(data) => {
                self.quick_files = data.lines().map(PathBuf::from).filter(|path| path.exists()).collect();
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not load quick files: {error}")),
        }
    }

    fn save_quick_files(&self, files: &[PathBuf]) -> Result<(), String> {
        let file = ow_config_dir().join("quick-files");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("Could not save quick files: {error}"))?;
        }
        let data = files.iter().map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>().join("\n");
        std::fs::write(file, data).map_err(|error| format!("Could not save quick files: {error}"))
    }

    pub fn add_quick_file(&mut self) {
        let Some(path) = self.buf().path.clone() else { self.say("Current buffer is not a file"); return; };
        if let Err(error) = self.load_quick_files() {
            self.say(error);
            return;
        }
        if self.quick_files.contains(&path) {
            self.say("File is already in quick files");
            return;
        }
        let mut files = self.quick_files.clone();
        files.push(path);
        match self.save_quick_files(&files) {
            Ok(()) => {
                self.quick_files = files;
                self.say(format!("Added quick file {}", self.quick_files.len()));
            }
            Err(error) => self.say(error),
        }
    }

    pub fn quick_files_cmd(&mut self) {
        if let Err(error) = self.load_quick_files() {
            self.say(error);
            return;
        }
        self.quick_files.retain(|path| path.exists());
        let choices = self.quick_files.iter().map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>();
        if choices.is_empty() {
            self.say("No quick files");
            return;
        }
        self.start_prompt(Prompt::new("Quick file (RET open, d delete): ", PKind::QuickFiles, Some(choices)));
    }

    pub fn quick_file_at(&mut self, slot: usize) {
        if let Err(error) = self.load_quick_files() {
            self.say(error);
            return;
        }
        let Some(path) = slot.checked_sub(1).and_then(|index| self.quick_files.get(index)).cloned() else {
            self.say(format!("No quick file at slot {slot}"));
            return;
        };
        if !path.exists() {
            self.say(format!("Quick file does not exist: {}", path.display()));
            return;
        }
        match self.open_path(&path) {
            Ok(index) => self.switch_to(index),
            Err(error) => self.say(error),
        }
    }

    fn remove_selected_quick_file(&mut self, prompt: &mut Prompt) {
        let Some(selected) = prompt.matches.get(prompt.sel).cloned() else {
            self.say("No quick file selected");
            return;
        };
        let selected = PathBuf::from(selected);
        let mut files = self.quick_files.clone();
        let original_len = files.len();
        files.retain(|path| path != &selected);
        if files.len() == original_len {
            self.say("Selected file is not in quick files");
            return;
        }
        match self.save_quick_files(&files) {
            Ok(()) => {
                self.quick_files = files;
                let choices: Vec<String> = self.quick_files.iter().map(|path| path.to_string_lossy().to_string()).collect();
                prompt.lower = choices.iter().map(|path| path.to_lowercase()).collect();
                prompt.choices = Some(choices);
                prompt.input.clear();
                prompt.cur = 0;
                prompt.update();
                self.say("Quick file removed");
            }
            Err(error) => self.say(error),
        }
    }

    pub fn search_history_cmd(&mut self) {
        if self.search_history.is_empty() {
            self.search_history = std::fs::read_to_string(ow_config_dir().join("search-history"))
                .map(|s| s.lines().map(|line| line.to_string()).collect()).unwrap_or_default();
        }
        if self.search_history.is_empty() { self.say("No search history"); return; }
        self.start_prompt(Prompt::new("Search history: ", PKind::SearchHistory, Some(self.search_history.clone())));
    }

    fn remember_command(&mut self, name: &str) {
        if name == "self-insert-command" { return; }
        self.command_history.retain(|item| item != name);
        self.command_history.insert(0, name.to_string());
        self.command_history.truncate(50);
        let file = ow_config_dir().join("command-history");
        if let Some(parent) = file.parent() { let _ = std::fs::create_dir_all(parent); }
        let _ = std::fs::write(file, self.command_history.join("\n"));
    }

    pub fn save_session(&self) {
        let file = ow_config_dir().join("session");
        if let Some(parent) = file.parent() { let _ = std::fs::create_dir_all(parent); }
        let mut rows = Vec::new();
        for b in &self.bufs {
            if b.kind == BufKind::File {
                if let Some(path) = &b.path {
                    rows.push(format!("{}\t{}\t{}", path.to_string_lossy(), b.cursor.line, b.cursor.col));
                }
            }
        }
        let _ = std::fs::write(file, rows.join("\n"));
    }

    pub fn restore_session(&mut self) {
        let data = match std::fs::read_to_string(ow_config_dir().join("session")) { Ok(s) => s, Err(_) => return };
        for row in data.lines() {
            let mut fields = row.rsplitn(3, '\t');
            let col = fields.next().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            let line = fields.next().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            let Some(path) = fields.next() else { continue; };
            if !Path::new(path).exists() { continue; }
            if let Ok(i) = self.open_path(Path::new(path)) {
                self.switch_to(i);
                let height = self.text_h;
                let b = self.buf_mut();
                b.cursor = b.clamp(Pos::new(line, col));
                b.top = line.saturating_sub(height / 2);
            }
        }
    }

    pub fn find_file(&mut self, s: &str) {
        let p = self.norm_path(s);
        if p.is_dir() {
            self.open_dir(&p);
            return;
        }
        let existed = p.exists();
        match self.open_path(&p) {
            Ok(i) => {
                let ro = self.bufs[i].read_only;
                self.switch_to(i);
                if !existed {
                    self.say("(New file)");
                } else if ro {
                    self.say("Binary file: opened read-only");
                }
            }
            Err(e) => self.say(e),
        }
    }

    pub fn goto_line_in_cur(&mut self, line: usize, col: usize) {
        let b = self.buf_mut();
        b.cursor = b.clamp(Pos::new(line.saturating_sub(1), col));
    }

    pub fn kill_buffer_idx(&mut self, idx: usize) {
        self.search_highlight = None;
        if self.bufs.len() == 1 {
            self.bufs[0] = Buffer::new("*scratch*");
            self.cur = 0;
            for w in &mut self.windows {
                w.buf_idx = 0;
                w.cursor = Pos::default();
                w.top = 0;
                w.left = 0;
            }
            return;
        }
        self.bufs.remove(idx);
        let target = match self.prev_cur {
            Some(p) if p != idx => Some(if p > idx { p - 1 } else { p }),
            _ => None,
        };
        if idx < self.cur {
            self.cur -= 1;
        } else if idx == self.cur {
            self.cur = target.unwrap_or(idx.min(self.bufs.len() - 1));
        }
        self.prev_cur = None;
        self.cur = self.cur.min(self.bufs.len() - 1);
        for w in &mut self.windows {
            if w.buf_idx == idx {
                w.buf_idx = self.cur;
                w.cursor = Pos::default();
                w.top = 0;
                w.left = 0;
            } else if w.buf_idx > idx {
                w.buf_idx -= 1;
            }
        }
        self.refresh_git_info();
    }

    // ----------------------------------------------------------- windows
    pub fn sync_active_win_from_buf(&mut self) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == self.active_win_id) {
            if w.buf_idx < self.bufs.len() {
                let b = &self.bufs[w.buf_idx];
                w.cursor = b.cursor;
                w.top = b.top;
                w.left = b.left;
            }
        }
    }

    pub fn set_active_window(&mut self, next_id: usize) {
        if next_id == self.active_win_id {
            return;
        }
        self.sync_active_win_from_buf();
        if let Some(w) = self.windows.iter().find(|w| w.id == next_id) {
            let b_idx = w.buf_idx;
            let cursor = w.cursor;
            let top = w.top;
            let left = w.left;
            if self.cur != b_idx {
                self.search_highlight = None;
            }
            self.active_win_id = next_id;
            self.cur = b_idx;
            if b_idx < self.bufs.len() {
                self.bufs[b_idx].cursor = cursor;
                self.bufs[b_idx].top = top;
                self.bufs[b_idx].left = left;
            }
            self.refresh_git_info();
        }
    }

    pub fn split_window(&mut self, dir: SplitDir) {
        self.sync_active_win_from_buf();
        let new_id = self.next_win_id;
        self.next_win_id += 1;
        let active_win = self.windows.iter().find(|w| w.id == self.active_win_id).cloned();
        if let Some(cur) = active_win {
            let new_win = Window {
                id: new_id,
                buf_idx: cur.buf_idx,
                cursor: cur.cursor,
                top: cur.top,
                left: cur.left,
            };
            self.windows.push(new_win);
            self.win_tree.split(self.active_win_id, new_id, dir);
        }
    }

    pub fn split_window_below(&mut self) {
        self.split_window(SplitDir::Horizontal);
    }

    pub fn split_window_right(&mut self) {
        self.split_window(SplitDir::Vertical);
    }

    pub fn move_current_window(&mut self, direction: char) {
        self.sync_active_win_from_buf();
        let current_id = self.active_win_id;
        let tree_width = self.project_tree_width();
        let window_x = if tree_width > 0 {
            tree_width.saturating_add(1)
        } else {
            0
        };
        let area = ratatui::layout::Rect::new(
            window_x,
            0,
            self.screen_w.saturating_sub(window_x),
            self.screen_h.saturating_sub(1),
        );
        let mut rects = Vec::new();
        let mut separators = Vec::new();
        self.win_tree.compute_rects(area, &mut rects, &mut separators);
        let Some(target_id) = directional_window_neighbor(&rects, current_id, direction) else {
            self.say("No window in that direction");
            return;
        };
        if self.win_tree.swap_leaf_positions(current_id, target_id) {
            self.say("Moved window");
        } else {
            self.say("Could not move window");
        }
    }

    pub fn delete_other_windows(&mut self) {
        self.windows.retain(|w| w.id == self.active_win_id);
        self.win_tree = WindowTree::Leaf(self.active_win_id);
    }

    pub fn delete_window(&mut self) {
        if self.windows.len() <= 1 {
            self.say("Cannot delete the only window");
            return;
        }
        let target_id = self.active_win_id;
        self.win_tree.remove_leaf(target_id);
        self.windows.retain(|w| w.id != target_id);
        let leaves = self.win_tree.leaves();
        if let Some(&first) = leaves.first() {
            self.set_active_window(first);
        }
    }

    pub fn other_window(&mut self) {
        let leaves = self.win_tree.leaves();
        if leaves.len() <= 1 {
            return;
        }
        if let Some(pos) = leaves.iter().position(|&id| id == self.active_win_id) {
            let next_id = leaves[(pos + 1) % leaves.len()];
            self.set_active_window(next_id);
        }
    }

    pub fn balance_windows(&mut self) {
        self.win_tree.balance();
        self.say("Balanced windows");
    }

    pub fn open_special(&mut self, name: &str, kind: BufKind, text: &str, meta: Vec<LineMeta>, keep_pos: bool) {
        let idx = match self.bufs.iter().position(|b| b.name == name && b.kind == kind) {
            Some(i) => i,
            None => {
                let mut b = Buffer::new(name);
                b.kind = kind;
                self.bufs.push(b);
                self.bufs.len() - 1
            }
        };
        let old = self.bufs[idx].cursor;
        {
            let b = &mut self.bufs[idx];
            b.hl = match kind {
                BufKind::GitDiff => Highlighter::by_name("Diff").unwrap_or_else(Highlighter::plain),
                _ => Highlighter::plain(),
            };
            b.set_special(text, meta);
            b.top = if keep_pos { b.top } else { 0 };
            b.cursor = if keep_pos { b.clamp(old) } else { Pos::default() };
        }
        if idx != self.cur {
            self.switch_to(idx);
        }
    }

    /// Open a directory in a Dired buffer. The current directory is stored in
    /// Buffer::path; line metadata is reserved for the entries shown on screen.
    pub fn open_dir(&mut self, dir: &Path) {
        self.open_dir_at(dir, None);
    }

    /// Open a directory and optionally select an entry inside it.
    pub fn open_dir_at(&mut self, dir: &Path, select: Option<&Path>) {
        let dir = match std::fs::canonicalize(dir) {
            Ok(path) => path,
            Err(err) => {
                self.say(format!("Dired: cannot access {}: {err}", dir.display()));
                return;
            }
        };
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
                self.say(format!("Dired: cannot read {}: {err}", dir.display()));
                return;
            }
        };

        let mut entries: Vec<(bool, String, PathBuf)> = Vec::new();
        let mut entry_error = None;
        for item in read_dir {
            match item {
                Ok(entry) => {
                    let path = entry.path();
                    let is_dir = std::fs::metadata(&path)
                        .map(|metadata| metadata.is_dir())
                        .unwrap_or(false);
                    entries.push((is_dir, entry.file_name().to_string_lossy().into_owned(), path));
                }
                Err(err) => entry_error = Some(err),
            }
        }
        entries.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });

        let mut text = format!("{}:\n", dir.display());
        let parent_meta = dir.parent()
            .map(|parent| LineMeta::Path(parent.to_path_buf()))
            .unwrap_or(LineMeta::None);
        let mut meta = vec![LineMeta::None, parent_meta];
        text.push_str("  ../\n");
        for (is_dir, name, path) in &entries {
            text.push_str(&format!("  {}{}\n", name, if *is_dir { "/" } else { "" }));
            meta.push(LineMeta::Path(path.clone()));
        }

        let name = format!("*dired {}*", dir.display());
        self.open_special(&name, BufKind::Dired, &text, meta, false);
        {
            let visible_height = self.text_h.saturating_sub(2) / 2;
            let buffer = self.buf_mut();
            buffer.path = Some(dir.clone());
            let selected_line = select
                .and_then(|wanted| buffer.meta.iter().position(|item| matches!(item, LineMeta::Path(path) if path.as_path() == wanted)))
                .unwrap_or(1);
            buffer.cursor = Pos::new(selected_line.min(buffer.lines.len().saturating_sub(1)), 2);
            buffer.top = buffer.cursor.line.saturating_sub(visible_height);
        }
        if let Some(err) = entry_error {
            self.say(format!("Dired: some entries could not be read: {err}"));
        }
    }

    pub fn project_tree_root(&self) -> Option<PathBuf> {
        self.project_tree.as_ref().map(|tree| tree.root.clone())
    }

    pub fn open_project_tree(&mut self, root: &Path) {
        let root = match std::fs::canonicalize(root) {
            Ok(path) if path.is_dir() => path,
            _ => {
                self.say(format!("Project tree: not a directory: {}", root.display()));
                return;
            }
        };
        self.tree_expanded.insert(root.clone());
        self.project_tree = Some(ProjectTree {
            root: root.clone(),
            entries: Vec::new(),
            cursor: 0,
            top: 0,
            pending: String::new(),
        });
        self.refresh_project_tree(None);
        self.project_tree_open = true;
        self.project_tree_focused = true;
        self.proj_root = Some(root);
    }

    fn refresh_project_tree(&mut self, selected: Option<&Path>) {
        let Some(root) = self.project_tree.as_ref().map(|tree| tree.root.clone()) else {
            return;
        };
        let mut entries = Vec::new();
        self.append_project_tree(&root, 0, &mut entries);
        if let Some(tree) = self.project_tree.as_mut() {
            tree.entries = entries;
            if let Some(selected) = selected {
                if let Some(cursor) = tree.entries.iter().position(|entry| entry.path == selected) {
                    tree.cursor = cursor;
                } else {
                    tree.cursor = tree.cursor.min(tree.entries.len().saturating_sub(1));
                }
            }
            tree.top = tree.top.min(tree.cursor);
        }
    }

    fn append_project_tree(
        &self,
        dir: &Path,
        depth: usize,
        entries: &mut Vec<ProjectTreeEntry>,
    ) {
        let Ok(dir_entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut dir_entries: Vec<_> = dir_entries.flatten().collect();
        dir_entries.sort_by(|a, b| {
            let a_dir = a.file_type().is_ok_and(|kind| kind.is_dir());
            let b_dir = b.file_type().is_ok_and(|kind| kind.is_dir());
            b_dir
                .cmp(&a_dir)
                .then_with(|| a.file_name().to_string_lossy().to_lowercase()
                    .cmp(&b.file_name().to_string_lossy().to_lowercase()))
        });

        for entry in dir_entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(name.as_str(), ".git" | "target" | "node_modules") {
                continue;
            }
            let path = entry.path();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            let expanded = is_dir && self.tree_expanded.contains(&path);
            entries.push(ProjectTreeEntry {
                path: path.clone(),
                name,
                depth,
                is_dir,
                expanded,
            });
            if expanded {
                self.append_project_tree(&path, depth + 1, entries);
            }
        }
    }

    pub fn toggle_project_tree_directory(&mut self, path: &Path) {
        if self.tree_expanded.contains(path) {
            self.tree_expanded.remove(path);
        } else {
            self.tree_expanded.insert(path.to_path_buf());
        }
        self.refresh_project_tree(Some(path));
    }

    pub fn project_tree_selected_path(&self) -> Option<PathBuf> {
        self.project_tree
            .as_ref()
            .and_then(|tree| tree.entries.get(tree.cursor))
            .map(|entry| entry.path.clone())
    }

    fn project_tree_current_dir(&self) -> Option<PathBuf> {
        let tree = self.project_tree.as_ref()?;
        match tree.entries.get(tree.cursor) {
            Some(entry) if entry.is_dir => Some(entry.path.clone()),
            Some(entry) => entry.path.parent().map(Path::to_path_buf),
            None => Some(tree.root.clone()),
        }
    }

    pub fn project_tree_width(&self) -> u16 {
        if !self.project_tree_open || self.screen_w < 30 {
            return 0;
        }
        (self.screen_w / 4).clamp(18, 30).min(self.screen_w.saturating_sub(13))
    }

    fn move_project_tree_cursor(&mut self, delta: isize) {
        let Some(tree) = self.project_tree.as_mut() else { return };
        if tree.entries.is_empty() {
            tree.cursor = 0;
        } else if delta < 0 {
            tree.cursor = tree.cursor.saturating_sub(delta.unsigned_abs());
        } else {
            tree.cursor = tree.cursor.saturating_add(delta as usize).min(tree.entries.len() - 1);
        }
        self.fix_project_tree_scroll();
    }

    fn fix_project_tree_scroll(&mut self) {
        let height = self.screen_h.saturating_sub(2) as usize;
        if height == 0 {
            return;
        }
        if let Some(tree) = self.project_tree.as_mut() {
            if tree.cursor < tree.top {
                tree.top = tree.cursor;
            } else if tree.cursor >= tree.top + height {
                tree.top = tree.cursor + 1 - height;
            }
        }
    }

    fn open_project_tree_file(&mut self, path: &Path) {
        self.split_window_right();
        let new_window = self.next_win_id - 1;
        self.set_active_window(new_window);
        match self.open_path(path) {
            Ok(index) => {
                self.switch_to(index);
                self.project_tree_focused = false;
            }
            Err(error) => {
                self.say(error);
                self.delete_window();
            }
        }
    }

    pub fn visit_project_tree_entry(&mut self) {
        let Some(entry) = self
            .project_tree
            .as_ref()
            .and_then(|tree| tree.entries.get(tree.cursor))
            .cloned()
        else {
            return;
        };
        if entry.is_dir {
            self.toggle_project_tree_directory(&entry.path);
        } else {
            self.open_project_tree_file(&entry.path);
        }
    }

    pub fn copy_file(&mut self, path: PathBuf) {
        if !path_exists(path.as_path()) {
            self.say(format!("Copy failed: {} no longer exists", path.display()));
            return;
        }
        self.file_clipboard = Some(path.clone());
        self.file_clipboard_cut = false;
        self.say(format!("Copied: {}", path.file_name().unwrap_or_default().to_string_lossy()));
    }

    pub fn cut_file(&mut self, path: PathBuf) {
        if !path_exists(path.as_path()) {
            self.say(format!("Cut failed: {} no longer exists", path.display()));
            return;
        }
        self.file_clipboard = Some(path.clone());
        self.file_clipboard_cut = true;
        self.say(format!("Cut: {}", path.file_name().unwrap_or_default().to_string_lossy()));
    }

    pub fn paste_file(&mut self, dest_dir: PathBuf) {
        let Some(src) = self.file_clipboard.clone() else {
            self.say("No file in clipboard");
            return;
        };
        let Some(file_name) = src.file_name() else {
            self.say("Paste failed: invalid source path");
            return;
        };
        let dest = dest_dir.join(file_name);
        if !path_exists(src.as_path()) {
            self.say(format!("Paste failed: {} no longer exists", src.display()));
            return;
        }
        if src == dest {
            self.say("Paste cancelled: source and destination are identical");
            return;
        }
        if src.is_dir() && dest.starts_with(&src) {
            self.say("Paste cancelled: cannot copy a directory into itself");
            return;
        }
        if path_exists(dest.as_path()) {
            self.say(format!("Paste failed: {} already exists", dest.display()));
            return;
        }

        let result = if self.file_clipboard_cut {
            match std::fs::rename(&src, &dest) {
                Ok(()) => Ok(()),
                Err(rename_error) => {
                    // Cross-device moves cannot be completed with rename alone.
                    match copy_path_recursive(&src, &dest) {
                        Ok(()) => match remove_path_recursive(&src) {
                            Ok(()) => Ok(()),
                            Err(err) => {
                                let _ = remove_path_recursive(&dest);
                                Err(err)
                            }
                        },
                        Err(copy_error) => Err(io::Error::new(
                            copy_error.kind(),
                            format!("{copy_error} (rename also failed: {rename_error})"),
                        )),
                    }
                }
            }
        } else {
            copy_path_recursive(&src, &dest)
        };

        match result {
            Err(err) => self.say(format!("Paste failed: {err}")),
            Ok(()) => {
                if self.file_clipboard_cut {
                    self.file_clipboard = None;
                    self.file_clipboard_cut = false;
                }
                self.say(format!("Pasted: {}", dest.file_name().unwrap_or_default().to_string_lossy()));
                self.dired_refresh();
            }
        }
    }

    pub fn create_file(&mut self, name: &str) {
        let p = self.norm_path(name);
        if let Err(e) = std::fs::File::create(&p) {
            self.say(format!("Create failed: {e}"));
        } else {
            self.say(format!("Created: {}", p.file_name().unwrap_or_default().to_string_lossy()));
            self.dired_refresh();
        }
    }

    pub fn delete_file(&mut self, path: PathBuf) {
        if !path_exists(path.as_path()) {
            self.say(format!("Delete failed: {} no longer exists", path.display()));
            return;
        }
        match remove_path_recursive(&path) {
            Err(err) => self.say(format!("Delete failed: {err}")),
            Ok(()) => {
                self.say(format!("Deleted: {}", path.file_name().unwrap_or_default().to_string_lossy()));
                if self.file_clipboard.as_deref() == Some(path.as_path()) {
                    self.file_clipboard = None;
                    self.file_clipboard_cut = false;
                }
                self.dired_refresh();
            }
        }
    }

    /// Return the directory represented by the active Dired buffer.
    pub fn dired_current_dir(&self) -> Option<PathBuf> {
        if self.buf().kind == BufKind::Dired {
            self.buf().path.clone()
        } else {
            None
        }
    }

    /// Refresh the current listing while keeping the selected entry if possible.
    pub fn dired_refresh(&mut self) {
        if self.project_tree_open {
            let selected = self.project_tree_selected_path();
            self.refresh_project_tree(selected.as_deref());
        }
        if self.buf().kind != BufKind::Dired {
            return;
        }
        let dir = self.dired_current_dir();
        let selected = self.buf().meta.get(self.buf().cursor.line).and_then(|meta| {
            if let LineMeta::Path(path) = meta { Some(path.clone()) } else { None }
        });
        if let Some(dir) = dir {
            self.open_dir_at(&dir, selected.as_deref());
        } else {
            self.say("Dired: current directory is unavailable");
        }
    }

    pub fn save_current(&mut self, path: Option<PathBuf>) {
        let (n, name) = {
            let b = self.buf_mut();
            let res = b.save(path.as_deref());
            match res {
                Ok(()) => (Ok(b.lines.len()), b.path.clone()),
                Err(e) => (Err(e.to_string()), None),
            }
        };
        match n {
            Ok(n) => {
                self.say(format!("Wrote {} ({n} lines)", name.map(|p| p.display().to_string()).unwrap_or_default()));
                self.buf_mut().git_marks_version = u64::MAX;
                self.refresh_git_info();
            }
            Err(e) => self.say(format!("Save failed: {e}")),
        }
    }

    pub fn reload_buffer(&mut self, idx: usize) {
        let Some(p) = self.bufs[idx].path.clone() else { return };
        match std::fs::read(&p) {
            Ok(bytes) => {
                let b = &mut self.bufs[idx];
                b.set_text(&String::from_utf8_lossy(&bytes));
                if let Ok(metadata) = std::fs::metadata(&p) {
                    b.mtime = metadata.modified().ok();
                    b.file_size = Some(metadata.len());
                }
                b.git_marks_version = u64::MAX;
            }
            Err(_) => {} // file vanished: keep contents
        }
    }

    /// Reload unmodified buffers whose file changed on disk (after pull/checkout/external edits).
    pub fn revert_unmodified(&mut self) -> usize {
        let mut n = 0;
        for i in 0..self.bufs.len() {
            let b = &self.bufs[i];
            if b.kind != BufKind::File || b.modified {
                continue;
            }
            let Some(p) = &b.path else { continue };
            let metadata = std::fs::metadata(p);
            let changed = metadata
                .as_ref()
                .is_ok_and(|metadata| {
                    Some(metadata.len()) != b.file_size
                        || metadata.modified().ok() != b.mtime
                });
            if changed {
                self.reload_buffer(i);
                n += 1;
            }
        }
        n
    }

    // ------------------------------------------------------------- scroll
    pub fn gutter(&self) -> (usize, usize) {
        let b = self.buf();
        let marks = if b.kind == BufKind::File && b.git_tracked { 1 } else { 0 };
        let ln = if self.line_numbers { b.lines.len().to_string().len() + 1 } else { 0 };
        (marks, ln)
    }

    pub fn fix_scroll(&mut self) {
        let h = self.text_h.max(1);
        let (g1, g2) = self.gutter();
        let avail = self.text_w.saturating_sub(g1 + g2).max(1);
        let b = &mut self.bufs[self.cur];
        let l = b.cursor.line;
        if self.wrap {
            if l < b.top {
                b.top = l;
                b.left = 0;
            }

            let top_segment = wrap_segments(&b.lines[b.top], b.tab_width, avail)
                .iter()
                .position(|(start, _)| *start == b.left)
                .unwrap_or(0);
            let mut cursor_row = if l == b.top {
                0
            } else {
                wrap_segments(&b.lines[b.top], b.tab_width, avail)
                    .len()
                    .saturating_sub(top_segment)
            };
            for line in b.top + 1..l {
                cursor_row += wrap_segments(&b.lines[line], b.tab_width, avail).len();
            }
            let cursor_segments = wrap_segments(&b.lines[l], b.tab_width, avail);
            let display_column = display_col(&b.lines[l], b.cursor.col, b.tab_width);
            let cursor_segment = cursor_segments
                .iter()
                .position(|(start, end)| display_column >= *start && display_column < *end)
                .or_else(|| {
                    cursor_segments
                        .iter()
                        .position(|(start, end)| display_column == *start && *start == *end)
                })
                .unwrap_or(cursor_segments.len().saturating_sub(1));
            cursor_row += if l == b.top {
                cursor_segment.saturating_sub(top_segment)
            } else {
                cursor_segment
            };

            if cursor_row >= h {
                b.top = l;
                b.left = 0;
                if cursor_segment >= h {
                    b.left = cursor_segments[cursor_segment + 1 - h].0;
                }
            }
            b.top = b.top.min(b.lines.len() - 1);
            return;
        }

        if l < b.top {
            b.top = if b.top - l == 1 { l } else { l.saturating_sub(h / 2) };
        } else if l >= b.top + h {
            b.top = if l + 1 - (b.top + h) == 1 { l + 1 - h } else { l.saturating_sub(h / 2) };
        }
        b.top = b.top.min(b.lines.len() - 1);
        let d = display_col(&b.lines[l], b.cursor.col, b.tab_width);
        if d < b.left || d >= b.left + avail {
            b.left = d.saturating_sub(avail / 2);
            if d < avail.saturating_sub(1) {
                b.left = 0;
            }
        }
    }

    // ---------------------------------------------------------- git glue
    pub fn repo_here(&self) -> Option<git2::Repository> {
        let b = self.buf();
        if b.kind == BufKind::File {
            if let Some(p) = b.path.as_ref().and_then(|p| p.parent()) {
                if let Some(r) = git::open(p) {
                    return Some(r);
                }
            }
        }
        if let Some(root) = &self.proj_root {
            if let Some(r) = git::open(root) {
                return Some(r);
            }
        }
        git::open(&self.cwd)
    }

    pub fn refresh_git_info(&mut self) {
        self.git = self.repo_here().map(|r| git::info(&r));
        self.git_stamp = Instant::now();
    }

    pub fn refresh_marks(&mut self) {
        let (path, text, n, version) = {
            let b = self.buf();
            if b.kind != BufKind::File {
                return;
            }
            let Some(p) = b.path.clone() else { return };
            (p, b.text(), b.lines.len(), b.version)
        };
        let marks = self.repo_here().and_then(|r| {
            let rel = git::rel_path(&r, &path)?;
            git::line_marks(&r, &rel, text.as_bytes(), n)
        });
        let b = self.buf_mut();
        b.git_tracked = marks.is_some();
        b.git_marks = marks.unwrap_or_default();
        b.git_marks_version = version;
    }

    fn poll_run(&mut self) -> bool {
        let mut redraw = false;
        loop {
            let event = match self.run_rx.as_ref().map(Receiver::try_recv) {
                Some(Ok(event)) => event,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.finish_current_run(Err("command process ended unexpectedly".to_string()), false);
                    return true;
                }
            };
            match event {
                RunEvent::Output(line) => {
                    self.append_run_output(line);
                    redraw = true;
                }
                RunEvent::Finished(result, stopped) => {
                    self.finish_current_run(result, stopped);
                    return true;
                }
            }
        }
        redraw
    }

    fn append_run_output(&mut self, line: String) {
        let root = self.run_dir.clone().unwrap_or_else(|| self.cwd.clone());
        let meta = command_location(&line, &root);
        self.run_output.push(line.clone());
        let Some(index) = self.bufs.iter().position(|buffer| buffer.kind == BufKind::Run) else {
            return;
        };
        let buffer = &mut self.bufs[index];
        buffer.lines.push(line);
        buffer.meta.push(meta);
        let cursor = Pos::new(buffer.lines.len().saturating_sub(1), 0);
        buffer.cursor = cursor;
        for window in self.windows.iter_mut().filter(|window| window.buf_idx == index) {
            window.cursor = cursor;
        }
    }

    fn finish_current_run(&mut self, result: Result<ExitStatus, String>, stopped: bool) {
        self.run_rx = None;
        self.run_stop_tx = None;
        self.run_stopping = false;
        let root = self.run_dir.take().unwrap_or_else(|| self.cwd.clone());
        let command = self.run_command.clone();
        let output = std::mem::take(&mut self.run_output);
        let elapsed = self.run_started.take().map(|start| start.elapsed()).unwrap_or_default();
        self.finish_run(result, &output, &root, &command, elapsed, stopped);
    }

    fn update_run_elapsed(&mut self) -> bool {
        let Some(started) = self.run_started else { return false };
        let elapsed_tenths = (started.elapsed().as_millis() / 100) as u64;
        if elapsed_tenths == self.run_elapsed_tenths {
            return false;
        }
        self.run_elapsed_tenths = elapsed_tenths;

        let Some(index) = self.bufs.iter().position(|buffer| buffer.kind == BufKind::Run) else {
            return false;
        };
        if let Some(line) = self.bufs[index].lines.get_mut(2) {
            *line = format!("Command running... ({:.1}s)", elapsed_tenths as f64 / 10.0);
            true
        } else {
            false
        }
    }

    /// Idle work. Returns true when the screen needs a redraw.
    pub fn tick(&mut self) -> bool {
        let mut redraw = self.poll_run();
        redraw |= self.update_run_elapsed();
        let b = self.buf();
        if b.kind == BufKind::File && b.path.is_some() && b.git_marks_version != b.version {
            self.refresh_marks();
            redraw = true;
        }
        if self.git_stamp.elapsed().as_secs() >= 5 {
            self.refresh_git_info();
            redraw = true;
        }
        if self.disk_stamp.elapsed() >= Duration::from_millis(100) {
            self.disk_stamp = Instant::now();
            if self.revert_unmodified() > 0 {
                if matches!(self.mode, Mode::Normal) {
                    self.say("File changed on disk: buffer reloaded");
                }
                redraw = true;
            }
        }
        redraw
    }

    // ---------------------------------------------------------- key input
    pub fn on_key(&mut self, ev: KeyEvent) {
        let Some(key) = keys::parse(&ev) else { return };
        if self.completion.preview.is_some()
            && !matches!(key.name.as_str(), "TAB" | "S-TAB" | "RET" | "ESC" | "C-[")
        {
            self.completion.clear();
        }
        if matches!(self.mode, Mode::Normal)
            && self.project_tree_open
            && self.project_tree_focused
            && self.handle_project_tree_key(&key)
        {
            return;
        }
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        match mode {
            Mode::Normal => {
                if matches!(key.name.as_str(), "ESC" | "C-[") {
                    if self.vim_mode == VimMode::Insert && self.buf().kind != BufKind::Dired {
                        self.handle_vim_key(key);
                    }
                    self.cancel_key_sequences();
                } else if !self.pending.is_empty() {
                    self.normal_key(key);
                } else if self.special_key(&key.name) {
                    // Handled by special buffer (git status, log, diff, etc.)
                } else if self.vim_mode == VimMode::Normal && self.handle_vim_page_key(&key.name) {
                    // Ctrl-B / Ctrl-D are Vim page motions, not Emacs cursor/edit commands.
                } else if self.vim_mode == VimMode::Insert && is_emacs_motion_key(&key.name) {
                    self.normal_key(key);
                } else if !self.handle_vim_key(key.clone()) && !is_emacs_motion_key(&key.name) {
                    self.normal_key(key);
                }
            }
            Mode::Prompt(p) => self.prompt_key(p, key),
            Mode::ISearch(s) => self.isearch_key(s, key),
            Mode::QReplace(q) => self.qr_key(q, key),
            Mode::YesNo(y) => self.yesno_key(y, key),
        }
        if matches!(self.mode, Mode::Normal) && self.vim_mode == VimMode::Insert {
            self.update_completion();
        } else {
            self.completion.clear();
        }
    }

    fn cycle_completion(&mut self, forward: bool) {
        let len = self.completion.items.len();
        if len == 0 {
            return;
        }
        if self.completion.preview.is_some() {
            self.completion.selected = if forward {
                (self.completion.selected + 1) % len
            } else {
                (self.completion.selected + len - 1) % len
            };
        }
        self.preview_completion();
    }

    fn preview_completion(&mut self) {
        let Some((start, end)) = self.completion.range else {
            return;
        };
        let Some(candidate) = self.completion.items.get(self.completion.selected).cloned() else {
            return;
        };
        if self.completion.original.is_none() {
            let original_cursor = self.buf().cursor;
            let original = self.buf().text_in(start, end);
            self.completion.original = Some(original);
            self.completion.original_cursor = Some(original_cursor);
        }
        let buffer = self.buf_mut();
        buffer.delete(start, end);
        let preview_end = buffer.insert(start, &candidate);
        buffer.cursor = preview_end;
        self.completion.range = Some((start, preview_end));
        self.completion.preview = Some(candidate);
    }

    fn update_completion(&mut self) {
        if self.completion.preview.is_some() {
            return;
        }
        let buffer = self.buf();
        if buffer.kind != BufKind::File || !self.vim_multi_cursors.is_empty() {
            self.completion.clear();
            return;
        }
        let cursor = buffer.cursor;
        let Some(line) = buffer.lines.get(cursor.line) else {
            self.completion.clear();
            return;
        };
        let chars: Vec<char> = line.chars().collect();
        let mut start = cursor.col.min(chars.len());
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        let mut end = cursor.col.min(chars.len());
        while end < chars.len() && is_word(chars[end]) {
            end += 1;
        }
        let prefix: String = chars[start..cursor.col.min(chars.len())].iter().collect();
        if prefix.chars().count() < 2 {
            self.completion.clear();
            return;
        }
        let range = (Pos::new(cursor.line, start), Pos::new(cursor.line, end));
        let keep_selection = self.completion.prefix == prefix && self.completion.range == Some(range);
        let previous_selection = self.completion.selected;
        let current_language = self.buf().hl.name.clone();

        self.refresh_project_completion_words();
        let mut scores: HashMap<String, i32> = self
            .completion_project_words
            .iter()
            .filter(|(word, _)| {
                word.starts_with(&prefix) && word.as_str() != prefix.as_str()
            })
            .map(|(word, count)| (word.clone(), 10 + (*count).min(10) as i32))
            .collect();

        for word in crate::langs::completion_words(&self.buf().hl.name) {
            if word.starts_with(&prefix) && word != prefix.as_str() {
                scores.entry(word.to_string()).or_insert(100);
            }
        }

        for (index, candidate_buffer) in self.bufs.iter().enumerate() {
            if candidate_buffer.kind != BufKind::File
                || !candidate_buffer.hl.name.eq_ignore_ascii_case(&current_language)
            {
                continue;
            }
            let base_score = if index == self.cur { 1_000 } else { 300 };
            let from = if index == self.cur {
                cursor.line.saturating_sub(500)
            } else {
                0
            };
            let to = if index == self.cur {
                (cursor.line + 501).min(candidate_buffer.lines.len())
            } else {
                candidate_buffer.lines.len().min(2_000)
            };
            let text = candidate_buffer.lines[from..to].join("\n");
            let mut found = HashMap::<String, usize>::new();
            collect_completion_words(&text, &mut found);
            for (word, count) in found {
                if word.starts_with(&prefix) && word != prefix {
                    let score = base_score + count.min(20) as i32;
                    scores
                        .entry(word)
                        .and_modify(|existing| *existing = (*existing).max(score))
                        .or_insert(score);
                }
            }
        }

        let mut items: Vec<(String, i32)> = scores.into_iter().collect();
        items.sort_by(|(a, score_a), (b, score_b)| {
            score_b.cmp(score_a).then(a.len().cmp(&b.len())).then(a.cmp(b))
        });
        self.completion.items = items.into_iter().take(5).map(|(word, _)| word).collect();
        self.completion.selected = if keep_selection {
            previous_selection.min(self.completion.items.len().saturating_sub(1))
        } else {
            0
        };
        self.completion.range = (!self.completion.items.is_empty()).then_some(range);
        self.completion.prefix = prefix;
    }

    fn refresh_project_completion_words(&mut self) {
        let language = self.buf().hl.name.clone();
        let repo = self.repo_here();
        let root = self
            .proj_root
            .clone()
            .or_else(|| repo.as_ref().and_then(git::workdir))
            .unwrap_or_else(|| self.cwd.clone());
        let fresh = self.completion_project_root.as_ref() == Some(&root)
            && self.completion_project_language == language
            && self
                .completion_project_stamp
                .is_some_and(|stamp| stamp.elapsed() < Duration::from_secs(10));
        if fresh {
            return;
        }

        let files = repo
            .as_ref()
            .map(git::project_files)
            .unwrap_or_else(|| git::walk_files(&root, 5_000));
        let mut words = HashMap::new();
        let mut bytes_read = 0usize;
        for relative_path in files
            .iter()
            .filter(|path| is_completion_source_for(path, &language))
            .take(1_000)
        {
            let path = root.join(relative_path);
            let Ok(metadata) = std::fs::metadata(&path) else { continue };
            let size = metadata.len() as usize;
            if size > 256 * 1024 || bytes_read + size > 8 * 1024 * 1024 {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else { continue };
            bytes_read += text.len();
            collect_completion_words(&text, &mut words);
        }
        self.completion_project_root = Some(root);
        self.completion_project_stamp = Some(Instant::now());
        self.completion_project_language = language;
        self.completion_project_words = words;
    }

    fn handle_project_tree_key(&mut self, key: &Key) -> bool {
        let name = key.name.as_str();
        if !self.pending.is_empty()
            || self.vim_pending.starts_with("SPC")
            || name == "SPC"
            || name == "C-g"
            || name.starts_with("M-")
            || name.starts_with("C-x")
        {
            return false;
        }

        if self.project_tree.as_ref().is_some_and(|tree| tree.pending == "g") {
            if let Some(tree) = self.project_tree.as_mut() {
                tree.pending.clear();
                if name == "g" {
                    tree.cursor = 0;
                    self.fix_project_tree_scroll();
                }
            }
            return true;
        }

        match name {
            "j" | "<down>" | "n" => self.move_project_tree_cursor(1),
            "k" | "<up>" => self.move_project_tree_cursor(-1),
            "G" => {
                if let Some(tree) = self.project_tree.as_mut() {
                    tree.cursor = tree.entries.len().saturating_sub(1);
                }
                self.fix_project_tree_scroll();
            }
            "g" => {
                if let Some(tree) = self.project_tree.as_mut() {
                    tree.pending = "g".to_string();
                }
            }
            "C-f" | "C-d" => {
                let amount = self.screen_h.saturating_sub(2).max(1) as isize;
                self.move_project_tree_cursor(amount);
            }
            "C-b" | "C-u" => {
                let amount = self.screen_h.saturating_sub(2).max(1) as isize;
                self.move_project_tree_cursor(-amount);
            }
            "RET" | "l" | "<right>" => self.visit_project_tree_entry(),
            "h" | "<left>" => {
                if let Some(entry) = self
                    .project_tree
                    .as_ref()
                    .and_then(|tree| tree.entries.get(tree.cursor))
                    .cloned()
                {
                    if entry.is_dir && entry.expanded {
                        self.toggle_project_tree_directory(&entry.path);
                    } else if let Some(parent) = entry.path.parent()
                        && let Some(tree) = self.project_tree.as_mut()
                        && let Some(index) = tree.entries.iter().position(|item| item.path == parent)
                    {
                        tree.cursor = index;
                        self.fix_project_tree_scroll();
                    }
                }
            }
            "c" | "x" | "d" | "r" => {
                let Some(path) = self.project_tree_selected_path() else {
                    return true;
                };
                let dir = self.project_tree_current_dir().unwrap_or_else(|| {
                    self.project_tree_root().unwrap_or_else(|| self.cwd.clone())
                });
                match name {
                    "c" => self.copy_file(path),
                    "x" => self.cut_file(path),
                    "d" => self.delete_file(path),
                    "r" => {
                        let old_name = path.file_name().unwrap_or_default().to_string_lossy();
                        let label = format!("Rename {old_name} to: ");
                        self.start_prompt(Prompt::new(
                            &label,
                            PKind::DiredRename { dir, source: path },
                            None,
                        ));
                    }
                    _ => {}
                }
            }
            "p" => {
                let dest = self.project_tree_current_dir().unwrap_or_else(|| {
                    self.project_tree_root().unwrap_or_else(|| self.cwd.clone())
                });
                self.paste_file(dest);
            }
            "%" => {
                let dir = self.project_tree_current_dir().unwrap_or_else(|| {
                    self.project_tree_root().unwrap_or_else(|| self.cwd.clone())
                });
                self.start_prompt(Prompt::new(
                    "Create (name or dir/): ",
                    PKind::DiredCreate(dir),
                    None,
                ));
            }
            "R" => self.dired_refresh(),
            "q" => {
                self.project_tree_open = false;
                self.project_tree_focused = false;
            }
            "TAB" => self.project_tree_focused = false,
            _ => return false,
        }
        true
    }

    fn cancel_key_sequences(&mut self) {
        self.pending.clear();
        self.vim_pending.clear();
        self.vim_op = None;
        self.vim_replace_next = false;
        self.vim_replace_mode = false;
        self.vim_multi_cursors.clear();
        self.vim_count = 0;
        self.vim_operator_count = 0;
        self.meta_pending = false;
        self.arg = None;
        self.arg_typing = false;
        self.arg_digits = false;
        self.quote_next = false;
        self.vim_mode = VimMode::Normal;
        self.buf_mut().mark_active = false;
        self.say("Quit");
    }

    /// Handle Vim's page motions before the Emacs command dispatcher.
    /// This keeps the two keybinding systems from assigning different
    /// meanings to the same key in Normal mode.
    fn handle_vim_page_key(&mut self, name: &str) -> bool {
        match name {
            "C-b" | "C-u" => {
                self.vim_goal_col = None;
                self.scroll_up();
                true
            }
            "C-d" => {
                self.vim_goal_col = None;
                self.scroll_down();
                true
            }
            _ => false,
        }
    }

    pub fn on_paste(&mut self, s: &str) {
        self.vim_goal_col = None;
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        match &mut self.mode {
            Mode::Prompt(p) => {
                let line = s.lines().next().unwrap_or("");
                let at = p.byte();
                p.input.insert_str(at, line);
                p.cur += line.chars().count();
                p.update();
            }
            Mode::Normal => {
                let b = self.buf_mut();
                b.group += 1;
                let at = b.cursor;
                let e = b.insert(at, &s);
                b.cursor = e;
                if b.ro_attempt {
                    b.ro_attempt = false;
                    self.say("Buffer is read-only");
                }
            }
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, ev: ratatui::crossterm::event::MouseEvent) {
        use ratatui::crossterm::event::{MouseButton, MouseEventKind};
        use ratatui::layout::Rect;

        self.vim_goal_col = None;
        if self.screen_w == 0 || self.screen_h <= 1 {
            return;
        }

        if ev.kind == MouseEventKind::Up(MouseButton::Left) {
            self.mouse_resize = None;
            return;
        }

        let tree_width = self.project_tree_width();
        if tree_width > 0 && ev.column < tree_width {
            match ev.kind {
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    if let Some(tree) = self.project_tree.as_mut() {
                        let visible = self.screen_h.saturating_sub(2) as usize;
                        let max_top = tree.entries.len().saturating_sub(visible);
                        tree.top = if ev.kind == MouseEventKind::ScrollUp {
                            tree.top.saturating_sub(3)
                        } else {
                            tree.top.saturating_add(3).min(max_top)
                        };
                    }
                    return;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.project_tree_focused = true;
                    if ev.row == 0 {
                        return;
                    }
                    let Some(tree) = self.project_tree.as_mut() else { return };
                    let line = tree.top + ev.row.saturating_sub(1) as usize;
                    if line >= tree.entries.len() {
                        return;
                    }
                    tree.cursor = line;
                    let now = Instant::now();
                    let double = matches!(
                        self.last_click,
                        Some((time, window, previous_line))
                            if window == usize::MAX
                                && previous_line == line
                                && now.duration_since(time).as_millis() < 600
                    );
                    if double {
                        self.last_click = None;
                        self.visit_project_tree_entry();
                    } else {
                        self.last_click = Some((now, usize::MAX, line));
                    }
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {}
                _ => return,
            }
        }

        let window_x = if tree_width > 0 { tree_width + 1 } else { 0 };
        let win_area = Rect::new(
            window_x,
            0,
            self.screen_w.saturating_sub(window_x),
            self.screen_h.saturating_sub(1),
        );
        let mut win_rects = Vec::new();
        let mut separators = Vec::new();
        self.win_tree.compute_rects(win_area, &mut win_rects, &mut separators);

        if ev.kind == MouseEventKind::Drag(MouseButton::Left) {
            if let Some(resize) = &self.mouse_resize {
                let offset = match resize.dir {
                    SplitDir::Vertical => {
                        ev.column.saturating_sub(resize.area.x) as f32
                            / resize.area.width.saturating_sub(1).max(1) as f32
                    }
                    SplitDir::Horizontal => {
                        ev.row.saturating_sub(resize.area.y) as f32
                            / resize.area.height.max(1) as f32
                    }
                };
                self.win_tree.set_ratio_at_path(&resize.path, offset);
                return;
            }
        }

        if ev.kind == MouseEventKind::Down(MouseButton::Left) {
            let mut path = Vec::new();
            if let Some((path, dir, area)) =
                self.win_tree
                    .split_at(win_area, ev.column, ev.row, &mut path)
            {
                self.mouse_resize = Some(MouseResize { path, dir, area });
                return;
            }
            self.mouse_resize = None;
        }

        let found = win_rects.iter().find(|(_, r)| {
            ev.column >= r.x && ev.column < r.x + r.width && ev.row >= r.y && ev.row < r.y + r.height
        }).copied();

        let Some((target_win_id, rect)) = found else { return };

        if matches!(ev.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
            if target_win_id == self.active_win_id {
                for _ in 0..3 {
                    if ev.kind == MouseEventKind::ScrollUp {
                        self.previous_line();
                    } else {
                        self.next_line();
                    }
                }
            } else if let Some(window) = self.windows.iter_mut().find(|window| window.id == target_win_id) {
                let Some(buffer) = self.bufs.get(window.buf_idx) else { return };
                let max_top = buffer.lines.len().saturating_sub((rect.height as usize).saturating_sub(1));
                window.top = if ev.kind == MouseEventKind::ScrollUp {
                    window.top.saturating_sub(3)
                } else {
                    window.top.saturating_add(3).min(max_top)
                };
            }
            return;
        }

        let activates_window = matches!(
            ev.kind,
            MouseEventKind::Down(MouseButton::Left)
        );
        if activates_window && self.project_tree_open {
            self.project_tree_focused = false;
        }
        if activates_window && target_win_id != self.active_win_id {
            self.set_active_window(target_win_id);
        }

        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) if self.buf().kind == BufKind::Dired => {
                let text_h = (rect.height as usize).saturating_sub(1);
                let click_rel_y = (ev.row as usize).saturating_sub(rect.y as usize);
                if click_rel_y >= text_h {
                    return;
                }
                let line = self.buf().top + click_rel_y;
                let has_entry = matches!(self.buf().meta.get(line), Some(LineMeta::Path(_)));
                if !has_entry {
                    return;
                }
                self.buf_mut().cursor = Pos::new(line, 2);
                self.buf_mut().mark_active = false;
                let now = Instant::now();
                let double = matches!(
                    self.last_click,
                    Some((t, w, l)) if w == target_win_id && l == line
                        && now.duration_since(t).as_millis() < 600
                );
                if double {
                    self.last_click = None;
                    self.visit_line();
                } else {
                    self.last_click = Some((now, target_win_id, line));
                }
            }
            MouseEventKind::Down(MouseButton::Left)
                if matches!(self.buf().kind, BufKind::Grep | BufKind::Run) =>
            {
                let text_h = (rect.height as usize).saturating_sub(1);
                let click_rel_y = (ev.row as usize).saturating_sub(rect.y as usize);
                if click_rel_y >= text_h {
                    return;
                }
                let line = self.buf().top + click_rel_y;
                if !matches!(self.buf().meta.get(line), Some(LineMeta::Grep { .. })) {
                    return;
                }
                self.buf_mut().cursor = Pos::new(line, 0);
                self.buf_mut().mark_active = false;
                let now = Instant::now();
                let double = matches!(
                    self.last_click,
                    Some((t, w, l)) if w == target_win_id && l == line
                        && now.duration_since(t).as_millis() < 600
                );
                if double {
                    self.last_click = None;
                    self.visit_line();
                } else {
                    self.last_click = Some((now, target_win_id, line));
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.buf().kind == BufKind::Dired => {}
            MouseEventKind::Down(MouseButton::Left) => {
                let text_h = (rect.height as usize).saturating_sub(1);
                let click_rel_y = (ev.row as usize).saturating_sub(rect.y as usize);
                if click_rel_y < text_h {
                    let b = self.buf();
                    let top = b.top;
                    let target_line = (top + click_rel_y).min(b.lines.len().saturating_sub(1));
                    let gm = if b.kind == BufKind::File && b.git_tracked { 1 } else { 0 };
                    let gl = if self.line_numbers { b.lines.len().to_string().len() + 1 } else { 0 };
                    let gutter = (gm + gl) as u16;
                    let text_rel_x = ev.column.saturating_sub(rect.x + gutter) as usize;
                    let target_d = b.left + text_rel_x;
                    let line_str = b.lines.get(target_line).map(|s| s.as_str()).unwrap_or("");
                    let target_col = col_from_display(line_str, target_d, b.tab_width);
                    let new_pos = Pos::new(target_line, target_col);

                    if self.vim_mode == VimMode::Visual || self.vim_mode == VimMode::VisualLine || self.vim_mode == VimMode::VisualBlock {
                        self.buf_mut().cursor = new_pos;
                    } else {
                        self.buf_mut().cursor = new_pos;
                        self.buf_mut().mark_active = false;
                        self.buf_mut().mark = Some(new_pos);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let text_h = (rect.height as usize).saturating_sub(1);
                let click_rel_y = (ev.row as usize).saturating_sub(rect.y as usize);
                if click_rel_y < text_h {
                    let b = self.buf();
                    let top = b.top;
                    let target_line = (top + click_rel_y).min(b.lines.len().saturating_sub(1));
                    let gm = if b.kind == BufKind::File && b.git_tracked { 1 } else { 0 };
                    let gl = if self.line_numbers { b.lines.len().to_string().len() + 1 } else { 0 };
                    let gutter = (gm + gl) as u16;
                    let text_rel_x = ev.column.saturating_sub(rect.x + gutter) as usize;
                    let target_d = b.left + text_rel_x;
                    let line_str = b.lines.get(target_line).map(|s| s.as_str()).unwrap_or("");
                    let target_col = col_from_display(line_str, target_d, b.tab_width);
                    let new_pos = Pos::new(target_line, target_col);

                    if self.vim_mode == VimMode::Normal {
                        self.vim_mode = VimMode::Visual;
                        self.say("-- VISUAL --");
                        if self.buf().mark.is_none() {
                            let cur = self.buf().cursor;
                            self.buf_mut().mark = Some(cur);
                        }
                    }
                    self.buf_mut().mark_active = true;
                    self.buf_mut().cursor = new_pos;
                }
            }
            _ => {}
        }
    }

    fn normal_key(&mut self, key: Key) {
        let mut name = key.name.clone();
        let mut ch = key.ch;
        self.msg.clear();
        if matches!(name.as_str(), "ESC" | "C-[") {
            self.cancel_key_sequences();
            return;
        }
        if self.quote_next {
            self.quote_next = false;
            let c = match (name.as_str(), ch) {
                (_, Some(c)) => Some(c),
                ("RET", _) => Some('\n'),
                ("TAB", _) => Some('\t'),
                (n, _) if n.starts_with("C-") && n.len() == 3 => Some((n.as_bytes()[2] & 0x1f) as char),
                _ => None,
            };
            if let Some(c) = c {
                self.last_char = Some(c);
                self.run("self-insert-command");
            }
            return;
        }
        if self.meta_pending {
            self.meta_pending = false;
            name = keys::meta_of(&name);
            ch = None;
        }
        if name == "C-g" {
            self.pending.clear();
            self.arg = None;
            self.arg_typing = false;
            self.arg_digits = false;
            let b = self.buf_mut();
            b.mark_active = false;
            self.say("Quit");
            return;
        }
        // numeric argument digits after C-u / M-digit
        if let Some(d) = digit_of(&name, self.arg.is_some() && self.arg_typing, ch) {
            let base = if self.arg_digits { self.arg.unwrap_or(0) } else { 0 };
            self.arg = Some((base * 10 + d).min(100_000));
            self.arg_typing = true;
            self.arg_digits = true;
            self.say(format!("C-u {}", self.arg.unwrap_or(0)));
            return;
        }
        if self.pending.is_empty() && self.special_key(&name) {
            return;
        }
        self.pending.push(name.clone());
        let seq = self.pending.join(" ");
        if let Some(cmd) = self.keymap.get(&seq).copied() {
            self.pending.clear();
            self.last_char = ch;
            self.run(cmd);
        } else if self.prefixes.contains(&seq) {
            self.say(format!("{seq} -"));
        } else if self.pending.len() == 1 && ch.is_some() {
            self.pending.clear();
            self.last_char = ch;
            self.run("self-insert-command");
        } else {
            self.pending.clear();
            self.arg = None;
            self.say(format!("{seq} is undefined"));
        }
    }

    pub fn run(&mut self, name: &str) {
        let Some(f) = COMMANDS.iter().find(|c| c.0 == name).map(|c| c.1) else {
            self.say(format!("[No match] {name}"));
            return;
        };
        let n = if name == "universal-argument" { 1 } else { self.arg.take().unwrap_or(1).clamp(1, 100_000) };
        if name != "universal-argument" {
            self.arg_typing = false;
            self.arg_digits = false;
        }
        let is_ins = name == "self-insert-command";
        if !self.buf().is_undo_grouping() && !(is_ins && self.last_cmd == "self-insert-command") {
            self.buf_mut().group += 1;
        }
        if !matches!(name, "next-line" | "previous-line" | "scroll-up-command" | "scroll-down-command") {
            self.buf_mut().goal = None;
        }
        self.this_kill = false;
        let was = self.cur;
        if REPEATABLE.contains(&name) {
            for _ in 0..n {
                f(self);
                if self.buf().ro_attempt {
                    break;
                }
            }
        } else {
            f(self);
        }
        let _ = was;
        if self.cur < self.bufs.len() && self.buf().ro_attempt {
            self.buf_mut().ro_attempt = false;
            self.say("Buffer is read-only");
        }
        self.prev_kill = self.this_kill;
        self.last_cmd = name.to_string();
    }

    // ------------------------------------------------------ yes/no, prompt
    fn yesno_key(&mut self, y: YesNo, key: Key) {
        match key.name.as_str() {
            "y" | "Y" | "SPC" => match y.act {
                YAct::Quit => self.quit = true,
                YAct::KillBuf(i) => {
                    if i < self.bufs.len() {
                        self.kill_buffer_idx(i)
                    }
                }
                YAct::Restore(rel) => self.restore_file_now(&rel),
            },
            "n" | "N" | "C-g" | "ESC" => self.say("Cancelled"),
            _ => self.mode = Mode::YesNo(y),
        }
    }

    pub fn start_prompt(&mut self, p: Prompt) {
        self.mode = Mode::Prompt(p);
    }

    fn prompt_key(&mut self, mut p: Prompt, key: Key) {
        let n = key.name.as_str();
        match n {
            "C-g" | "ESC" => {
                self.say("Quit");
                return;
            }
            "RET" => {
                let value = if p.choices.is_some() && !p.matches.is_empty() {
                    p.matches[p.sel.min(p.matches.len() - 1)].clone()
                } else {
                    p.input.clone()
                };
                self.prompt_done(p.kind, value);
                return;
            }
            "d" if matches!(&p.kind, PKind::QuickFiles) => {
                self.remove_selected_quick_file(&mut p);
            }
            "TAB" => {
                if p.choices.is_some() && !p.matches.is_empty() {
                    p.sel = (p.sel + 1) % p.matches.len();
                } else {
                    self.prompt_complete(&mut p);
                }
            }
            "S-TAB" => {
                if !p.matches.is_empty() {
                    p.sel = (p.sel + p.matches.len() - 1) % p.matches.len();
                }
            }
            "DEL" => {
                if p.cur > 0 {
                    let a = byte_of(&p.input, p.cur - 1);
                    let b = p.byte();
                    p.input.replace_range(a..b, "");
                    p.cur -= 1;
                    p.update();
                }
            }
            "C-d" | "<delete>" => {
                if p.cur < p.input.chars().count() {
                    let a = p.byte();
                    let b = byte_of(&p.input, p.cur + 1);
                    p.input.replace_range(a..b, "");
                    p.update();
                }
            }
            "C-a" | "<home>" => p.cur = 0,
            "C-e" | "<end>" => p.cur = p.input.chars().count(),
            "C-b" | "<left>" => p.cur = p.cur.saturating_sub(1),
            "C-f" | "<right>" => p.cur = (p.cur + 1).min(p.input.chars().count()),
            "C-k" => {
                let a = p.byte();
                p.input.truncate(a);
                p.update();
            }
            "C-u" => {
                p.input.clear();
                p.cur = 0;
                p.update();
            }
            "M-DEL" | "C-w" => {
                let chars: Vec<char> = p.input.chars().collect();
                let mut i = p.cur;
                while i > 0 && chars[i - 1] == ' ' {
                    i -= 1;
                }
                while i > 0 && chars[i - 1] != ' ' && chars[i - 1] != '/' {
                    i -= 1;
                }
                let a = byte_of(&p.input, i);
                let b = p.byte();
                p.input.replace_range(a..b, "");
                p.cur = i;
                p.update();
            }
            "C-y" => {
                if let Some(t) = self.kill_ring.last() {
                    let line = t.lines().next().unwrap_or("").to_string();
                    let at = p.byte();
                    p.input.insert_str(at, &line);
                    p.cur += line.chars().count();
                    p.update();
                }
            }
            "C-n" | "<down>" => {
                if !p.matches.is_empty() {
                    p.sel = (p.sel + 1) % p.matches.len();
                }
            }
            "<up>" => {
                if !p.matches.is_empty() {
                    p.sel = (p.sel + p.matches.len() - 1) % p.matches.len();
                }
            }
            _ => {
                if let Some(c) = key.ch {
                    let at = p.byte();
                    p.input.insert(at, c);
                    p.cur += 1;
                    p.update();
                }
            }
        }
        self.mode = Mode::Prompt(p);
    }

    fn prompt_complete(&mut self, p: &mut Prompt) {
        if matches!(p.kind, PKind::FindFile | PKind::WriteFile) {
            let (dir_part, prefix) = match p.input.rfind('/') {
                Some(i) => (p.input[..=i].to_string(), p.input[i + 1..].to_string()),
                None => (String::new(), p.input.clone()),
            };
            let dir = if dir_part.is_empty() { self.cwd.clone() } else { self.norm_path(&dir_part) };
            let mut names: Vec<String> = std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.flatten()
                        .filter_map(|e| {
                            let n = e.file_name().to_string_lossy().to_string();
                            if !n.starts_with(&prefix) || (n.starts_with('.') && !prefix.starts_with('.')) {
                                return None;
                            }
                            Some(if e.path().is_dir() { format!("{n}/") } else { n })
                        })
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            if names.is_empty() {
                p.hint = vec!["[No match]".into()];
                return;
            }
            let lcp = common_prefix(&names);
            if lcp.chars().count() > prefix.chars().count() {
                p.input = format!("{dir_part}{lcp}");
                p.cur = p.input.chars().count();
            }
            p.hint = if names.len() == 1 { vec!["[Sole completion]".into()] } else { names };
        } else if p.choices.is_some() && !p.matches.is_empty() {
            let m = p.matches[p.sel.min(p.matches.len() - 1)].clone();
            p.input = m;
            p.cur = p.input.chars().count();
            let sel = p.sel;
            p.update();
            p.sel = sel.min(p.matches.len().saturating_sub(1));
            p.sel = 0;
        }
    }

    fn prompt_done(&mut self, kind: PKind, value: String) {
        match kind {
            PKind::FindFile => {
                if value.trim().is_empty() {
                    self.say("Quit");
                } else {
                    self.find_file(value.trim());
                }
            }
            PKind::QuickFiles => {
                if value.trim().is_empty() {
                    self.say("Quit");
                } else {
                    self.find_file(value.trim());
                }
            }
            PKind::WriteFile => {
                if !value.trim().is_empty() {
                    let p = self.norm_path(value.trim());
                    self.save_current(Some(p));
                }
            }
            PKind::Switch => {
                if let Some(i) = self.bufs.iter().position(|b| b.name == value) {
                    self.switch_to(i);
                } else {
                    let p = self.norm_path(&value);
                    if p.is_dir() {
                        self.open_dir(&p);
                    } else if p.exists() {
                        self.find_file(&value);
                    } else {
                        self.say(format!("No such path: {value}"));
                    }
                }
            }
            PKind::SelectDirectory => {
                if let Some(i) = self.bufs.iter().position(|b| b.name == value) {
                    self.switch_to(i);
                    if self.buf().kind == BufKind::Dired {
                        self.search_root = self.buf().path.clone();
                    }
                } else {
                    let path = self.norm_path(value.trim());
                    match std::fs::canonicalize(&path) {
                        Ok(path) if path.is_dir() => {
                            self.open_dir(&path);
                            if self.buf().kind == BufKind::Dired
                                && self.buf().path.as_deref() == Some(path.as_path())
                            {
                                self.search_root = Some(path.clone());
                                self.say(format!("Search base: {}", path.display()));
                            }
                        }
                        _ if path.exists() => self.find_file(value.trim()),
                        _ => self.say(format!("No such path: {value}")),
                    }
                }
            }
            PKind::WorkspaceDirectory => {
                let path = self.norm_path(value.trim());
                match std::fs::canonicalize(&path) {
                    Ok(path) if path.is_dir() => {
                        self.open_dir(&path);
                        if self.buf().kind == BufKind::Dired
                            && self.buf().path.as_deref() == Some(path.as_path())
                        {
                            self.cwd = path.clone();
                            self.proj_root = None;
                            self.search_root = Some(path.clone());
                            self.say(format!("Base directory: {}", path.display()));
                        }
                    }
                    _ => self.say(format!("Not a directory: {}", value.trim())),
                }
            }

            PKind::Kill => {
                if let Some(i) = self.bufs.iter().position(|b| b.name == value) {
                    self.kill_buffer_checked(i);
                } else {
                    self.say(format!("No buffer named {value}"));
                }
            }
            PKind::Command => { self.remember_command(value.trim()); self.run(value.trim()); },
            PKind::Goto => {
                let (l, c) = match value.trim().split_once(':') {
                    Some((l, c)) => (l, c.parse::<usize>().unwrap_or(1).saturating_sub(1)),
                    None => (value.trim(), 0),
                };
                match l.parse::<usize>() {
                    Ok(n) => self.goto_line_in_cur(n, c),
                    Err(_) => self.say("Not a line number"),
                }
            }
            PKind::Commit { amend } => self.git_commit_now(value.trim(), amend),
            PKind::Checkout => self.git_checkout_now(value.trim()),
            PKind::Run => self.run_now(&value),
            PKind::ProjFile => self.project_open(&value),
            PKind::ProjGrep => self.project_grep_now(&value),
            PKind::ProjGrepResults { root, entries } => {
                if let Some((_, LineMeta::Grep { path, line, .. })) =
                    entries.iter().find(|(label, _)| label == &value)
                {
                    if let Some(grep_index) = self
                        .bufs
                        .iter()
                        .rposition(|buffer| buffer.name == "*grep*" && buffer.kind == BufKind::Grep)
                    {
                        if let Some(result_line) = self.bufs[grep_index].meta.iter().position(|meta| {
                            matches!(meta, LineMeta::Grep { path: result_path, line: result_number, .. }
                                if result_path == path && result_number == line)
                        }) {
                            self.bufs[grep_index].cursor = Pos::new(result_line, 0);
                            self.grep_fresh = false;
                        }
                    }
                    self.visit_file_line(&root.join(path), *line);
                }
            }
            PKind::QrFrom => {
                if value.is_empty() {
                    self.say("Quit");
                } else {
                    let label = format!("Query replace {value} with: ");
                    self.start_prompt(Prompt::new(&label, PKind::QrTo(value), None));
                }
            }
            PKind::QrTo(from) => self.qr_start(from, value),
            PKind::SearchHistory => {
                if !value.is_empty() {
                    self.last_search = value.clone();
                    self.last_search_forward = true;
                    let from = self.buf().cursor;
                    if let Some((a, e)) = self.buf().find_forward(from, &value, true) {
                        self.buf_mut().cursor = a;
                        self.search_highlight = Some((self.cur, a, e));
                    }
                }
            }
            PKind::TabWidth => match value.trim().parse::<usize>() {
                Ok(n) if (1..=16).contains(&n) => {
                    self.buf_mut().tab_width = n;
                    self.say(format!("Tab width {n}"));
                }
                _ => self.say("Invalid width"),
            },
            PKind::SetLang => match Highlighter::by_name(value.trim()) {
                Some(h) => {
                    let name = h.name.clone();
                    let b = self.buf_mut();
                    b.hl = h;
                    self.say(format!("Language: {name}"));
                }
                None => self.say("Unknown language"),
            },
            PKind::SetTheme => match crate::syntax::set_theme(value.trim()) {
                Ok(name) => {
                    for buffer in &mut self.bufs {
                        buffer.hl.refresh_theme();
                    }
                    self.say(format!("Theme set: {name}"));
                }
                Err(error) => self.say(error),
            },
            PKind::DiredCreate(dir) => {
                let input = value.trim();
                if input.is_empty() {
                    self.say("Create cancelled");
                } else {
                    let is_dir = input.ends_with(std::path::MAIN_SEPARATOR) || input.ends_with('/');
                    let name = input.trim_end_matches(&['/', '\\'][..]);
                    if name.is_empty() {
                        self.say("Create failed: invalid name");
                        return;
                    }
                    let path = dir.join(name);
                    let result = if is_dir {
                        std::fs::create_dir_all(&path)
                    } else {
                        std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map(|_| ())
                    };
                    match result {
                        Ok(()) => {
                            self.say(format!("Created: {}", path.display()));
                            self.dired_refresh();
                        }
                        Err(err) => self.say(format!("Create failed: {err}")),
                    }
                }
            }
            PKind::DiredRename { dir, source } => {
                let input = value.trim();
                if input.is_empty() {
                    self.say("Rename cancelled");
                } else {
                    let destination = dir.join(input);
                    if destination == source {
                        self.say("Rename cancelled: source and destination are identical");
                    } else if path_exists(destination.as_path()) {
                        self.say(format!("Rename failed: {} already exists", destination.display()));
                    } else {
                        match std::fs::rename(&source, &destination) {
                            Ok(()) => {
                                self.say(format!("Renamed: {}", destination.display()));
                                self.dired_refresh();
                            }
                            Err(err) => self.say(format!("Rename failed: {err}")),
                        }
                    }
                }
            }
        }
    }

    pub fn kill_buffer_checked(&mut self, i: usize) {
        let b = &self.bufs[i];
        if b.modified && b.kind == BufKind::File {
            let q = format!("Buffer {} modified; kill anyway? (y or n) ", b.name);
            self.mode = Mode::YesNo(YesNo { q, act: YAct::KillBuf(i) });
        } else {
            self.kill_buffer_idx(i);
        }
    }

    // ---------------------------------------------------------- isearch
    pub fn isearch_start(&mut self, forward: bool) {
        self.last_search_forward = forward;
        self.search_highlight = None;
        let origin = self.buf().cursor;
        self.mode = Mode::ISearch(ISearch { query: String::new(), forward, origin, matched: None, failing: false });
    }

    fn isearch_step(&mut self, s: &mut ISearch, advance: bool) {
        if s.query.is_empty() {
            return;
        }
        let wrap = advance && s.failing;
        let b = &self.bufs[self.cur];
        let from = match (advance, s.matched) {
            (true, _) => b.cursor,
            (false, Some((a, e))) => {
                if s.forward {
                    a
                } else {
                    e
                }
            }
            (false, None) => s.origin,
        };
        let res = if s.forward { b.find_forward(from, &s.query, wrap) } else { b.find_backward(from, &s.query, wrap) };
        match res {
            Some((a, e)) => {
                s.matched = Some((a, e));
                s.failing = false;
                self.bufs[self.cur].cursor = if s.forward { e } else { a };
                if wrap {
                    self.say("Wrapped");
                }
            }
            None => s.failing = true,
        }
    }

    fn isearch_key(&mut self, mut s: ISearch, key: Key) {
        match key.name.as_str() {
            "C-g" => {
                self.bufs[self.cur].cursor = s.origin;
                self.search_highlight = None;
                self.say("Quit");
                return;
            }
            "RET" => {
                self.finish_isearch(&s);
                return;
            }
            "ESC" | "C-[" => {
                self.finish_isearch(&s);
                self.search_highlight = None;
                self.normal_key(key);
                return;
            }
            "C-s" | "C-r" => {
                s.forward = key.name == "C-s";
                if s.query.is_empty() {
                    s.query = self.last_search.clone();
                }
                self.isearch_step(&mut s, true);
            }
            "DEL" => {
                s.query.pop();
                s.matched = None;
                s.failing = false;
                self.bufs[self.cur].cursor = s.origin;
                self.isearch_step(&mut s, false);
            }
            "C-w" => {
                let b = &self.bufs[self.cur];
                let e = b.forward_word(b.cursor);
                s.query.push_str(&b.text_in(b.cursor, e));
                self.isearch_step(&mut s, false);
            }
            _ => {
                if let Some(c) = key.ch {
                    s.query.push(c);
                    self.isearch_step(&mut s, false);
                } else {
                    self.finish_isearch(&s);
                    self.normal_key(key);
                    return;
                }
            }
        }
        self.mode = Mode::ISearch(s);
    }

    fn finish_isearch(&mut self, s: &ISearch) {
        self.last_search_forward = s.forward;
        self.search_highlight = if s.failing {
            None
        } else {
            s.matched.map(|(start, end)| (self.cur, start, end))
        };
        if !s.query.is_empty() {
            self.last_search = s.query.clone();
            self.search_history.retain(|q| q != &s.query);
            self.search_history.insert(0, s.query.clone());
            self.search_history.truncate(50);
            let file = ow_config_dir().join("search-history");
            if let Some(parent) = file.parent() { let _ = std::fs::create_dir_all(parent); }
            let _ = std::fs::write(file, self.search_history.join("\n"));
        }
        let b = self.buf_mut();
        if b.cursor != s.origin {
            b.mark = Some(s.origin);
            b.mark_active = false;
        }
    }

    // ---------------------------------------------------- query-replace
    fn qr_start(&mut self, from: String, to: String) {
        if self.buf().read_only {
            self.say("Buffer is read-only");
            return;
        }
        self.buf_mut().group += 1;
        let q = QReplace { from, to, count: 0, cur: None };
        self.qr_next(q);
    }

    fn qr_finish(&mut self, q: QReplace) {
        if q.count > 0 {
            self.buf_mut().end_undo_group();
        }
        self.say(format!("Replaced {} occurrence{}", q.count, if q.count == 1 { "" } else { "s" }));
    }

    fn qr_next(&mut self, mut q: QReplace) {
        let cur = self.buf().cursor;
        match self.buf().find_forward(cur, &q.from, false) {
            Some((a, e)) => {
                q.cur = Some((a, e));
                self.buf_mut().cursor = e;
                self.mode = Mode::QReplace(q);
            }
            None => self.qr_finish(q),
        }
    }

    fn qr_replace_cur(&mut self, q: &mut QReplace) {
        if let Some((a, e)) = q.cur.take() {
            let b = self.buf_mut();
            if q.count == 0 {
                b.begin_undo_group();
            }
            let end = b.replace(a, e, &q.to);
            b.cursor = end;
            q.count += 1;
        }
    }

    fn qr_key(&mut self, mut q: QReplace, key: Key) {
        match key.name.as_str() {
            "y" | "SPC" => {
                self.qr_replace_cur(&mut q);
                self.qr_next(q);
            }
            "n" | "DEL" => self.qr_next(q),
            "." => {
                self.qr_replace_cur(&mut q);
                self.qr_finish(q);
            }
            "!" => {
                self.qr_replace_cur(&mut q);
                loop {
                    let cur = self.buf().cursor;
                    let Some((a, e)) = self.buf().find_forward(cur, &q.from, false) else { break };
                    let b = self.buf_mut();
                    let end = b.replace(a, e, &q.to);
                    b.cursor = end;
                    q.count += 1;
                }
                self.qr_finish(q);
            }
            "q" | "RET" | "ESC" | "C-g" => self.qr_finish(q),
            _ => self.mode = Mode::QReplace(q),
        }
    }

    // ------------------------------------------------- special buffers
    fn special_key(&mut self, key: &str) -> bool {
        let kind = self.buf().kind;
        if kind == BufKind::File {
            return false;
        }
        if kind == BufKind::Run && key == "q" && self.windows.len() > 1 {
            self.delete_window();
            return true;
        }
        match key {
            "q" => {
                let i = self.cur;
                self.kill_buffer_idx(i);
                return true;
            }
            "n" | "p" => {
                if kind == BufKind::Dired {
                    return false;
                }
                let down = key == "n";
                if kind == BufKind::GitDiff {
                    self.diff_hunk_jump(down);
                } else if down {
                    self.next_line();
                } else {
                    self.previous_line();
                }
                return true;
            }
            "RET" | "o" => {
                self.visit_line();
                return true;
            }
            _ => {}
        }
        match (kind, key) {
            (BufKind::GitStatus, "s") => self.status_stage_line(true),
            (BufKind::GitStatus, "u") => self.status_stage_line(false),
            (BufKind::GitStatus, "g") => self.git_status_refresh(),
            (BufKind::GitStatus, "d") => self.status_diff_line(),
            (BufKind::GitStatus, "D") => self.run("git-diff"),
            (BufKind::GitStatus, "c") => self.run("git-commit"),
            (BufKind::GitStatus, "C") => self.run("git-commit-amend"),
            (BufKind::GitStatus, "l") => self.run("git-log"),
            (BufKind::GitStatus, "P") => self.run("git-push"),
            (BufKind::GitStatus, "F") => self.run("git-pull"),
            (BufKind::GitStatus, "b") => self.run("git-checkout"),
            (BufKind::GitStatus, "x") => self.status_restore_line(),
            (BufKind::GitLog, "g") => self.run("git-log"),
            (BufKind::List, "d") => {
                if let Some(LineMeta::Buf(n)) = self.buf().meta.get(self.buf().cursor.line).cloned() {
                    if let Some(i) = self.bufs.iter().position(|b| b.name == n) {
                        self.kill_buffer_checked(i);
                    }
                }
            }
            (BufKind::Dired, "R") => self.dired_refresh(),
            _ => return false,
        }
        true
    }

    fn diff_hunk_jump(&mut self, down: bool) {
        let b = self.buf_mut();
        let n = b.lines.len();
        let is_h = |l: &str| l.starts_with("@@") || l.starts_with("diff --git");
        let cur = b.cursor.line;
        let found = if down { (cur + 1..n).find(|&i| is_h(&b.lines[i])) } else { (0..cur).rev().find(|&i| is_h(&b.lines[i])) };
        if let Some(i) = found {
            b.cursor = Pos::new(i, 0);
        }
    }

    pub fn visit_line(&mut self) {
        let b = self.buf();
        let Some(meta) = b.meta.get(b.cursor.line).cloned() else { return };
        let root = self.proj_root.clone().unwrap_or_else(|| self.cwd.clone());
        match meta {
            LineMeta::File { path, .. } => self.visit_file_line(&root.join(path), 1),
            LineMeta::Grep { path, line, .. } => self.visit_file_line(&root.join(path), line),
            LineMeta::Commit(id) => self.git_show_commit(&id),
            LineMeta::Path(p) => {
                if p.is_dir() {
                    self.open_dir(&p)
                } else if p.is_file() {
                    self.visit_file_line(&p, 1)
                } else {
                    self.say(format!("Dired: {} no longer exists", p.display()));
                    self.dired_refresh();
                }
            }
            LineMeta::Buf(n) => {
                if let Some(i) = self.bufs.iter().position(|b| b.name == n) {
                    self.switch_to(i);
                }
            }
            LineMeta::None => {}
        }
    }

    pub fn visit_file_line(&mut self, path: &Path, line: usize) {
        let p = self.norm_path(&path.to_string_lossy());
        match self.open_path(&p) {
            Ok(i) => {
                self.switch_to(i);
                self.goto_line_in_cur(line, 0);
                self.buf_mut().top = line.saturating_sub(self.text_h / 2);
            }
            Err(e) => self.say(e),
        }
    }

}

pub(crate) fn ow_config_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") { return PathBuf::from(path).join("ow"); }
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("." )).join(".config/ow")
}

// --------------------------------------------------------------- Dired filesystem helpers
fn copy_path_recursive(source: &Path, destination: &Path) -> io::Result<()> {
    let result = copy_path_recursive_inner(source, destination);
    if result.is_err() {
        let _ = remove_path_recursive(destination);
    }
    result
}

fn copy_path_recursive_inner(source: &Path, destination: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(source)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination)?;
        #[cfg(windows)]
        {
            if std::fs::metadata(source).map(|m| m.is_dir()).unwrap_or(false) {
                std::os::windows::fs::symlink_dir(target, destination)?;
            } else {
                std::os::windows::fs::symlink_file(target, destination)?;
            }
        }
        return Ok(());
    }
    if metadata.is_dir() {
        std::fs::create_dir(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_path_recursive_inner(&entry.path(), &destination.join(entry.file_name()))?;
        }
        std::fs::set_permissions(destination, metadata.permissions())?;
    } else {
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

fn path_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn remove_path_recursive(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

// --------------------------------------------------------------- helpers
fn digit_of(name: &str, typing: bool, ch: Option<char>) -> Option<usize> {
    if typing {
        if let Some(c) = ch {
            return c.to_digit(10).map(|d| d as usize);
        }
    }
    if let Some(r) = name.strip_prefix("M-") {
        if r.len() == 1 {
            return r.chars().next().and_then(|c| c.to_digit(10)).map(|d| d as usize);
        }
    }
    None
}

pub fn common_prefix(v: &[String]) -> String {
    let Some(first) = v.first() else { return String::new() };
    let mut end = first.chars().count();
    for s in &v[1..] {
        end = end.min(first.chars().zip(s.chars()).take_while(|(a, b)| a == b).count());
    }
    first.chars().take(end).collect()
}

pub fn display_col(line: &str, col: usize, tab: usize) -> usize {
    let mut d = 0;
    for c in line.chars().take(col) {
        d += if c == '\t' { tab - d % tab } else { c.width().unwrap_or(0) };
    }
    d
}

pub(crate) fn wrap_segments(line: &str, tab: usize, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut segments = Vec::new();
    let (mut segment_start, mut display_column) = (0, 0);

    for character in line.chars() {
        let cell_width = if character == '\t' {
            tab - display_column % tab
        } else if character.is_control() {
            1
        } else {
            UnicodeWidthChar::width(character).unwrap_or(0)
        };
        if display_column > segment_start && display_column + cell_width > segment_start + width {
            segments.push((segment_start, display_column));
            segment_start = display_column;
        }
        display_column += cell_width;
    }

    if display_column > segment_start || segments.is_empty() {
        segments.push((segment_start, display_column));
    }
    if display_column > 0 && display_column == segment_start + width {
        segments.push((display_column, display_column));
    }
    segments
}

pub fn col_from_display(line: &str, target_d: usize, tab: usize) -> usize {
    let mut d = 0;
    for (i, c) in line.chars().enumerate() {
        let cw = if c == '\t' {
            tab - d % tab
        } else if c.is_control() {
            1
        } else {
            unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
        };
        if d + cw > target_d {
            return i;
        }
        d += cw;
    }
    line.chars().count()
}

// ------------------------------------------------------------- commands
include!("commands.rs");
/// Emacs-style movement keys are deliberately not dispatched by the
/// command map: cursor navigation belongs to Vim. Non-movement Emacs
/// commands (editing, kill ring, mark, buffers, windows, Git, etc.) remain.
fn is_emacs_motion_key(name: &str) -> bool {
    matches!(
        name,
        "C-n" | "C-p"
            | "M-f" | "M-b"
            | "M-<" | "M->"
            | "C-v" | "M-v"
            | "M-m" | "M-{" | "M-}"
            | "C-<left>" | "C-<right>"
            | "C-<home>" | "C-<end>"
            | "<pageup>" | "<pagedown>"
    )
}

fn collect_completion_words(text: &str, words: &mut HashMap<String, usize>) {
    for word in text.split(|c: char| !is_word(c)) {
        if word.chars().count() >= 2 {
            *words.entry(word.to_string()).or_default() += 1;
        }
    }
}

fn is_completion_source(path: &str) -> bool {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "c" | "cc" | "cpp" | "cs" | "dart" | "ex" | "exs" | "go" | "h" | "hpp"
            | "java" | "js" | "jsx" | "kt" | "lua" | "php" | "py" | "rb" | "rs"
            | "scala" | "sh" | "swift" | "ts" | "tsx" | "vue" | "zig"
    )
}

fn is_completion_source_for(path: &str, language: &str) -> bool {
    if !is_completion_source(path) {
        return false;
    }
    Highlighter::detect(Some(Path::new(path)), "")
        .name
        .eq_ignore_ascii_case(language)
}

include!("vim.rs");


