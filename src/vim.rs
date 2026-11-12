// Vim modal editing layer for ow

impl Editor {
    pub fn handle_vim_key(&mut self, key: Key) -> bool {
        if self.buf().kind == BufKind::Dired {
            let n = key.name.as_str();
            let meta = self.buf().meta.get(self.buf().cursor.line).cloned();
            let cur_dir = self.dired_current_dir().unwrap_or_else(|| self.cwd.clone());

            // ---- gg sequence ----
            if self.vim_pending == "g" {
                self.vim_pending.clear();
                if n == "g" { self.bob(); }
                return true;
            }

            match n {
                // --- File operations (differ from vim defaults) ---
                "c" => {
                    if let Some(LineMeta::Path(p)) = meta {
                        self.copy_file(p);
                        return true;
                    }
                }
                "x" => {
                    if let Some(LineMeta::Path(p)) = meta {
                        self.cut_file(p);
                        return true;
                    }
                }
                "p" => {
                    self.paste_file(cur_dir);
                    return true;
                }
                "%" => {
                    self.start_prompt(Prompt::new("Create (name or dir/): ", PKind::DiredCreate(cur_dir), None));
                    return true;
                }
                "d" => {
                    if let Some(LineMeta::Path(p)) = meta {
                        self.delete_file(p);
                        return true;
                    }
                }
                "r" => {
                    if let Some(LineMeta::Path(p)) = meta {
                        let old_name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let label = format!("Rename {} to: ", old_name);
                        self.file_clipboard = Some(p);
                        self.start_prompt(Prompt::new(&label, PKind::DiredCreate(cur_dir), None));
                        return true;
                    }
                }

                // --- Vim-style navigation ---
                "j" | "<down>" => { self.next_line(); return true; }
                "k" | "<up>"   => { self.previous_line(); return true; }
                "l" | "<right>" | "RET" => { self.visit_line(); return true; }
                "h" | "<left>" => {
                    // go up to parent directory
                    if let Some(parent) = cur_dir.parent().map(|p| p.to_path_buf()) {
                        self.open_dir(&parent);
                    }
                    return true;
                }
                "G" => { self.eob(); return true; }
                "g" => {
                    self.vim_pending = "g".to_string();
                    self.say("g-");
                    return true;
                }
                "C-f" | "C-d" => { self.scroll_down(); return true; }
                "C-b" | "C-u" => { self.scroll_up(); return true; }
                "q" => {
                    let i = self.cur;
                    self.kill_buffer_idx(i);
                    return true;
                }
                "n" => { self.next_line(); return true; }

                // --- Let SPC leader and C-g through ---
                "SPC" | "C-g" => { /* fall through to normal vim handling */ }

                // --- Block everything else ---
                _ => { return true; }
            }
        }

        match self.vim_mode {
            VimMode::Normal => self.handle_vim_normal(key),
            VimMode::Insert => self.handle_vim_insert(key),
            VimMode::Visual | VimMode::VisualLine => self.handle_vim_visual(key),
        }
    }

