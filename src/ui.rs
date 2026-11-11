//! Rendering with ratatui: text area, git gutter, mode line, echo area.
use crate::buffer::*;
use crate::editor::*;
use crate::syntax::palette::*;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}
fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

fn special_styles(kind: BufKind, line: &str, meta: Option<&LineMeta>) -> Option<Vec<Style>> {
    let n = line.chars().count();
    let mut v = vec![Style::default(); n];
    let mut paint = |a: usize, b: usize, s: Style| {
        for x in v.iter_mut().take(b.min(n)).skip(a) {
            *x = s;
        }
    };
    match kind {
        BufKind::GitStatus => {
            if line.starts_with("Repository:") || line.starts_with("Branch:") {
                paint(0, 11, fg(c(TYPE)).add_modifier(Modifier::BOLD));
            } else if line.starts_with("Staged") || line.starts_with("Unstaged") || line.starts_with("Untracked") {
                paint(0, n, bold());
            } else if line.starts_with(" s stage") {
                paint(0, n, fg(c(DIM)));
            } else if let Some(LineMeta::File { staged, .. }) = meta {
                let col = if *staged {
                    c(INFO)
                } else if line.trim_start().starts_with('?') {
                    c(ERROR)
                } else {
                    c(WARN)
                };
                paint(0, n, fg(col));
            }
        }
        BufKind::GitLog => {
            paint(0, 7, fg(c(WARN)));
            paint(9, 19, fg(c(TYPE)));
            paint(21, 30, fg(c(DIM)));
            paint(32, 46, fg(c(INFO)));
        }
        BufKind::Grep => match meta {
            Some(LineMeta::Grep { path, line: ln }) => {
                let pl = path.chars().count();
                paint(0, pl, fg(c(TYPE)));
                paint(pl, pl + 1 + ln.to_string().len() + 1, fg(c(INFO)));
            }
            _ => paint(0, n, bold()),
        },
        BufKind::GitBlame => {
            paint(0, 7, fg(c(WARN)));
            paint(8, 22, fg(c(INFO)));
            paint(23, 36, fg(c(DIM)));
        }
        BufKind::Dired => {
            if meta.is_none() {
                paint(0, n, bold());
            } else if line.ends_with('/') {
                paint(0, n, fg(c(FUNC)).add_modifier(Modifier::BOLD));
            }
        }
        BufKind::List | BufKind::Help => {
            if !line.starts_with(' ') && !line.is_empty() || line.starts_with(" MR") {
                paint(0, n, bold());
            }
        }
        _ => return None,
    }
    Some(v)
}

#[allow(clippy::too_many_arguments)]
fn paint_line(
    buf: &mut ratatui::buffer::Buffer,
    x0: u16,
    y: u16,
    width: usize,
    left: usize,
    tab: usize,
    line: &str,
    styles: Option<&[Style]>,
    overlays: &[(usize, usize, Style)],
) {
    let mut d = 0usize;
    for (i, c) in line.chars().enumerate() {
        let cw = if c == '\t' { tab - d % tab } else if c.is_control() { 1 } else { c.width().unwrap_or(0) };
        let mut st = styles.and_then(|s| s.get(i)).copied().unwrap_or_default();
        for (a, b, o) in overlays {
            if i >= *a && i < *b {
                st = st.patch(*o);
            }
        }
        if d >= left && d + cw <= left + width && cw > 0 {
            let x = x0 + (d - left) as u16;
            if c == '\t' {
                for k in 0..cw {
                    buf.set_string(x + k as u16, y, " ", st);
                }
            } else if c.is_control() {
                buf.set_string(x, y, "·", st);
            } else {
                buf.set_string(x, y, c.to_string(), st);
            }
        }
        d += cw;
        if d >= left + width {
            break;
        }
    }
}

