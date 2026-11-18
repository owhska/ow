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

fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

fn special_styles(
    kind: BufKind,
    line: &str,
    meta: Option<&LineMeta>,
) -> Option<Vec<Style>> {
    let length = line.chars().count();
    let mut styles = vec![Style::default(); length];

    let mut paint = |a: usize, b: usize, style: Style| {
        for style_cell in styles
            .iter_mut()
            .take(b.min(length))
            .skip(a)
        {
            *style_cell = style;
        }
    };

    match kind {
        BufKind::GitStatus => {
            if line.starts_with("Repository:") || line.starts_with("Branch:") {
                paint(0, 11, fg(c(TYPE)).add_modifier(Modifier::BOLD));
            } else if line.starts_with("Staged")
                || line.starts_with("Unstaged")
                || line.starts_with("Untracked")
            {
                paint(0, length, bold());
            } else if line.starts_with(" s stage") {
                paint(0, length, fg(c(DIM)));
            } else if let Some(LineMeta::File { staged, .. }) = meta {
                let color = if *staged {
                    c(INFO)
                } else if line.trim_start().starts_with('?') {
                    c(ERROR)
                } else {
                    c(WARN)
                };

                paint(0, length, fg(color));
            }
        }
        BufKind::GitLog => {
            paint(0, 7, fg(c(WARN)));
            paint(9, 19, fg(c(TYPE)));
            paint(21, 30, fg(c(DIM)));
            paint(32, 46, fg(c(INFO)));
        }
        BufKind::Grep => match meta {
            Some(LineMeta::Grep { path, line: line_number, match_start, match_end }) => {
                let path_length = path.chars().count();

                paint(0, path_length, fg(c(TYPE)));
                let snippet_start = path_length + 1 + line_number.to_string().len() + 2;
                paint(path_length, snippet_start, fg(c(INFO)));
                paint(
                    snippet_start + match_start,
                    snippet_start + match_end,
                    Style::default().bg(c(WARN)).fg(Color::Black),
                );
            }
            _ => paint(0, length, bold()),
        },
        BufKind::GitBlame => {
            paint(0, 7, fg(c(WARN)));
            paint(8, 22, fg(c(INFO)));
            paint(23, 36, fg(c(DIM)));
        }
        BufKind::Dired => {
            if meta.is_none() {
                paint(0, length, bold());
            } else if line.ends_with('/') {
                paint(0, length, fg(c(FUNC)).add_modifier(Modifier::BOLD));
            }
        }
        BufKind::List | BufKind::Help => {
            if !line.starts_with(' ') && !line.is_empty() || line.starts_with(" MR") {
                paint(0, length, bold());
            }
        }
        _ => return None,
    }

    Some(styles)
}

#[allow(clippy::too_many_arguments)]
fn paint_line(
    buffer: &mut ratatui::buffer::Buffer,
    x0: u16,
    y: u16,
    width: usize,
    left: usize,
    tab: usize,
    line: &str,
    styles: Option<&[Style]>,
    overlays: &[(usize, usize, Style)],
) {
    let mut display_column = 0usize;

    for (index, character) in line.chars().enumerate() {
        let cell_width = if character == '\t' {
            tab - display_column % tab
        } else if character.is_control() {
            1
        } else {
            character.width().unwrap_or(0)
        };

        let mut style = styles
            .and_then(|styles| styles.get(index))
            .copied()
            .unwrap_or_default();

        for (start, end, overlay) in overlays {
            if index >= *start && index < *end {
                style = style.patch(*overlay);
            }
        }

        if display_column >= left
            && display_column + cell_width <= left + width
            && cell_width > 0
        {
            let x = x0 + (display_column - left) as u16;

            if character == '\t' {
                for offset in 0..cell_width {
                    buffer.set_string(x + offset as u16, y, " ", style);
                }
            } else if character.is_control() {
                buffer.set_string(x, y, "·", style);
            } else {
                buffer.set_string(x, y, character.to_string(), style);
            }
        }

        display_column += cell_width;

        if display_column >= left + width {
            break;
        }
    }
}