    // -------------------------------------------------------- Insert Mode
    fn handle_vim_insert(&mut self, key: Key) -> bool {
        let n = key.name.as_str();
        match n {
            "ESC" | "C-[" => {
                self.vim_mode = VimMode::Normal;
                let b = self.buf_mut();
                if b.cursor.col > 0 {
                    b.cursor.col -= 1;
                }
                //self.say("-- NORMAL --");
                return true;
            }
            "RET" => {
                self.newline_indent();
                return true;
            }
            "DEL" => {
                // Auto-pair backspace: if between empty pair (), {}, [], "", '', <>
                let b = self.buf_mut();
                if b.cursor.col > 0 && b.cursor.line < b.lines.len() {
                    let line = &b.lines[b.cursor.line];
                    let col = b.cursor.col;
                    let chars: Vec<char> = line.chars().collect();
                    if col <= chars.len() && col > 0 {
                        let prev = chars[col - 1];
                        let next = chars.get(col).copied();
                        let is_pair = matches!(
                            (prev, next),
                            ('(', Some(')'))
                                | ('{', Some('}'))
                                | ('[', Some(']'))
                                | ('"', Some('"'))
                                | ('\'', Some('\''))
                                | ('<', Some('>'))
                        );
                        if is_pair {
                            let at = Pos::new(b.cursor.line, col - 1);
                            let end = Pos::new(b.cursor.line, col + 1);
                            b.group += 1;
                            b.delete(at, end);
                            b.cursor = at;
                            return true;
                        }
                    }
                }
                self.delete_backward();
                return true;
            }
            "TAB" => {
                self.indent_tab();
                return true;
            }
            "S-TAB" => {
                self.unindent();
                return true;
            }
            _ => {}
        }

        // Auto-pairing: inoremap ( ()<Left>, etc.
        if self.auto_pair {
            if let Some(c) = key.ch {
                let b = self.buf_mut();
                if b.cursor.line < b.lines.len() {
                    let line = &b.lines[b.cursor.line];
                    let chars: Vec<char> = line.chars().collect();
                    // If user types closing delimiter that's already in front of cursor, skip over it
                    if b.cursor.col < chars.len() && chars[b.cursor.col] == c && matches!(c, ')' | '}' | ']' | '"' | '\'' | '>') {
                        b.cursor.col += 1;
                        return true;
                    }
                }

                match c {
                    '(' => { self.insert_pair("()"); return true; }
                    '{' => { self.insert_pair("{}"); return true; }
                    '[' => { self.insert_pair("[]"); return true; }
                    '"' => { self.insert_pair("\"\""); return true; }
                    '\'' => { self.insert_pair("''"); return true; }
                    '<' => { self.insert_pair("<>"); return true; }
                    _ => {}
                }
            }
        }

        // Return false so normal_key can run self-insert-command for regular chars
        false
    }

    fn insert_pair(&mut self, pair: &str) {
        let b = self.buf_mut();
        b.group += 1;
        let at = b.cursor;
        b.insert(at, pair);
        b.cursor.col += 1;
    }

    // -------------------------------------------------------- Visual Mode
    fn handle_vim_visual(&mut self, key: Key) -> bool {
        let n = key.name.as_str();
        match n {
            "ESC" | "C-[" | "C-g" => {
                self.vim_mode = VimMode::Normal;
                self.buf_mut().mark_active = false;
                //self.say("-- NORMAL --");
                return true;
            }
            // Move selected lines: K and J as requested
            "K" => {
                self.move_selected_lines(true);
                return true;
            }
            "J" => {
                self.move_selected_lines(false);
                return true;
            }
            // Delete selection:
            "d" | "x" => {
                self.delete_visual_selection();
                self.vim_mode = VimMode::Normal;
                //self.say("-- NORMAL --");
                return true;
            }
            // Change selection:
            "c" => {
                self.delete_visual_selection();
                self.vim_mode = VimMode::Insert;
                //self.say("-- INSERT --");
                return true;
            }
            // Yank selection:
                "y" => {
                    self.yank_visual_selection();
                    self.vim_mode = VimMode::Normal;
                    self.copy_to_system_clipboard();
                    return true;
                }








            // Toggle visual line:
            "V" => {
                if self.vim_mode == VimMode::VisualLine {
                    self.vim_mode = VimMode::Visual;
                    //self.say("-- VISUAL --");
                } else {
                    self.vim_mode = VimMode::VisualLine;
                    //self.say("-- VISUAL LINE --");
                }
                return true;
            }
            "v" => {
                self.vim_mode = VimMode::Normal;
                self.buf_mut().mark_active = false;
                //self.say("-- NORMAL --");
                return true;
            }
            // Movements:
            "h" | "<left>" => { self.backward_char(); return true; }
            "j" | "<down>" => { self.next_line(); return true; }
            "k" | "<up>" => { self.previous_line(); return true; }
            "l" | "<right>" => { self.forward_char(); return true; }
            "w" => { self.vim_forward_word(); return true; }
            "b" => { self.bwd_word(); return true; }
            "e" => { self.vim_forward_end_word(); return true; }
            // Page navigation is Vim-owned; never reuse Emacs cursor motions.
            "C-f" | "C-d" => { self.scroll_down(); return true; }
            "C-b" | "C-u" => { self.scroll_up(); return true; }
            "0" | "<home>" => { self.bol(); return true; }
            "$" | "<end>" | "L" | "S-l" => { self.eol(); return true; }
            "^" | "H" | "S-h" => { self.back_to_indentation(); return true; }
            "G" => { self.eob(); return true; }
            _ => {}
        }
        false
    }

