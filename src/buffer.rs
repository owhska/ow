//! Text buffer: lines of text, cursor/mark, undo/redo, file I/O.
use crate::syntax::Highlighter;
use ratatui::style::Style;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}
impl Pos {
    pub fn new(line: usize, col: usize) -> Self {
        Pos { line, col }
    }
}

#[derive(Clone, Debug)]
pub enum LineMeta {
    None,
    File { path: String, staged: bool },
    Commit(String),
    Grep { path: String, line: usize },
    Path(PathBuf),
    Buf(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufKind {
    File,
    GitStatus,
    GitLog,
    GitDiff,
    GitBlame,
    Grep,
    Dired,
    List,
    Help,
}

/// Git gutter marks (0 = none).
pub const MARK_ADDED: u8 = 1;
pub const MARK_MODIFIED: u8 = 2;
pub const MARK_DELETED: u8 = 3;

struct Edit {
    start: Pos,
    removed: String,
    inserted: String,
    group: u64,
    before: Pos,
}

pub struct Buffer {
    pub name: String,
    pub path: Option<PathBuf>,
    pub lines: Vec<String>,
    pub modified: bool,
    pub cursor: Pos,
    pub mark: Option<Pos>,
    pub mark_active: bool,
    pub top: usize,
    pub left: usize,
    pub goal: Option<usize>,
    pub kind: BufKind,
    pub read_only: bool,
    pub ro_attempt: bool,
    pub version: u64,
    pub crlf: bool,
    pub final_nl: bool,
    pub mtime: Option<SystemTime>,
    pub hl: Highlighter,
    pub meta: Vec<LineMeta>,
    pub use_tabs: bool,
    pub tab_width: usize,
    pub indent_unit: usize,
    pub git_marks: Vec<u8>,
    pub git_marks_version: u64,
    pub git_tracked: bool,
    pub group: u64,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    saved_len: Option<usize>,
}

pub fn byte_of(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn end_after(start: Pos, text: &str) -> Pos {
    let nl = text.matches('\n').count();
    if nl == 0 {
        Pos::new(start.line, start.col + text.chars().count())
    } else {
        Pos::new(start.line + nl, text.rsplit('\n').next().unwrap_or("").chars().count())
    }
}

impl Buffer {
    pub fn new(name: &str) -> Self {
        Buffer {
            name: name.to_string(),
            path: None,
            lines: vec![String::new()],
            modified: false,
            cursor: Pos::default(),
            mark: None,
            mark_active: false,
            top: 0,
            left: 0,
            goal: None,
            kind: BufKind::File,
            read_only: false,
            ro_attempt: false,
            version: 0,
            crlf: false,
            final_nl: true,
            mtime: None,
            hl: Highlighter::plain(),
            meta: Vec::new(),
            use_tabs: false,
            tab_width: 8,
            indent_unit: 4,
            git_marks: Vec::new(),
            git_marks_version: u64::MAX,
            git_tracked: false,
            group: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            saved_len: Some(0),
        }
    }

    #[cfg(test)]
    pub fn from_text(name: &str, text: &str) -> Self {
        let mut b = Buffer::new(name);
        b.set_text(text);
        b
    }

    /// Replace whole contents without recording undo (load / special buffers).
    pub fn set_text(&mut self, text: &str) {
        self.crlf = text.contains("\r\n");
        let norm = if self.crlf { text.replace("\r\n", "\n") } else { text.to_string() };
        self.final_nl = norm.ends_with('\n') || norm.is_empty();
        let body = norm.strip_suffix('\n').unwrap_or(&norm);
        self.lines = body.split('\n').map(|s| s.to_string()).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.undo.clear();
        self.redo.clear();
        self.saved_len = Some(0);
        self.modified = false;
        self.version += 1;
        self.hl.invalidate(0);
        self.cursor = self.clamp(self.cursor);
        self.detect_indent();
    }

    fn detect_indent(&mut self) {
        let (mut tabs, mut spaces) = (0, 0);
        for l in self.lines.iter().take(500) {
            match l.chars().next() {
                Some('\t') => tabs += 1,
                Some(' ') => {
                    if l.starts_with("  ") {
                        spaces += 1
                    }
                }
                _ => {}
            }
        }
        self.use_tabs = tabs > spaces;
        if !self.use_tabs && spaces > 0 {
            // guess indent unit from the smallest non-zero indent
            let m = self
                .lines
                .iter()
                .take(500)
                .filter(|l| l.starts_with(' '))
                .map(|l| l.len() - l.trim_start().len())
                .filter(|&n| n > 0 && !self.lines.is_empty())
                .min()
                .unwrap_or(4);
            self.indent_unit = if (2..=8).contains(&m) { m } else { 4 };
        }
        if let Some(n) = self.path.as_ref().and_then(|p| p.file_name()).and_then(|s| s.to_str()) {
            if n == "Makefile" || n == "makefile" || n.ends_with(".go") || n.ends_with(".mk") {
                self.use_tabs = true;
            }
        }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let binary = bytes.iter().take(8192).any(|&b| b == 0);
        let text = String::from_utf8_lossy(&bytes);
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut b = Buffer::new(&name);
        b.path = Some(path.to_path_buf());
        let first = text.lines().next().unwrap_or("").to_string();
        b.hl = Highlighter::detect(Some(path), &first);
        b.set_text(&text);
        b.read_only = binary;
        b.mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        Ok(b)
    }

    pub fn redetect(&mut self) {
        let first = self.lines.first().cloned().unwrap_or_default();
        self.hl = Highlighter::detect(self.path.as_deref(), &first);
        self.hl.invalidate(0);
    }

    pub fn text(&self) -> String {
        let mut s = self.lines.join("\n");
        if self.final_nl {
            s.push('\n');
        }
        s
    }

    pub fn save(&mut self, path: Option<&Path>) -> io::Result<()> {
        let target = path.map(|p| p.to_path_buf()).or_else(|| self.path.clone()).ok_or_else(|| io::Error::other("no file name"))?;
        let mut text = self.text();
        if self.lines.len() > 1 || !self.lines[0].is_empty() {
            if !self.final_nl {
                // mg always terminates files with a newline
                text.push('\n');
                self.final_nl = true;
            }
        }
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        std::fs::write(&target, text)?;
        self.mtime = std::fs::metadata(&target).and_then(|m| m.modified()).ok();
        if self.path.as_deref() != Some(target.as_path()) {
            self.path = Some(target.clone());
            self.name = target.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            self.redetect();
        }
        self.saved_len = Some(self.undo.len());
        self.modified = false;
        Ok(())
    }

    // ------------------------------------------------------------ queries
    pub fn line_len(&self, i: usize) -> usize {
        self.lines.get(i).map(|l| l.chars().count()).unwrap_or(0)
    }

    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos::new(line, p.col.min(self.line_len(line)))
    }

    pub fn end(&self) -> Pos {
        let l = self.lines.len() - 1;
        Pos::new(l, self.line_len(l))
    }

    pub fn char_at(&self, p: Pos) -> Option<char> {
        self.lines.get(p.line)?.chars().nth(p.col)
    }

    pub fn next_pos(&self, p: Pos) -> Option<Pos> {
        if p.col < self.line_len(p.line) {
            Some(Pos::new(p.line, p.col + 1))
        } else if p.line + 1 < self.lines.len() {
            Some(Pos::new(p.line + 1, 0))
        } else {
            None
        }
    }

    pub fn prev_pos(&self, p: Pos) -> Option<Pos> {
        if p.col > 0 {
            Some(Pos::new(p.line, p.col - 1))
        } else if p.line > 0 {
            Some(Pos::new(p.line - 1, self.line_len(p.line - 1)))
        } else {
            None
        }
    }

    pub fn forward_word(&self, mut p: Pos) -> Pos {
        loop {
            match self.char_at(p) {
                Some(c) if is_word(c) => break,
                Some(_) => p.col += 1,
                None => match self.next_pos(p) {
                    Some(q) => p = q,
                    None => return p,
                },
            }
        }
        while let Some(c) = self.char_at(p) {
            if is_word(c) {
                p.col += 1
            } else {
                break;
            }
        }
        p
    }

    pub fn backward_word(&self, mut p: Pos) -> Pos {
        loop {
            match self.prev_pos(p) {
                None => return p,
                Some(q) => {
                    if self.char_at(q).is_some_and(is_word) {
                        break;
                    }
                    p = q;
                }
            }
        }
        while let Some(q) = self.prev_pos(p) {
            if self.char_at(q).is_some_and(is_word) {
                p = q
            } else {
                break;
            }
        }
        p
    }

    pub fn text_in(&self, a: Pos, b: Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let (a, b) = (self.clamp(a), self.clamp(b));
        if a.line == b.line {
            let l = &self.lines[a.line];
            return l[byte_of(l, a.col)..byte_of(l, b.col)].to_string();
        }
        let mut s = String::new();
        let l = &self.lines[a.line];
        s.push_str(&l[byte_of(l, a.col)..]);
        for i in a.line + 1..b.line {
            s.push('\n');
            s.push_str(&self.lines[i]);
        }
        s.push('\n');
        let l = &self.lines[b.line];
        s.push_str(&l[..byte_of(l, b.col)]);
        s
    }

    pub fn indent_of(&self, line: usize) -> String {
        self.lines[line].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
    }

    // ------------------------------------------------------------ editing
    fn raw_replace(&mut self, start: Pos, end: Pos, text: &str) -> Pos {
        let head = {
            let l = &self.lines[start.line];
            l[..byte_of(l, start.col)].to_string()
        };
        let tail = {
            let l = &self.lines[end.line];
            l[byte_of(l, end.col)..].to_string()
        };
        let mut new = String::with_capacity(head.len() + text.len() + tail.len());
        new.push_str(&head);
        new.push_str(text);
        new.push_str(&tail);
        let new_lines: Vec<String> = new.split('\n').map(|s| s.to_string()).collect();
        self.lines.splice(start.line..=end.line, new_lines);
        self.version += 1;
        self.modified = true;
        self.hl.invalidate(start.line);
        end_after(start, text)
    }

    /// The single primitive all edits go through. Returns the end of the inserted text.
    pub fn replace(&mut self, start: Pos, end: Pos, text: &str) -> Pos {
        let (start, end) = if start <= end { (start, end) } else { (end, start) };
        let (start, end) = (self.clamp(start), self.clamp(end));
        if self.read_only {
            self.ro_attempt = true;
            return start;
        }
        let removed = self.text_in(start, end);
        if removed.is_empty() && text.is_empty() {
            return start;
        }
        let before = self.cursor;
        let e = self.raw_replace(start, end, text);
        if self.saved_len.is_some_and(|s| self.undo.len() < s) {
            self.saved_len = None;
        }
        self.undo.push(Edit { start, removed, inserted: text.to_string(), group: self.group, before });
        self.redo.clear();
        e
    }

    pub fn insert(&mut self, at: Pos, text: &str) -> Pos {
        self.replace(at, at, text)
    }

    pub fn delete(&mut self, a: Pos, b: Pos) {
        self.replace(a, b, "");
    }

    pub fn undo(&mut self) -> bool {
        let Some(g) = self.undo.last().map(|e| e.group) else { return false };
        let mut before = self.cursor;
        while self.undo.last().is_some_and(|e| e.group == g) {
            let e = self.undo.pop().unwrap();
            let end = end_after(e.start, &e.inserted);
            self.raw_replace(e.start, end, &e.removed);
            before = e.before;
            self.redo.push(e);
        }
        self.cursor = self.clamp(before);
        self.modified = self.saved_len != Some(self.undo.len());
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(g) = self.redo.last().map(|e| e.group) else { return false };
        let mut cur = self.cursor;
        while self.redo.last().is_some_and(|e| e.group == g) {
            let e = self.redo.pop().unwrap();
            let end = end_after(e.start, &e.removed);
            cur = self.raw_replace(e.start, end, &e.inserted);
            self.undo.push(e);
        }
        self.cursor = self.clamp(cur);
        self.modified = self.saved_len != Some(self.undo.len());
        true
    }

    // ---------------------------------------------------------- searching
    pub fn smart_case(q: &str) -> bool {
        q.chars().any(|c| c.is_uppercase())
    }

    fn fold(c: char, cs: bool) -> char {
        if cs { c } else { c.to_lowercase().next().unwrap_or(c) }
    }

    fn find_in_line(&self, line: usize, q: &[char], cs: bool, from: usize, backward_before: Option<usize>) -> Option<usize> {
        let chars: Vec<char> = self.lines[line].chars().map(|c| Self::fold(c, cs)).collect();
        if q.is_empty() || chars.len() < q.len() {
            return None;
        }
        let last = chars.len() - q.len();
        match backward_before {
            None => (from..=last).find(|&i| chars[i..i + q.len()] == *q),
            Some(lim) => (0..=last.min(lim)).rev().find(|&i| chars[i..i + q.len()] == *q),
        }
    }

    /// First match starting at or after `from`. Returns (start, end).
    pub fn find_forward(&self, from: Pos, query: &str, wrap: bool) -> Option<(Pos, Pos)> {
        let cs = Self::smart_case(query);
        let q: Vec<char> = query.chars().map(|c| Self::fold(c, cs)).collect();
        let n = self.lines.len();
        let from = self.clamp(from);
        for k in 0..=n {
            let line = from.line + k;
            if line >= n && !wrap {
                break;
            }
            let line = line % n;
            let start_col = if k == 0 { from.col } else { 0 };
            if k == n && start_col == 0 {
                break;
            }
            if let Some(c) = self.find_in_line(line, &q, cs, start_col, None) {
                return Some((Pos::new(line, c), Pos::new(line, c + q.len())));
            }
        }
        None
    }

    /// Last match whose start is strictly before `from`.
    pub fn find_backward(&self, from: Pos, query: &str, wrap: bool) -> Option<(Pos, Pos)> {
        let cs = Self::smart_case(query);
        let q: Vec<char> = query.chars().map(|c| Self::fold(c, cs)).collect();
        let n = self.lines.len();
        let from = self.clamp(from);
        for k in 0..=n {
            if k > from.line && !wrap {
                break;
            }
            let line = (from.line + n - (k % n)) % n;
            let lim = if k == 0 {
                if from.col == 0 {
                    continue;
                }
                from.col - 1
            } else {
                usize::MAX
            };
            if let Some(c) = self.find_in_line(line, &q, cs, 0, Some(lim)) {
                return Some((Pos::new(line, c), Pos::new(line, c + q.len())));
            }
        }
        None
    }

    // ------------------------------------------------------------ styling
    pub fn line_styles(&mut self, i: usize) -> Option<Vec<Style>> {
        self.hl.line_styles(&self.lines, i)
    }

    pub fn set_special(&mut self, text: &str, meta: Vec<LineMeta>) {
        self.read_only = false;
        self.set_text(text);
        self.read_only = true;
        self.meta = meta;
        self.modified = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_and_undo() {
        let mut b = Buffer::from_text("t", "hello\nworld\n");
        b.group = 1;
        let e = b.insert(Pos::new(0, 5), ", there\nnew");
        assert_eq!(b.lines, vec!["hello, there", "new", "world"]);
        assert_eq!(e, Pos::new(1, 3));
        b.group = 2;
        b.delete(Pos::new(0, 0), Pos::new(1, 1));
        assert_eq!(b.lines, vec!["ew", "world"]);
        assert!(b.undo());
        assert_eq!(b.lines, vec!["hello, there", "new", "world"]);
        assert!(b.undo());
        assert_eq!(b.lines, vec!["hello", "world"]);
        assert!(!b.modified);
        assert!(b.redo());
        assert_eq!(b.lines, vec!["hello, there", "new", "world"]);
        assert_eq!(b.text(), "hello, there\nnew\nworld\n");
    }

    #[test]
    fn unicode_cols() {
        let mut b = Buffer::from_text("t", "açaí\n");
        b.insert(Pos::new(0, 2), "X");
        assert_eq!(b.lines[0], "açXaí");
        assert_eq!(b.text_in(Pos::new(0, 1), Pos::new(0, 4)), "çXa");
    }

    #[test]
    fn search() {
        let b = Buffer::from_text("t", "foo Bar\nbaz foo\n");
        let (s, e) = b.find_forward(Pos::new(0, 1), "foo", false).unwrap();
        assert_eq!((s, e), (Pos::new(1, 4), Pos::new(1, 7)));
        assert!(b.find_forward(Pos::new(1, 5), "foo", false).is_none());
        assert!(b.find_forward(Pos::new(1, 5), "foo", true).is_some());
        let (s, _) = b.find_backward(Pos::new(1, 4), "foo", false).unwrap();
        assert_eq!(s, Pos::new(0, 0));
        assert!(b.find_forward(Pos::new(0, 0), "bar", false).is_some());
        assert!(b.find_forward(Pos::new(0, 0), "bAr", false).is_none());
    }

    #[test]
    fn words() {
        let b = Buffer::from_text("t", "foo  bar_baz\nqux\n");
        assert_eq!(b.forward_word(Pos::new(0, 0)), Pos::new(0, 3));
        assert_eq!(b.forward_word(Pos::new(0, 3)), Pos::new(0, 12));
        assert_eq!(b.forward_word(Pos::new(0, 12)), Pos::new(1, 3));
        assert_eq!(b.backward_word(Pos::new(0, 12)), Pos::new(0, 5));
        assert_eq!(b.backward_word(Pos::new(1, 0)), Pos::new(0, 5));
    }

    #[test]
    fn crlf_roundtrip() {
        let mut b = Buffer::from_text("t", "a\r\nb\r\n");
        assert!(b.crlf);
        assert_eq!(b.lines, vec!["a", "b"]);
        let dir = std::env::temp_dir().join(format!("mgrs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.txt");
        b.save(Some(&p)).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"a\r\nb\r\n");
        std::fs::remove_dir_all(&dir).ok();
    }
}