fn render_window(
    frame_buffer: &mut ratatui::buffer::Buffer,
    editor: &mut Editor,
    window: &Window,
    rect: Rect,
    is_active: bool,
    highlight_range: Option<(Pos, Pos)>,
    cursor_position: &mut Option<(u16, u16)>,
) {
    if window.buf_idx >= editor.bufs.len() {
        return;
    }

    let line_numbers = editor.line_numbers;
    let git_info = if is_active {
        editor.git.clone()
    } else {
        None
    };
    let search_highlight = editor.search_highlight;

    let buffer = &mut editor.bufs[window.buf_idx];
    let git_gutter_width = if buffer.kind == BufKind::File && buffer.git_tracked {
        1
    } else {
        0
    };

    let line_number_width = if line_numbers {
        buffer.lines.len().to_string().len() + 1
    } else {
        0
    };

    let text_width = (rect.width as usize).saturating_sub(git_gutter_width + line_number_width);
    let text_height = (rect.height as usize).saturating_sub(1);

    let top = if is_active {
        buffer.top
    } else {
        window.top.min(buffer.lines.len().saturating_sub(1))
    };

    let left = if is_active {
        buffer.left
    } else {
        window.left
    };

    let cursor = if is_active {
        buffer.cursor
    } else {
        window.cursor
    };

    let selection = if buffer.mark_active && is_active {
        buffer.mark.map(|mark| {
            if mark <= cursor {
                (mark, cursor)
            } else {
                (cursor, mark)
            }
        })
    } else {
        None
    };
    let line_count = buffer.lines.len();
    let (tab_width, kind) = (buffer.tab_width, buffer.kind);

    let mut visual_row = 0;
    for line_index in top..line_count {
        if visual_row >= text_height {
            break;
        }

        let line = buffer.lines[line_index].clone();
        let styles = match kind {
            BufKind::File | BufKind::GitDiff => buffer.line_styles(line_index),
            kind => special_styles(kind, &line, buffer.meta.get(line_index)),
        };

        let mut overlays: Vec<(usize, usize, Style)> = Vec::new();
        let line_length = line.chars().count();

        if let Some((start, end)) = selection {
            if line_index >= start.line && line_index <= end.line {
                if editor.vim_mode == VimMode::VisualLine {
                    // Line-wise Visual mode: highlight the entire line.
                    overlays.push((
                        0,
                        line_length,
                        Style::default().bg(c(SEL)),
                    ));
                } else if editor.vim_mode == VimMode::VisualBlock {
                    let left = buffer.mark.map(|m| m.col).unwrap_or(start.col).min(buffer.cursor.col);
                    let right = buffer.mark.map(|m| m.col).unwrap_or(start.col).max(buffer.cursor.col).saturating_add(1);
                    overlays.push((left.min(line_length), right.min(line_length), Style::default().bg(c(SEL))));
                } else if editor.vim_mode == VimMode::Visual {
                    // Character-wise Visual mode: highlight only from the
                    // selection start column to the cursor column.
                    let start_column = if line_index == start.line {
                        start.col
                    } else {
                        0
                    };

                    let end_column = if line_index == end.line {
                        end.col
                    } else {
                        line_length
                    };

                    if start_column < end_column {
                        overlays.push((
                            start_column,
                            end_column,
                            Style::default().bg(c(SEL)),
                        ));
                    }
                }
            }
        }

        if is_active && editor.vim_mode == VimMode::Insert {
            for pos in &editor.vim_multi_cursors {
                if pos.line == line_index && pos.col < line_length {
                    overlays.push((pos.col, pos.col + 1, Style::default().bg(c(WARN)).fg(Color::Black)));
                }
            }
        }

        if is_active {
            if let Some((start, end)) = highlight_range {
                if line_index >= start.line && line_index <= end.line {
                    let start_column = if line_index == start.line {
                        start.col
                    } else {
                        0
                    };

                    let end_column = if line_index == end.line {
                        end.col
                    } else {
                        line_length
                    };

                    overlays.push((
                        start_column,
                        end_column,
                        Style::default().bg(c(WARN)).fg(Color::Black),
                    ));
                }
            }
            if let Some((buffer_index, start, end)) = search_highlight {
                if buffer_index == window.buf_idx && line_index == start.line {
                    overlays.push((
                        start.col,
                        end.col,
                        Style::default().bg(c(WARN)).fg(Color::Black),
                    ));
                }
            }
        }

        let segments = if editor.wrap {
            crate::editor::wrap_segments(&line, tab_width, text_width)
        } else {
            vec![(left, left.saturating_add(text_width))]
        };
        let first_segment = if editor.wrap && line_index == top {
            segments
                .iter()
                .position(|(segment_left, _)| *segment_left == left)
                .unwrap_or(0)
        } else {
            0
        };
        for (segment_index, &(segment_left, _)) in segments.iter().enumerate().skip(first_segment) {
            if visual_row >= text_height {
                break;
            }
            let y = rect.y + visual_row as u16;
            let continuation = editor.wrap && segment_index > 0;

            if continuation {
                frame_buffer.set_string(
                    rect.x,
                    y,
                    " ".repeat(git_gutter_width + line_number_width),
                    Style::default(),
                );
            } else {
                if git_gutter_width == 1 {
                    let mark = buffer.git_marks.get(line_index).copied().unwrap_or(0);

                    let (character, style) = match mark {
                        MARK_ADDED => ("▎", fg(c(INFO))),
                        MARK_MODIFIED => ("▎", fg(c(WARN))),
                        MARK_DELETED => ("▁", fg(c(ERROR))),
                        _ => (" ", Style::default()),
                    };
                    frame_buffer.set_string(rect.x, y, character, style);
                }

                if line_number_width > 0 {
                    let number = format!(
                        "{:>width$} ",
                        line_index + 1,
                        width = line_number_width - 1
                    );
                    frame_buffer.set_string(
                        rect.x + git_gutter_width as u16,
                        y,
                        number,
                        fg(c(DIM)),
                    );
                }
            }

            paint_line(
                frame_buffer,
                rect.x + (git_gutter_width + line_number_width) as u16,
                y,
                text_width,
                if editor.wrap { segment_left } else { left },
                tab_width,
                &line,
                styles.as_deref(),
                &overlays,
            );

            if editor.wrap && is_active && cursor.line == line_index {
                let display_column = display_col(&line, cursor.col, tab_width);
                let segment_end = segments[segment_index].1;
                let is_last_segment = segment_index + 1 == segments.len();
                if display_column >= segment_left
                    && (display_column < segment_end
                        || (is_last_segment && display_column <= segment_end))
                {
                    let x = rect.x
                        + (git_gutter_width + line_number_width) as u16
                        + display_column.saturating_sub(segment_left) as u16;
                    if x < rect.x + rect.width {
                        *cursor_position = Some((x, y));
                    }
                }
            }

            visual_row += 1;
        }
    }

    // ---- mode line
    let flag = if buffer.read_only {
        "%%"
    } else if buffer.modified {
        "**"
    } else {
        "--"
    };

    let percentage = if buffer.lines.len() <= text_height {
        "All".to_string()
    } else if top == 0 {
        "Top".to_string()
    } else if top + text_height >= buffer.lines.len() {
        "Bot".to_string()
    } else {
        format!(
            "{}%",
            top * 100 / (buffer.lines.len() - text_height).max(1)
        )
    };

    let language = if buffer.kind == BufKind::File {
        format!("({})  ", buffer.hl.name)
    } else {
        String::new()
    };

    let mode_badge = if is_active {
        match editor.vim_mode {
            VimMode::Normal => "[NORMAL] ",
            VimMode::Insert => "[INSERT] ",
            VimMode::Visual => "[VISUAL] ",
            VimMode::VisualLine => "[V-LINE] ",
            VimMode::VisualBlock => "[V-BLOCK] ",
        }
    } else {
        ""
    };

    let left_text = format!(
        " {mode_badge}{flag}  {}  {language}L{}/{} C{}  {percentage}",
        buffer.name,
        cursor.line + 1,
        buffer.lines.len(),
        cursor.col + 1,
    );

    let mut right_text = String::new();

    if is_active {
        if let Some(git) = &git_info {
            right_text = format!("git:{}", git.branch);

            if git.ahead > 0 {
                right_text.push_str(&format!(" ↑{}", git.ahead));
            }

            if git.behind > 0 {
                right_text.push_str(&format!(" ↓{}", git.behind));
            }

            if git.staged > 0 {
                right_text.push_str(&format!(" +{}", git.staged));
            }

            if git.modified > 0 {
                right_text.push_str(&format!(" ~{}", git.modified));
            }

            if git.untracked > 0 {
                right_text.push_str(&format!(" ?{}", git.untracked));
            }

            right_text.push(' ');
        }
    }

    let mode_line_style = if is_active {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().bg(c(LINE)).fg(c(DIM))
    };

    let mode_line_y = rect.y + rect.height - 1;
    let width = rect.width as usize;

    frame_buffer.set_string(
        rect.x,
        mode_line_y,
        " ".repeat(width),
        mode_line_style,
    );

    let left_length = left_text.chars().count();
    let right_length = right_text.chars().count();

    frame_buffer.set_string(
        rect.x,
        mode_line_y,
        left_text
            .chars()
            .take(width)
            .collect::<String>(),
        mode_line_style,
    );

    if left_length + right_length + 2 <= width {
        frame_buffer.set_string(
            rect.x + (width - right_length) as u16,
            mode_line_y,
            &right_text,
            mode_line_style.add_modifier(Modifier::BOLD),
        );
    }

    if is_active && !editor.wrap && cursor_position.is_none() {
        let display_column = display_col(
            &buffer.lines[cursor.line],
            cursor.col,
            buffer.tab_width,
        );

        if cursor.line >= top
            && cursor.line < top + text_height
            && display_column >= left
        {
            let x = rect.x
                + (git_gutter_width + line_number_width) as u16
                + (display_column - left) as u16;
            let y = rect.y + (cursor.line - top) as u16;

            if x < rect.x + rect.width && y < mode_line_y {
                *cursor_position = Some((x, y));
            }
        }
    }
}