    // -------------------------------------------------------- Normal Mode
    fn handle_vim_normal(&mut self, key: Key) -> bool {
        let n = key.name.as_str();

        // 1. If replace character is pending: r <char>
        if self.vim_replace_next {
            self.vim_replace_next = false;
            if let Some(c) = key.ch {
                let b = self.buf_mut();
                if b.cursor.line < b.lines.len() {
                    let at = b.cursor;
                    if let Some(next) = b.next_pos(at) {
                        b.group += 1;
                        b.delete(at, next);
                        b.insert(at, &c.to_string());
                        b.cursor = at;
                    }
                }
            } else if n == "RET" {
                let b = self.buf_mut();
                let at = b.cursor;
                if let Some(next) = b.next_pos(at) {
                    b.group += 1;
                    b.delete(at, next);
                    b.insert(at, "\n");
                    b.cursor = at;
                }
            }
            return true;
        }

        // 2. If operator pending: d, c, y
        if let Some(op) = self.vim_op {
            self.vim_op = None;
            if op == 'd' || op == 'c' {
                self.buf_mut().group += 1;
            }
            match (op, n) {
                ('d', "d") => {
                    self.kill_line();
                    self.delete_char(); // remove trailing newline if any
                    return true;
                }
                ('c', "c") => {
                    self.kill_line();
                    self.open_line();
                    self.vim_mode = VimMode::Insert;
                    //self.say("-- INSERT --");
                    return true;
                }
                ('y', "y") => {
                    self.bol();
                    let at = self.buf().cursor;
                    self.buf_mut().mark = Some(at);
                    self.buf_mut().mark_active = true;
                    self.eol();
                    self.copy_region();
                    self.buf_mut().mark_active = false;
                    self.buf_mut().cursor = at;
                    self.say("1 line yanked");
                    return true;
                }
                ('d', "w") => {
                    let p = self.buf().cursor;
                    self.vim_forward_word();
                    let e = self.buf().cursor;
                    let text = self.buf().text_in(p, e);
                    self.buf_mut().delete(p, e);
                    self.buf_mut().cursor = p;
                    self.kill(&text, false);
                    return true;
                }
                ('c', "w") => { self.kill_word(); self.vim_mode = VimMode::Insert; self.say("-- INSERT --"); return true; }
                ('y', "w") => {
                    let at = self.buf().cursor;
                    self.buf_mut().mark = Some(at);
                    self.buf_mut().mark_active = true;
                    self.fwd_word();
                    self.copy_region();
                    self.buf_mut().mark_active = false;
                    self.buf_mut().cursor = at;
                    self.say("Word yanked");
                    return true;
                }
                ('d', "$") => { self.kill_line(); return true; }
                ('c', "$") => { self.kill_line(); self.vim_mode = VimMode::Insert; self.say("-- INSERT --"); return true; }
                ('y', "$") => {
                    let at = self.buf().cursor;
                    self.buf_mut().mark = Some(at);
                    self.buf_mut().mark_active = true;
                    self.eol();
                    self.copy_region();
                    self.buf_mut().mark_active = false;
                    self.buf_mut().cursor = at;
                    self.say("Yanked to end of line");
                    return true;
                }
                _ => {
                    self.say("Operator cancelled");
                    return true;
                }
            }
        }

        // 3. Leader key sequence: SPC ...
        if !self.vim_pending.is_empty() && self.vim_pending.starts_with("SPC") {
            let chord = format!("{} {}", self.vim_pending, n);
            match chord.as_str() {
                // <leader>f: file finder (project-find-file)
                "SPC f" => {
                    self.vim_pending.clear();
                    self.project_find_file();
                    return true;
                }
                // <leader><space>: project grep
                "SPC g" => {
                    self.vim_pending.clear();
                    self.project_grep_cmd();
                    return true;
                }
                // <leader>e: Explore (:Explore / dired)
                "SPC e" => {
                    self.vim_pending.clear();
                    self.dired_cmd();
                    return true;
                }
                // <leader><Tab>: :tabnext
                "SPC TAB" => {
                    self.vim_pending.clear();
                    self.next_buffer();
                    return true;
                }
                // <leader><S-Tab>: :tabprevious
                "SPC S-TAB" => {
                    self.vim_pending.clear();
                    self.prev_buffer();
                    return true;
                }
                // <leader>t: :term
                "SPC t" => {
                    self.vim_pending.clear();
                    return true;
                }
                // <leader>ww: :w
                "SPC w w" => {
                    self.vim_pending.clear();
                    self.save_cmd();
                    return true;
                }
                // <leader>wq: :q (close current window; quit if last window)
                "SPC w q" => {
                    self.vim_pending.clear();
                    if self.windows.len() > 1 {
                        self.delete_window();
                    } else {
                        self.quit_cmd();
                    }
                    return true;
                }
                // <leader>q: :tabclose
                "SPC q" => {
                    self.vim_pending.clear();
                    self.kill_buffer_cmd();
                    return true;
                }
                // <leader>s: vimgrep / isearch
                "SPC s" => {
                    self.vim_pending.clear();
                    self.isearch_start(true);
                    return true;
                }
                // <leader>wv: :vsplit
                "SPC w v" => {
                    self.vim_pending.clear();
                    self.split_window_right();
                    return true;
                }
                // <leader>ws: :split
                "SPC w s" => {
                    self.vim_pending.clear();
                    self.split_window_below();
                    return true;
                }
                // <leader>wh, <leader>wl, <leader>wj, <leader>wk: window navigation
                "SPC w h" | "SPC w l" | "SPC w j" | "SPC w k" => {
                    self.vim_pending.clear();
                    self.other_window();
                    return true;
                }
                // <leader>rr: replace word under cursor
                "SPC r r" => {
                    self.vim_pending.clear();
                    self.vim_replace_word_under_cursor();
                    return true;
                }
                // Window resize (inverts previously inverted directions):
                "SPC <left>" => {
                    self.vim_pending.clear();
                    self.win_tree.resize_split(self.active_win_id, SplitDir::Vertical, -0.05);
                    return true;
                }
                "SPC <right>" => {
                    self.vim_pending.clear();
                    self.win_tree.resize_split(self.active_win_id, SplitDir::Vertical, 0.05);
                    return true;
                }
                "SPC <up>" => {
                    self.vim_pending.clear();
                    self.win_tree.resize_split(self.active_win_id, SplitDir::Horizontal, -0.05);
                    return true;
                }
                "SPC <down>" => {
                    self.vim_pending.clear();
                    self.win_tree.resize_split(self.active_win_id, SplitDir::Horizontal, 0.05);
                    return true;
                }
                // <leader>x: Select directory
                "SPC x" => {
                    self.vim_pending.clear();
                    self.vim_select_directory();
                    return true;
                }
                // Prefixes that wait for next key:

                "SPC w" | "SPC r" => {
                    self.vim_pending = chord;
                    self.say(format!("{} -", self.vim_pending));
                    return true;
                }
                _ => {
                    self.say(format!("{chord} is undefined"));
                    self.vim_pending.clear();
                    return true;
                }
            }
        }

        // Leader trigger:
        if n == "SPC" {
            self.vim_pending = "SPC".to_string();
            self.say("SPC -");
            return true;
        }

        // 'gg' sequence:
        if self.vim_pending == "g" {
            self.vim_pending.clear();
            if n == "g" {
                self.bob();
                return true;
            }
        }

        // 4. Direct bindings requested:
        match n {
            "C-a" => {
                self.mark_whole();
                self.vim_mode = VimMode::VisualLine;
                //self.say("-- VISUAL LINE --");
                return true;
            }
            "C-e" => {
                self.switch_buffer_cmd();
                return true;
            }
            "S-<right>" | "S-Right" => {
                self.next_buffer();
                return true;
            }
            "S-<left>" | "S-Left" => {
                self.prev_buffer();
                return true;
            }
            "H" | "S-h" => {
                self.back_to_indentation();
                return true;
            }
            "L" | "S-l" => {
                self.eol();
                return true;
            }
            _ => {}
        }

        // 5. Normal mode Vim motions and edits:
        match n {
            // Enter insert mode:
            "i" => {
                self.vim_mode = VimMode::Insert;
                //self.say("-- INSERT --");
                return true;
            }
            "I" => {
                self.back_to_indentation();
                self.vim_mode = VimMode::Insert;
                //self.say("-- INSERT --");
                return true;
            }
            "a" => {
                let b = self.buf_mut();
                if b.cursor.col < b.line_len(b.cursor.line) {
                    b.cursor.col += 1;
                }
                self.vim_mode = VimMode::Insert;
                //self.say("-- INSERT --");
                return true;
            }
            "A" => {
                self.eol();
                self.vim_mode = VimMode::Insert;
                //self.say("-- INSERT --");
                return true;
            }
            "o" => {
                self.eol();
                self.newline_indent();
                self.vim_mode = VimMode::Insert;
                //self.say("-- INSERT --");
                return true;
            }
            "O" => {
                self.bol();
                self.open_line();
                self.vim_mode = VimMode::Insert;
                //self.say("-- INSERT --");
                return true;
            }

            // Enter visual mode:
            "v" => {
                self.vim_mode = VimMode::Visual;
                let b = self.buf_mut();
                b.mark = Some(b.cursor);
                b.mark_active = true;
                //self.say("-- VISUAL --");
                return true;
            }
            "V" => {
                self.vim_mode = VimMode::VisualLine;
                let b = self.buf_mut();
                b.mark = Some(b.cursor);
                b.mark_active = true;
                //self.say("-- VISUAL LINE --");
                return true;
            }

            // Motions:
            // Motions:
            "h" | "<left>" => { self.backward_char(); return true; }
            "j" | "<down>" => { self.next_line(); return true; }
            "k" | "<up>" => { self.previous_line(); return true; }
            "l" | "<right>" => { self.forward_char(); return true; }
            "w" => { self.vim_forward_word(); return true; }
            "b" => { self.bwd_word(); return true; }
            "e" => { self.vim_forward_end_word(); return true; }
            "C-f" => { self.scroll_down(); return true; }
            "0" | "<home>" => { self.bol(); return true; }
            "^" => { self.back_to_indentation(); return true; }
            "$" | "<end>" | "L" | "S-l" => { self.eol(); return true; }
            "G" => { self.eob(); return true; }
            "SPC x" => { self.vim_select_directory(); return true; }

            // Edits:
            "x" => {
                self.buf_mut().group += 1;
                self.delete_char();
                return true;
            }
            "~" => {
                let b = self.buf_mut();
                if let Some(c) = b.char_at(b.cursor) {
                    let toggled = if c.is_uppercase() {
                        c.to_lowercase().collect::<String>()
                    } else {
                        c.to_uppercase().collect::<String>()
                    };
                    let at = b.cursor;
                    b.group += 1;
                    if let Some(next) = b.next_pos(at) {
                        b.delete(at, next);
                        b.insert(at, &toggled);
                        b.cursor = b.next_pos(at).unwrap_or(at);
                    }
                }
                return true;
            }
            "r" => {
                self.vim_replace_next = true;
                self.say("replace: ");
                return true;
            }
            "u" => { self.undo_cmd(); return true; }
            "C-r" => { self.redo_cmd(); return true; }
            "p" => {
                let b = self.buf_mut();
                if b.cursor.col < b.line_len(b.cursor.line) {
                    b.cursor.col += 1;
                }
                self.yank();
                return true;
            }
            "P" => { self.yank(); return true; }
            "D" => { self.kill_line(); return true; }
            "C" => { self.kill_line(); self.vim_mode = VimMode::Insert; self.say("-- INSERT --"); return true; }
            "S" => { self.kill_line(); self.open_line(); self.vim_mode = VimMode::Insert; self.say("-- INSERT --"); return true; }
            "Y" => {
                self.bol();
                let at = self.buf().cursor;
                self.buf_mut().mark = Some(at);
                self.buf_mut().mark_active = true;
                self.eol();
                self.copy_region();
                self.buf_mut().mark_active = false;
                self.buf_mut().cursor = at;
                self.say("1 line yanked");
                return true;
            }

            // Operator triggers:
            "d" => { self.vim_op = Some('d'); self.say("d-"); return true; }
            "c" => { self.vim_op = Some('c'); self.say("c-"); return true; }
            "y" => { self.vim_op = Some('y'); self.say("y-"); return true; }

            // Search & commands:
            "/" => { self.isearch_start(true); return true; }
            "?" => { self.isearch_start(false); return true; }
            ":" => { self.extended_command(); return true; }

            // ESC in normal mode cancels any pending state
            "ESC" | "C-[" => {
                self.vim_pending.clear();
                self.vim_op = None;
                self.vim_replace_next = false;
                //self.say("-- NORMAL --");
                return true;
            }
            _ => {}
        }

        // Non-motion Ctrl/Alt commands may fall through to the complementary
        // Emacs command map. Movement keys are filtered by editor.rs so they
        // cannot acquire a second, conflicting meaning here.
        if n.starts_with("C-") || n.starts_with("M-") {
            return false;
        }

        // In normal mode, unknown plain characters should NOT insert text
        if key.ch.is_some() {
            self.say(format!("Undefined key: {n}"));
            return true;
        }

        false
    }