fn render_window(
    fbuf: &mut ratatui::buffer::Buffer,
    ed: &mut Editor,
    win: &Window,
    rect: Rect,
    is_active: bool,
    hl_range: Option<(Pos, Pos)>,
    cursor_pos: &mut Option<(u16, u16)>,
) {
    if win.buf_idx >= ed.bufs.len() {
        return;
    }
    let line_numbers = ed.line_numbers;
    let git_info = if is_active { ed.git.clone() } else { None };
    let b = &mut ed.bufs[win.buf_idx];
    let gm = if b.kind == BufKind::File && b.git_tracked { 1 } else { 0 };
    let gl = if line_numbers { b.lines.len().to_string().len() + 1 } else { 0 };
    let text_w = (rect.width as usize).saturating_sub(gm + gl);
    let text_h = (rect.height as usize).saturating_sub(1);

    let top = if is_active { b.top } else { win.top.min(b.lines.len().saturating_sub(1)) };
    let left = if is_active { b.left } else { win.left };
    let cursor = if is_active { b.cursor } else { win.cursor };
    let region = if b.mark_active && is_active {
        b.mark.map(|m| if m <= cursor { (m, cursor) } else { (cursor, m) })
    } else {
        None
    };
    let n = b.lines.len();
    let (tab, kind) = (b.tab_width, b.kind);

    for r in 0..text_h {
        let li = top + r;
        let y = rect.y + r as u16;
        if li >= n {
            break;
        }
        if gm == 1 {
            let m = b.git_marks.get(li).copied().unwrap_or(0);
            let (ch, st) = match m {
                MARK_ADDED => ("▎", fg(c(INFO))),
                MARK_MODIFIED => ("▎", fg(c(WARN))),
                MARK_DELETED => ("▁", fg(c(ERROR))),
                _ => (" ", Style::default()),
            };
            fbuf.set_string(rect.x, y, ch, st);
        }
        if gl > 0 {
            let s = format!("{:>w$} ", li + 1, w = gl - 1);
            fbuf.set_string(rect.x + gm as u16, y, s, fg(c(DIM)));
        }
        if kind == BufKind::File && li == cursor.line && is_active {
            // No-op: visual mode selection logic handles the highlight via overlays
        }
        let line = b.lines[li].clone();
        let styles = match kind {
            BufKind::File | BufKind::GitDiff => b.line_styles(li),
            k => special_styles(k, &line, b.meta.get(li)),
        };
        let mut ov: Vec<(usize, usize, Style)> = Vec::new();
        let len = line.chars().count();
        if let Some((a, e)) = region {
            if li >= a.line && li <= e.line {
                let s = if li == a.line { a.col } else { 0 };
                let en = if li == e.line { e.col } else { len + 1 };
                if ed.vim_mode == VimMode::Visual || ed.vim_mode == VimMode::VisualLine {
                    ov.push((0, len, Style::default().bg(c(SEL))));
                } else {
                    ov.push((s, en, Style::default().bg(c(SEL))));
                }
            }
        }
        if is_active {
            if let Some((a, e)) = hl_range {
                if li >= a.line && li <= e.line {
                    let s = if li == a.line { a.col } else { 0 };
                    let en = if li == e.line { e.col } else { len };
                    ov.push((s, en, Style::default().bg(c(WARN)).fg(Color::Black)));
                }
            }
        }
        if kind != BufKind::File && li == cursor.line && is_active && !matches!(kind, BufKind::GitBlame | BufKind::Help) {
            // Removed cursor line highlight to avoid interference with visual mode
        }
        paint_line(
            fbuf,
            rect.x + (gm + gl) as u16,
            y,
            text_w,
            left,
            tab,
            &line,
            styles.as_deref(),
            &ov,
        );

    }

    // ---- mode line
    let flag = if b.read_only { "%%" } else if b.modified { "**" } else { "--" };
    let pct = if b.lines.len() <= text_h {
        "All".to_string()
    } else if top == 0 {
        "Top".to_string()
    } else if top + text_h >= b.lines.len() {
        "Bot".to_string()
    } else {
        format!("{}%", top * 100 / (b.lines.len() - text_h).max(1))
    };
    let lang = if b.kind == BufKind::File { format!("({})  ", b.hl.name) } else { String::new() };
    let mode_badge = if is_active {
        match ed.vim_mode {
            VimMode::Normal => "[NORMAL] ",
            VimMode::Insert => "[INSERT] ",
            VimMode::Visual => "[VISUAL] ",
            VimMode::VisualLine => "[V-LINE] ",
        }
    } else {
        ""
    };
    let left_txt = format!(" {mode_badge}{flag}  {}  {lang}L{}/{} C{}  {pct}", b.name, cursor.line + 1, b.lines.len(), cursor.col + 1);
    let mut right_txt = String::new();
    if is_active {
        if let Some(g) = &git_info {
            right_txt = format!("git:{}", g.branch);
            if g.ahead > 0 {
                right_txt.push_str(&format!(" ↑{}", g.ahead));
            }
            if g.behind > 0 {
                right_txt.push_str(&format!(" ↓{}", g.behind));
            }
            if g.staged > 0 {
                right_txt.push_str(&format!(" +{}", g.staged));
            }
            if g.modified > 0 {
                right_txt.push_str(&format!(" ~{}", g.modified));
            }
            if g.untracked > 0 {
                right_txt.push_str(&format!(" ?{}", g.untracked));
            }
            right_txt.push(' ');
        }
    }
    let ml = if is_active {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().bg(c(LINE)).fg(c(DIM))
    };
    let my = rect.y + rect.height - 1;
    let w = rect.width as usize;
    fbuf.set_string(rect.x, my, " ".repeat(w), ml);
    let lw = left_txt.chars().count();
    let rw = right_txt.chars().count();
    fbuf.set_string(rect.x, my, left_txt.chars().take(w).collect::<String>(), ml);
    if lw + rw + 2 <= w {
        fbuf.set_string(rect.x + (w - rw) as u16, my, &right_txt, ml.add_modifier(Modifier::BOLD));
    }

    if is_active && cursor_pos.is_none() {
        let d = display_col(&b.lines[cursor.line], cursor.col, b.tab_width);
        if cursor.line >= top && cursor.line < top + text_h && d >= left {
            let x = rect.x + (gm + gl) as u16 + (d - left) as u16;
            let y = rect.y + (cursor.line - top) as u16;
            if x < rect.x + rect.width && y < my {
                *cursor_pos = Some((x, y));
            }
        }
    }
}

