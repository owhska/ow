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
    grouping: bool,
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
        Pos::new(
            start.line,
            start.col + text.chars().count(),
        )
    } else {
        Pos::new(
            start.line + nl,
            text.rsplit('\n').next().unwrap_or("").chars().count(),
        )
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
            grouping: false,
            undo: Vec::new(),
            redo: Vec::new(),
            saved_len: Some(0),
        }
    }

    #[cfg(test)]
    pub fn from_text(name: &str, text: &str) -> Self {
        let mut buffer = Buffer::new(name);
        buffer.set_text(text);
        buffer
    }

    /// Replace whole contents without recording undo (load / special buffers).
    pub fn set_text(&mut self, text: &str) {
        self.crlf = text.contains("\r\n");

        let normalized = if self.crlf {
            text.replace("\r\n", "\n")
        } else {
            text.to_string()
        };

        self.final_nl = normalized.ends_with('\n') || normalized.is_empty();

        let body = normalized.strip_suffix('\n').unwrap_or(&normalized);
        self.lines = body.split('\n').map(|line| line.to_string()).collect();

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

        for line in self.lines.iter().take(500) {
            match line.chars().next() {
                Some('\t') => tabs += 1,
                Some(' ') => {
                    if line.starts_with("  ") {
                        spaces += 1;
                    }
                }
                _ => {}
            }
        }

        self.use_tabs = tabs > spaces;

        if !self.use_tabs && spaces > 0 {
            // Guess indent unit from the smallest non-zero indent.
            let min_indent = self
                .lines
                .iter()
                .take(500)
                .filter(|line| line.starts_with(' '))
                .map(|line| line.len() - line.trim_start().len())
                .filter(|&indent| indent > 0)
                .min()
                .unwrap_or(4);

            self.indent_unit = if (2..=8).contains(&min_indent) {
                min_indent
            } else {
                4
            };
        }

        if let Some(file_name) = self
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
        {
            if file_name == "Makefile"
                || file_name == "makefile"
                || file_name.ends_with(".go")
                || file_name.ends_with(".mk")
            {
                self.use_tabs = true;
            }
        }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let binary = bytes.iter().take(8192).any(|&byte| byte == 0);
        let text = String::from_utf8_lossy(&bytes);

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut buffer = Buffer::new(&name);
        buffer.path = Some(path.to_path_buf());

        let first = text.lines().next().unwrap_or("").to_string();
        buffer.hl = Highlighter::detect(Some(path), &first);
        buffer.set_text(&text);
        buffer.read_only = binary;
        buffer.mtime = std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok();

        Ok(buffer)
    }

    pub fn redetect(&mut self) {
        let first = self.lines.first().cloned().unwrap_or_default();
        self.hl = Highlighter::detect(self.path.as_deref(), &first);
        self.hl.invalidate(0);
    }

    pub fn text(&self) -> String {
        let mut text = self.lines.join("\n");

        if self.final_nl {
            text.push('\n');
        }

        text
    }

    pub fn save(&mut self, path: Option<&Path>) -> io::Result<()> {
        let target = path
            .map(|path| path.to_path_buf())
            .or_else(|| self.path.clone())
            .ok_or_else(|| io::Error::other("no file name"))?;

        let mut text = self.text();

        if self.lines.len() > 1 || !self.lines[0].is_empty() {
            if !self.final_nl {
                // mg always terminates files with a newline.
                text.push('\n');
                self.final_nl = true;
            }
        }

        if self.crlf {
            text = text.replace('\n', "\r\n");
        }

        std::fs::write(&target, text)?;

        self.mtime = std::fs::metadata(&target)
            .and_then(|metadata| metadata.modified())
            .ok();

        if self.path.as_deref() != Some(target.as_path()) {
            self.path = Some(target.clone());
            self.name = target
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            self.redetect();
        }

        self.saved_len = Some(self.undo.len());
        self.modified = false;

        Ok(())
    }

    // ------------------------------------------------------------ queries
    pub fn line_len(&self, line: usize) -> usize {
        self.lines
            .get(line)
            .map(|text| text.chars().count())
            .unwrap_or(0)
    }

    pub fn clamp(&self, pos: Pos) -> Pos {
        let last_line = self.lines.len().saturating_sub(1);
        let line = pos.line.min(last_line);

        Pos::new(line, pos.col.min(self.line_len(line)))
    }

    pub fn end(&self) -> Pos {
        let last_line = self.lines.len().saturating_sub(1);
        Pos::new(last_line, self.line_len(last_line))
    }

    pub fn char_at(&self, pos: Pos) -> Option<char> {
        self.lines.get(pos.line)?.chars().nth(pos.col)
    }

    pub fn next_pos(&self, pos: Pos) -> Option<Pos> {
        if pos.col < self.line_len(pos.line) {
            Some(Pos::new(pos.line, pos.col + 1))
        } else if pos.line + 1 < self.lines.len() {
            Some(Pos::new(pos.line + 1, 0))
        } else {
            None
        }
    }

    pub fn prev_pos(&self, pos: Pos) -> Option<Pos> {
        if pos.col > 0 {
            Some(Pos::new(pos.line, pos.col - 1))
        } else if pos.line > 0 {
            Some(Pos::new(
                pos.line - 1,
                self.line_len(pos.line - 1),
            ))
        } else {
            None
        }
    }

    pub fn forward_word(&self, mut pos: Pos) -> Pos {
        loop {
            match self.char_at(pos) {
                Some(c) if is_word(c) => break,
                Some(_) => pos.col += 1,
                None => match self.next_pos(pos) {
                    Some(next) => pos = next,
                    None => return pos,
                },
            }
        }

        while let Some(c) = self.char_at(pos) {
            if is_word(c) {
                pos.col += 1;
            } else {
                break;
            }
        }

        pos
    }

    pub fn backward_word(&self, mut pos: Pos) -> Pos {
        loop {
            match self.prev_pos(pos) {
                None => return pos,
                Some(previous) => {
                    if self.char_at(previous).is_some_and(is_word) {
                        break;
                    }

                    pos = previous;
                }
            }
        }

        while let Some(previous) = self.prev_pos(pos) {
            if self.char_at(previous).is_some_and(is_word) {
                pos = previous;
            } else {
                break;
            }
        }

        pos
    }

    pub fn text_in(&self, a: Pos, b: Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let (a, b) = (self.clamp(a), self.clamp(b));

        if a.line == b.line {
            let line = &self.lines[a.line];
            return line[byte_of(line, a.col)..byte_of(line, b.col)].to_string();
        }

        let mut text = String::new();

        let first = &self.lines[a.line];
        text.push_str(&first[byte_of(first, a.col)..]);

        for line in a.line + 1..b.line {
            text.push('\n');
            text.push_str(&self.lines[line]);
        }

        text.push('\n');

        let last = &self.lines[b.line];
        text.push_str(&last[..byte_of(last, b.col)]);

        text
    }

    pub fn indent_of(&self, line: usize) -> String {
        self.lines[line]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    // ------------------------------------------------------------ editing
    fn raw_replace(&mut self, start: Pos, end: Pos, text: &str) -> Pos {
        let head = {
            let line = &self.lines[start.line];
            line[..byte_of(line, start.col)].to_string()
        };

        let tail = {
            let line = &self.lines[end.line];
            line[byte_of(line, end.col)..].to_string()
        };

        let mut new = String::with_capacity(head.len() + text.len() + tail.len());
        new.push_str(&head);
        new.push_str(text);
        new.push_str(&tail);

        let new_lines: Vec<String> = new.split('\n').map(|line| line.to_string()).collect();

        self.lines.splice(start.line..=end.line, new_lines);
        self.version += 1;
        self.modified = true;
        self.hl.invalidate(start.line);

        end_after(start, text)
    }

    /// The single primitive all edits go through. Returns the end of the
    /// inserted text.
    ///
    /// Every successful primitive edit receives its own undo group unless an
    /// explicit transaction is active. Insert sessions and rectangular edits
    /// use transactions so a logical user action is undone atomically.
    pub fn replace(&mut self, start: Pos, end: Pos, text: &str) -> Pos {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

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
        let end_pos = self.raw_replace(start, end, text);

        if self.saved_len.is_some_and(|saved| self.undo.len() < saved) {
            self.saved_len = None;
        }

        if !self.grouping {
            self.group += 1;
        }

        self.undo.push(Edit {
            start,
            removed,
            inserted: text.to_string(),
            group: self.group,
            before,
        });

        self.redo.clear();

        end_pos
    }

    /// Group a sequence of primitive edits into one undo step.
    pub fn begin_undo_group(&mut self) {
        if !self.grouping {
            self.group += 1;
            self.grouping = true;
        }
    }

    pub fn end_undo_group(&mut self) {
        self.grouping = false;
    }

    pub fn is_undo_grouping(&self) -> bool {
        self.grouping
    }

    pub fn insert(&mut self, at: Pos, text: &str) -> Pos {
        self.replace(at, at, text)
    }

    pub fn delete(&mut self, a: Pos, b: Pos) {
        self.replace(a, b, "");
    }

    pub fn undo(&mut self) -> bool {
        let Some(group) = self.undo.last().map(|edit| edit.group) else {
            return false;
        };

        // Collect every edit belonging to this undo group. With the current
        // one-edit-per-group policy, this is normally exactly one edit, but
        // the loop remains correct if grouped edits are ever reintroduced.
        let mut batch = Vec::new();

        while self
            .undo
            .last()
            .is_some_and(|edit| edit.group == group)
        {
            batch.push(self.undo.pop().unwrap());
        }

        // `batch` is newest-first. Undo must apply it newest-first.
        for edit in &batch {
            let end = end_after(edit.start, &edit.inserted);
            self.raw_replace(edit.start, end, &edit.removed);
        }

        // Redo must replay the edits in their original chronological order.
        for edit in batch.into_iter().rev() {
            self.redo.push(edit);
        }

        // The cursor returns to its position before the oldest edit in the
        // undone group.
        if let Some(oldest) = self.redo.last() {
            self.cursor = self.clamp(oldest.before);
        }

        self.modified = self.saved_len != Some(self.undo.len());

        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(group) = self.redo.last().map(|edit| edit.group) else {
            return false;
        };

        // Collect every edit belonging to this redo group. `redo` stores edits
        // in chronological order, but popping yields newest-first order.
        let mut batch = Vec::new();

        while self
            .redo
            .last()
            .is_some_and(|edit| edit.group == group)
        {
            batch.push(self.redo.pop().unwrap());
        }

        let mut cursor = self.cursor;

        // Reapply in chronological order: the oldest edit first.
        for edit in batch.into_iter().rev() {
            let end = end_after(edit.start, &edit.removed);
            cursor = self.raw_replace(edit.start, end, &edit.inserted);

            self.undo.push(edit);
        }

        self.cursor = self.clamp(cursor);
        self.modified = self.saved_len != Some(self.undo.len());

        true
    }

    // ---------------------------------------------------------- searching
    pub fn smart_case(query: &str) -> bool {
        query.chars().any(|c| c.is_uppercase())
    }

    fn fold(c: char, case_sensitive: bool) -> char {
        if case_sensitive {
            c
        } else {
            c.to_lowercase().next().unwrap_or(c)
        }
    }

    fn find_in_line(
        &self,
        line: usize,
        query: &[char],
        case_sensitive: bool,
        from: usize,
        backward_before: Option<usize>,
    ) -> Option<usize> {
        let chars: Vec<char> = self.lines[line]
            .chars()
            .map(|c| Self::fold(c, case_sensitive))
            .collect();

        if query.is_empty() || chars.len() < query.len() {
            return None;
        }

        let last = chars.len() - query.len();

        match backward_before {
            None => (from..=last).find(|&index| chars[index..index + query.len()] == *query),
            Some(limit) => (0..=last.min(limit))
                .rev()
                .find(|&index| chars[index..index + query.len()] == *query),
        }
    }

    /// First match starting at or after `from`. Returns `(start, end)`.
    pub fn find_forward(&self, from: Pos, query: &str, wrap: bool) -> Option<(Pos, Pos)> {
        let case_sensitive = Self::smart_case(query);
        let query: Vec<char> = query
            .chars()
            .map(|c| Self::fold(c, case_sensitive))
            .collect();

        let line_count = self.lines.len();
        let from = self.clamp(from);

        for offset in 0..=line_count {
            let line = from.line + offset;

            if line >= line_count && !wrap {
                break;
            }

            let line = line % line_count;
            let start_column = if offset == 0 { from.col } else { 0 };

            if let Some(column) =
                self.find_in_line(line, &query, case_sensitive, start_column, None)
            {
                return Some((
                    Pos::new(line, column),
                    Pos::new(line, column + query.len()),
                ));
            }
        }

        None
    }

    /// Last match whose start is strictly before `from`.
    pub fn find_backward(&self, from: Pos, query: &str, wrap: bool) -> Option<(Pos, Pos)> {
        let case_sensitive = Self::smart_case(query);
        let query: Vec<char> = query
            .chars()
            .map(|c| Self::fold(c, case_sensitive))
            .collect();

        let line_count = self.lines.len();
        let from = self.clamp(from);

        for offset in 0..=line_count {
            if offset > from.line && !wrap {
                break;
            }

            let line = (from.line + line_count - (offset % line_count)) % line_count;

            let limit = if offset == 0 {
                if from.col == 0 {
                    continue;
                }

                from.col - 1
            } else {
                usize::MAX
            };

            if let Some(column) =
                self.find_in_line(line, &query, case_sensitive, 0, Some(limit))
            {
                return Some((
                    Pos::new(line, column),
                    Pos::new(line, column + query.len()),
                ));
            }
        }

        None
    }

    // ------------------------------------------------------------ styling
    pub fn line_styles(&mut self, line: usize) -> Option<Vec<Style>> {
        self.hl.line_styles(&self.lines, line)
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
        let mut buffer = Buffer::from_text("t", "hello\nworld\n");

        buffer.insert(Pos::new(0, 5), ", there\nnew");
        assert_eq!(
            buffer.lines,
            vec!["hello, there", "new", "world"]
        );

        buffer.delete(Pos::new(0, 0), Pos::new(1, 1));
        assert_eq!(buffer.lines, vec!["ew", "world"]);

        assert!(buffer.undo());
        assert_eq!(
            buffer.lines,
            vec!["hello, there", "new", "world"]
        );

        assert!(buffer.undo());
        assert_eq!(buffer.lines, vec!["hello", "world"]);

        assert!(!buffer.modified);
        assert!(buffer.redo());
        assert_eq!(
            buffer.lines,
            vec!["hello, there", "new", "world"]
        );

        assert_eq!(buffer.text(), "hello, there\nnew\nworld\n");
    }

    #[test]
    fn delete_then_undo_restores_text() {
        let mut buffer = Buffer::from_text("t", "hello\n");
        buffer.insert(Pos::new(0, 5), "X");
        assert_eq!(buffer.lines[0], "helloX");

        buffer.delete(Pos::new(0, 5), Pos::new(0, 6));
        assert_eq!(buffer.lines[0], "hello");

        assert!(buffer.undo());
        assert_eq!(buffer.lines[0], "helloX");
    }

    #[test]
    fn delete_empty_line_and_undo() {
        let mut buffer = Buffer::from_text("t", "a\n\nb\n");
        buffer.delete(Pos::new(1, 0), Pos::new(2, 0));
        assert_eq!(buffer.lines, vec!["a", "b"]);

        assert!(buffer.undo());
        assert_eq!(buffer.lines, vec!["a", "", "b"]);
    }

    #[test]
    fn unicode_cols() {
        let mut buffer = Buffer::from_text("t", "açaí\n");
        buffer.insert(Pos::new(0, 2), "X");

        assert_eq!(buffer.lines[0], "açXaí");
        assert_eq!(
            buffer.text_in(Pos::new(0, 1), Pos::new(0, 4)),
            "çXa"
        );
    }

    #[test]
    fn search() {
        let buffer = Buffer::from_text("t", "foo Bar\nbaz foo\n");

        let (start, end) = buffer
            .find_forward(Pos::new(0, 1), "foo", false)
            .unwrap();
        assert_eq!((start, end), (Pos::new(1, 4), Pos::new(1, 7)));

        assert!(buffer
            .find_forward(Pos::new(1, 5), "foo", false)
            .is_none());
        assert!(buffer
            .find_forward(Pos::new(1, 5), "foo", true)
            .is_some());

        let (start, _) = buffer
            .find_backward(Pos::new(1, 4), "foo", false)
            .unwrap();
        assert_eq!(start, Pos::new(0, 0));

        assert!(buffer
            .find_forward(Pos::new(0, 0), "bar", false)
            .is_some());
        assert!(buffer
            .find_forward(Pos::new(0, 0), "bAr", false)
            .is_none());
    }

    #[test]
    fn words() {
        let buffer = Buffer::from_text("t", "foo  bar_baz\nqux\n");

        assert_eq!(
            buffer.forward_word(Pos::new(0, 0)),
            Pos::new(0, 3)
        );
        assert_eq!(
            buffer.forward_word(Pos::new(0, 3)),
            Pos::new(0, 12)
        );
        assert_eq!(
            buffer.forward_word(Pos::new(0, 12)),
            Pos::new(1, 3)
        );
        assert_eq!(
            buffer.backward_word(Pos::new(0, 12)),
            Pos::new(0, 5)
        );
        assert_eq!(
            buffer.backward_word(Pos::new(1, 0)),
            Pos::new(0, 5)
        );
    }

    #[test]
    fn crlf_roundtrip() {
        let mut buffer = Buffer::from_text("t", "a\r\nb\r\n");
        assert!(buffer.crlf);
        assert_eq!(buffer.lines, vec!["a", "b"]);

        let directory = std::env::temp_dir().join(format!(
            "mgrs-{}",
            std::process::id()
        ));

        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("x.txt");

        buffer.save(Some(&path)).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"a\r\nb\r\n"
        );

        std::fs::remove_dir_all(&directory).ok();
    }
}