        pub fn copy_to_system_clipboard(&mut self) {
        let b = self.buf();
        let cur = b.cursor;
        let (start, end) = if b.mark_active {
            let m = b.mark.unwrap();
            if m <= cur { (m, cur) } else { (cur, m) }
        } else {
            (cur, cur)
        };
        let text = b.text_in(start, end);
        if !text.is_empty() {
            crate::clipboard::set_clipboard(&text);
        }
    }
    pub fn move_selected_lines(&mut self, up: bool) {
        let b = self.buf_mut();
        let Some(m) = b.mark else { return };
        let cur = b.cursor;
        let (min_l, max_l) = if m.line <= cur.line {
            (m.line, cur.line)
        } else {
            (cur.line, m.line)
        };
        if up {
            if min_l == 0 {
                return;
            }
            let above = b.lines.remove(min_l - 1);
            b.lines.insert(max_l, above);
            b.cursor.line -= 1;
            if let Some(m) = &mut b.mark {
                m.line -= 1;
            }
        } else {
            if max_l + 1 >= b.lines.len() {
                return;
            }
            let below = b.lines.remove(max_l + 1);
            b.lines.insert(min_l, below);
            b.cursor.line += 1;
            if let Some(m) = &mut b.mark {
                m.line += 1;
            }
        }
        b.modified = true;
    }