pub fn render(f: &mut Frame, ed: &mut Editor) {
    let area = f.area();
    let (w, h) = (area.width as usize, area.height as usize);
    ed.screen_w = area.width;
    ed.screen_h = area.height;
    if h < 3 || w < 12 {
        return;
    }

    let hl_range: Option<(Pos, Pos)> = match &ed.mode {
        Mode::ISearch(s) => s.matched,
        Mode::QReplace(q) => q.cur,
        _ => None,
    };

    let win_area = Rect::new(0, 0, w as u16, (h - 1) as u16);
    let mut win_rects = Vec::new();
    let mut separators = Vec::new();
    ed.win_tree.compute_rects(win_area, &mut win_rects, &mut separators);

    let active_id = ed.active_win_id;
    for &(id, rect) in &win_rects {
        if id == active_id {
            ed.text_h = (rect.height as usize).saturating_sub(1);
            ed.text_w = rect.width as usize;
            ed.fix_scroll();
            if let Some(w) = ed.windows.iter_mut().find(|w| w.id == id) {
                if w.buf_idx < ed.bufs.len() {
                    let b = &ed.bufs[w.buf_idx];
                    w.cursor = b.cursor;
                    w.top = b.top;
                    w.left = b.left;
                }
            }
            break;
        }
    }

    let fbuf = f.buffer_mut();
    let mut cursor: Option<(u16, u16)> = None;

    // Draw vertical separators
    for &(sep_x, sep_y, sep_h) in &separators {
        for row in 0..sep_h {
            fbuf.set_string(sep_x, sep_y + row, "│", fg(c(DIM)));
        }
    }

    // Render each window
    for &(id, rect) in &win_rects {
        if rect.height < 2 || rect.width < 4 {
            continue;
        }
        let is_active = id == active_id;
        let win = ed.windows.iter().find(|w| w.id == id).cloned();
        if let Some(win) = win {
            render_window(fbuf, ed, &win, rect, is_active, hl_range, &mut cursor);
        }
    }

    // ---- echo area
    let ey = (h - 1) as u16;
    match &ed.mode {
        Mode::Prompt(p) => {
            let head = format!("{}{}", p.label, p.input);
            fbuf.set_string(0, ey, head.chars().take(w).collect::<String>(), Style::default());
            let lw = unicode_width::UnicodeWidthStr::width(p.label.as_str());
            let iw: usize = p.input.chars().take(p.cur).map(|c| c.width().unwrap_or(1)).sum();
            cursor = Some(((lw + iw).min(w - 1) as u16, ey));
            let mut x = unicode_width::UnicodeWidthStr::width(head.as_str()) + 2;
            let items: &Vec<String> = if !p.hint.is_empty() { &p.hint } else { &p.matches };
            if x < w {
                for (i, m) in items.iter().enumerate() {
                    let mw = unicode_width::UnicodeWidthStr::width(m.as_str());
                    if x + mw + 3 > w {
                        fbuf.set_string(x as u16, ey, "…", fg(c(DIM)));
                        break;
                    }
                    let st = if p.hint.is_empty() && i == p.sel { Style::default().add_modifier(Modifier::REVERSED) } else { fg(c(DIM)) };
                    fbuf.set_string(x as u16, ey, m, st);
                    x += mw + 3;
                }
            }
        }
        Mode::ISearch(s) => {
            let label = format!(
                "{}I-search{}: {}",
                if s.failing { "Failing " } else { "" },
                if s.forward { "" } else { " backward" },
                s.query
            );
            let st = if s.failing { fg(c(ERROR)) } else { Style::default() };
            fbuf.set_string(0, ey, label.chars().take(w).collect::<String>(), st);
        }
        Mode::QReplace(q) => {
            let t = format!("Query replacing {} with {}: (y/n/!/./q) ", q.from, q.to);
            fbuf.set_string(0, ey, t.chars().take(w).collect::<String>(), bold());
        }
        Mode::YesNo(y) => {
            fbuf.set_string(0, ey, y.q.chars().take(w).collect::<String>(), bold());
        }
        Mode::Normal => {
            let m = if ed.msg.is_empty() && !ed.pending.is_empty() { format!("{} -", ed.pending.join(" ")) } else { ed.msg.clone() };
            fbuf.set_string(0, ey, m.chars().take(w).collect::<String>(), Style::default());
        }
    }

    if let Some((x, y)) = cursor {
        f.set_cursor_position(Position::new(x, y));
    }
}