fn render_project_tree(
    buffer: &mut ratatui::buffer::Buffer,
    editor: &Editor,
    rect: Rect,
) {
    let Some(tree) = &editor.project_tree else { return };
    let title: String = tree.root.to_string_lossy().chars().take(rect.width as usize).collect();
    buffer.set_string(0, 0, title, bold().fg(c(TYPE)));

    let visible_height = rect.height.saturating_sub(1) as usize;
    for (offset, entry) in tree.entries.iter().skip(tree.top).take(visible_height).enumerate() {
        let marker = if entry.is_dir {
            if entry.expanded { "v " } else { "> " }
        } else {
            "  "
        };
        let suffix = if entry.is_dir { "/" } else { "" };
        let line = format!("{}{}{}{}", "  ".repeat(entry.depth), marker, entry.name, suffix);
        let style = if entry.is_dir {
            fg(c(FUNC)).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let line: String = line.chars().take(rect.width as usize).collect();
        buffer.set_string(0, (offset + 1) as u16, line, style);
    }
}

pub fn render(frame: &mut Frame, editor: &mut Editor) {
    let area = frame.area();
    let (width, height) = (area.width as usize, area.height as usize);

    editor.screen_w = area.width;
    editor.screen_h = area.height;

    if height < 3 || width < 12 {
        return;
    }

    let highlight_range: Option<(Pos, Pos)> = match &editor.mode {
        Mode::ISearch(search) => search.matched,
        Mode::QReplace(replace) => replace.cur,
        _ => None,
    };

    let tree_width = editor.project_tree_width();
    let window_x = if tree_width > 0 { tree_width + 1 } else { 0 };
    let window_area = Rect::new(
        window_x,
        0,
        (width as u16).saturating_sub(window_x),
        (height - 1) as u16,
    );
    let mut window_rects = Vec::new();
    let mut separators = Vec::new();

    editor
        .win_tree
        .compute_rects(window_area, &mut window_rects, &mut separators);

    let active_id = editor.active_win_id;

    for &(window_id, rect) in &window_rects {
        if window_id == active_id {
            editor.text_h = (rect.height as usize).saturating_sub(1);
            editor.text_w = rect.width as usize;
            editor.fix_scroll();

            if let Some(window) = editor
                .windows
                .iter_mut()
                .find(|window| window.id == window_id)
            {
                if window.buf_idx < editor.bufs.len() {
                    let buffer = &editor.bufs[window.buf_idx];
                    window.cursor = buffer.cursor;
                    window.top = buffer.top;
                    window.left = buffer.left;
                }
            }

            break;
        }
    }

    let frame_buffer = frame.buffer_mut();
    let mut cursor: Option<(u16, u16)> = None;

    if tree_width > 0 {
        render_project_tree(
            frame_buffer,
            editor,
            Rect::new(0, 0, tree_width, (height - 1) as u16),
        );
        for row in 0..height.saturating_sub(1) as u16 {
            frame_buffer.set_string(tree_width, row, "│", fg(c(DIM)));
        }
    }

    // Draw vertical separators.
    for &(separator_x, separator_y, separator_height) in &separators {
        for row in 0..separator_height {
            frame_buffer.set_string(
                separator_x,
                separator_y + row,
                "│",
                fg(c(DIM)),
            );
        }
    }

    // Render each window.
    for &(window_id, rect) in &window_rects {
        if rect.height < 2 || rect.width < 4 {
            continue;
        }

        let is_active = window_id == active_id;

        let window = editor
            .windows
            .iter()
            .find(|window| window.id == window_id)
            .cloned();

        if let Some(window) = window {
            render_window(
                frame_buffer,
                editor,
                &window,
                rect,
                is_active,
                highlight_range,
                &mut cursor,
            );
        }
    }

    if tree_width > 0 && editor.project_tree_focused {
        if let Some(tree) = &editor.project_tree {
            let y = tree.cursor.saturating_sub(tree.top) + 1;
            if y < height.saturating_sub(1) {
                cursor = Some((1.min(tree_width.saturating_sub(1)), y as u16));
            }
        }
    }

    // ---- echo area
    let echo_y = (height - 1) as u16;

    match &editor.mode {
        Mode::Prompt(prompt) => {
            let head = format!("{}{}", prompt.label, prompt.input);

            frame_buffer.set_string(
                0,
                echo_y,
                head.chars().take(width).collect::<String>(),
                Style::default(),
            );

            let label_width =
                unicode_width::UnicodeWidthStr::width(prompt.label.as_str());

            let input_width: usize = prompt
                .input
                .chars()
                .take(prompt.cur)
                .map(|c| c.width().unwrap_or(1))
                .sum();

            cursor = Some((
                (label_width + input_width).min(width - 1) as u16,
                echo_y,
            ));

            let mut x =
                unicode_width::UnicodeWidthStr::width(head.as_str()) + 2;

            let items: &Vec<String> = if !prompt.hint.is_empty() {
                &prompt.hint
            } else {
                &prompt.matches
            };

            if x < width {
                for (index, item) in items.iter().enumerate() {
                    let item_width =
                        unicode_width::UnicodeWidthStr::width(item.as_str());

                    if x + item_width + 3 > width {
                        frame_buffer.set_string(
                            x as u16,
                            echo_y,
                            "…",
                            fg(c(DIM)),
                        );
                        break;
                    }

                    let style = if prompt.hint.is_empty() && index == prompt.sel {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        fg(c(DIM))
                    };

                    frame_buffer.set_string(x as u16, echo_y, item, style);
                    x += item_width + 3;
                }
            }
        }
        Mode::ISearch(search) => {
            let label = format!(
                "{}I-search{}: {}",
                if search.failing { "Failing " } else { "" },
                if search.forward { "" } else { " backward" },
                search.query
            );

            let style = if search.failing {
                fg(c(ERROR))
            } else {
                Style::default()
            };

            frame_buffer.set_string(
                0,
                echo_y,
                label.chars().take(width).collect::<String>(),
                style,
            );
        }
        Mode::QReplace(replace) => {
            let text = format!(
                "Query replacing {} with {}: (y/n/!/./q) ",
                replace.from, replace.to
            );

            frame_buffer.set_string(
                0,
                echo_y,
                text.chars().take(width).collect::<String>(),
                bold(),
            );
        }
        Mode::YesNo(yes_no) => {
            frame_buffer.set_string(
                0,
                echo_y,
                yes_no
                    .q
                    .chars()
                    .take(width)
                    .collect::<String>(),
                bold(),
            );
        }
        Mode::Normal => {
            let message = if editor.msg.is_empty() && !editor.pending.is_empty() {
                format!("{} -", editor.pending.join(" "))
            } else {
                editor.msg.clone()
            };

            frame_buffer.set_string(
                0,
                echo_y,
                message.chars().take(width).collect::<String>(),
                Style::default(),
            );
        }
    }

    if let Some((x, y)) = cursor {
        frame.set_cursor_position(Position::new(x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_render_places_cursor_on_continuation_row() {
        let mut editor = Editor::new();
        editor.buf_mut().set_text("abcdefghij");
        editor.buf_mut().cursor = Pos::new(0, 8);
        editor.wrap = true;
        editor.text_w = 6;
        editor.text_h = 2;
        editor.fix_scroll();

        let rect = Rect::new(0, 0, 6, 3);
        let mut frame = ratatui::buffer::Buffer::empty(rect);
        let mut cursor_position = None;
        render_window(
            &mut frame,
            &mut editor,
            &Window::new(0, 0),
            rect,
            true,
            None,
            &mut cursor_position,
        );

        assert_eq!(cursor_position, Some((2, 1)));
    }

    #[test]
    fn persistent_search_match_is_highlighted() {
        let mut editor = Editor::new();
        editor.buf_mut().set_text("one target");
        editor.search_highlight = Some((0, Pos::new(0, 4), Pos::new(0, 10)));

        let rect = Rect::new(0, 0, 12, 2);
        let mut frame = ratatui::buffer::Buffer::empty(rect);
        let mut cursor_position = None;
        render_window(
            &mut frame,
            &mut editor,
            &Window::new(0, 0),
            rect,
            true,
            None,
            &mut cursor_position,
        );

        assert_eq!(frame[(4, 0)].style().bg, Some(c(WARN)));
        assert_ne!(frame[(3, 0)].style().bg, Some(c(WARN)));
    }

    #[test]
    fn project_tree_does_not_highlight_the_selected_row() {
        let mut editor = Editor::new();
        editor.project_tree = Some(crate::editor::ProjectTree {
            root: ".".into(),
            entries: vec![crate::editor::ProjectTreeEntry {
                path: "file.rs".into(),
                name: "file.rs".into(),
                depth: 0,
                is_dir: false,
                expanded: false,
            }],
            cursor: 0,
            top: 0,
            pending: String::new(),
        });
        editor.project_tree_focused = true;

        let rect = Rect::new(0, 0, 20, 3);
        let mut frame = ratatui::buffer::Buffer::empty(rect);
        render_project_tree(&mut frame, &editor, rect);

        assert_ne!(frame[(0, 1)].style().bg, Some(c(SEL)));
    }

    #[test]
    fn grep_match_is_highlighted_in_results_list() {
        let line = "src/a.rs:12: one needle here";
        let meta = LineMeta::Grep {
            path: "src/a.rs".into(),
            line: 12,
            match_start: 4,
            match_end: 10,
        };
        let styles = special_styles(BufKind::Grep, line, Some(&meta)).unwrap();
        let snippet_start = "src/a.rs:12: ".chars().count();

        assert_eq!(styles[snippet_start + 4].bg, Some(c(WARN)));
        assert_ne!(styles[snippet_start + 3].bg, Some(c(WARN)));
    }
}