    fn vim_forward_end_word(&mut self) {
        let b = self.buf_mut();
        let mut p = b.cursor;
        if let Some(next) = b.next_pos(p) {
            p = next;
        }
        while let Some(c) = b.char_at(p) {
            if is_word(c) {
                break;
            }
            if let Some(next) = b.next_pos(p) {
                p = next;
            } else {
                b.cursor = p;
                return;
            }
        }
        while let Some(next) = b.next_pos(p) {
            if let Some(c) = b.char_at(next) {
                if is_word(c) {
                    p = next;
                    continue;
                }
            }
            break;
        }
        b.cursor = p;
    }

    pub fn word_under_cursor(&self) -> Option<String> {
        let b = self.buf();
        let line = b.lines.get(b.cursor.line)?;
        let chars: Vec<char> = line.chars().collect();
        if b.cursor.col >= chars.len() || !is_word(chars[b.cursor.col]) {
            return None;
        }
        let mut start = b.cursor.col;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        let mut end = b.cursor.col;
        while end < chars.len() && is_word(chars[end]) {
            end += 1;
        }
        Some(chars[start..end].iter().collect())
    }

    pub fn vim_replace_word_under_cursor(&mut self) {
        if let Some(w) = self.word_under_cursor() {
            self.start_prompt(Prompt::new(&format!("Replace '{w}' with: "), PKind::QrTo(w), None));
        } else {
            self.say("No word under cursor");
        }
    }

    fn vim_forward_word(&mut self) {
        let b = self.buf_mut();
        let mut p = b.cursor;
        if let Some(c) = b.char_at(p) {
            if is_word(c) {
                while let Some(c2) = b.char_at(p) {
                    if is_word(c2) {
                        if let Some(next) = b.next_pos(p) { p = next; } else { break; }
                    } else {
                        break;
                    }
                }
            } else if !c.is_whitespace() {
                while let Some(c2) = b.char_at(p) {
                    if !is_word(c2) && !c2.is_whitespace() {
                        if let Some(next) = b.next_pos(p) { p = next; } else { break; }
                    } else {
                        break;
                    }
                }
            }
        }
        while let Some(c) = b.char_at(p) {
            if c.is_whitespace() {
                if let Some(next) = b.next_pos(p) { p = next; } else { break; }
            } else {
                break;
            }
        }
        b.cursor = p;
    }

    fn delete_visual_selection(&mut self) {
        let b = self.buf();
        let Some(m) = b.mark else { return };
        let cur = b.cursor;
        if self.vim_mode == VimMode::VisualLine {
            let min_l = m.line.min(cur.line);
            let max_l = m.line.max(cur.line);
            let start = Pos::new(min_l, 0);
            let end = if max_l + 1 < b.lines.len() {
                Pos::new(max_l + 1, 0)
            } else {
                Pos::new(max_l, b.line_len(max_l))
            };
            let text = b.text_in(start, end);
            self.buf_mut().delete(start, end);
            self.buf_mut().cursor = start;
            self.buf_mut().mark_active = false;
            self.kill(&text, false);
        } else {
            let (start, mut end) = if m <= cur { (m, cur) } else { (cur, m) };
            if let Some(next) = b.next_pos(end) {
                end = next;
            }
            let text = b.text_in(start, end);
            self.buf_mut().delete(start, end);
            self.buf_mut().cursor = start;
            self.buf_mut().mark_active = false;
            self.kill(&text, false);
        }
    }

    fn yank_visual_selection(&mut self) {
        let b = self.buf();
        let Some(m) = b.mark else { return };
        let cur = b.cursor;
        let (start, end) = if self.vim_mode == VimMode::VisualLine {
            let min_l = m.line.min(cur.line);
            let max_l = m.line.max(cur.line);
            let s = Pos::new(min_l, 0);
            let e = if max_l + 1 < b.lines.len() {
                Pos::new(max_l + 1, 0)
            } else {
                Pos::new(max_l, b.line_len(max_l))
            };
            (s, e)
        } else {
            let (s, mut e) = if m <= cur { (m, cur) } else { (cur, m) };
            if let Some(next) = b.next_pos(e) {
                e = next;
            }
            (s, e)
        };
        let text = b.text_in(start, end);
        self.kill(&text, false);
        self.buf_mut().mark_active = false;
        self.say("Yanked selection");
    }

    pub fn vim_select_directory(&mut self) {
        let mut entries = Vec::new();
        let ignored = [".git", "node_modules", "dist", "build", ".cache", ".npm", ".cargo", ".rustup", "venv", ".venv", "__pycache__", "target", "vendor", "staticfiles", "coverage"];
        
        let mut cmd = format!("find {} -mindepth 1 -maxdepth 6 ", self.cwd.display());
        if !ignored.is_empty() {
            let expr = ignored.iter()
                .map(|n| format!("-name '{}'", n))
                .collect::<Vec<_>>()
                .join(" -o ");
            cmd.push_str(&format!("\\( {} \\) -prune -o -type d -print", expr));
        } else {
            cmd.push_str("-type d -print");
        }

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&format!("{} 2>/dev/null", cmd))
            .output();

        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let line = line.trim();
                if !line.is_empty() && line != "." {
                    entries.push(line.to_string());
                }
            }
        }
        entries.sort();
        
        if entries.is_empty() {
            self.say("No directories found");
            return;
        }

        self.start_prompt(Prompt::new(
            "Select directory: ",
            PKind::Switch,
            Some(entries)
        ));
    }

}

