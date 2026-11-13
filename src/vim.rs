// Vim modal editing layer for ow

impl Editor {
    pub fn handle_vim_key(&mut self, key: Key) -> bool {
        if self.buf().kind == BufKind::Dired {
            let n = key.name.as_str();
            let meta = self.buf().meta.get(self.buf().cursor.line).cloned();
            let is_entry = self.buf().cursor.line > 1;
            let cur_dir = self.dired_current_dir().unwrap_or_else(|| self.cwd.clone());

            // ---- gg sequence ----
            if self.vim_pending == "g" {
                self.vim_pending.clear();
                if n == "g" {
                    let last_line = self.buf().lines.len().saturating_sub(1);
                    self.buf_mut().cursor = Pos::new(1.min(last_line), 2);
                }
                return true;
            }

            match n {
                // --- File operations (differ from vim defaults) ---
                "c" => {
                    if is_entry {
                        if let Some(LineMeta::Path(p)) = meta.clone() {
                            self.copy_file(p);
                            return true;
                        }
                    }
                }
                "x" => {
                    if is_entry {
                        if let Some(LineMeta::Path(p)) = meta.clone() {
                            self.cut_file(p);
                            return true;
                        }
                    }
                }
                "p" => {
                    self.paste_file(cur_dir);
                    return true;
                }
                "%" => {
                    self.start_prompt(Prompt::new(
                        "Create (name or dir/): ",
                        PKind::DiredCreate(cur_dir),
                        None,
                    ));
                    return true;
                }
                "d" => {
                    if is_entry {
                        if let Some(LineMeta::Path(p)) = meta.clone() {
                            self.delete_file(p);
                            return true;
                        }
                    }
                }
                "r" => {
                    if is_entry {
                        if let Some(LineMeta::Path(p)) = meta.clone() {
                            let old_name = p
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let label = format!("Rename {} to: ", old_name);

                            self.start_prompt(Prompt::new(
                                &label,
                                PKind::DiredRename { dir: cur_dir, source: p },
                                None,
                            ));
                            return true;
                        }
                    }
                }
                "R" => {
                    self.dired_refresh();
                    return true;
                }

                // --- Vim-style navigation ---
                "j" | "<down>" => {
                    self.next_line();
                    return true;
                }
                "k" | "<up>" => {
                    self.previous_line();
                    return true;
                }
                "l" | "<right>" | "RET" => {
                    self.visit_line();
                    return true;
                }
                "h" | "<left>" => {
                    // Go up to the parent directory.
                    if let Some(parent) = cur_dir.parent().map(|p| p.to_path_buf()) {
                        self.open_dir_at(&parent, Some(&cur_dir));
                    }
                    return true;
                }
                "G" => {
                    self.eob();
                    return true;
                }
                "g" => {
                    self.vim_pending = "g".to_string();
                    self.say("g-");
                    return true;
                }
                "C-f" | "C-d" => {
                    self.scroll_down();
                    return true;
                }
                "C-b" | "C-u" => {
                    self.scroll_up();
                    return true;
                }
                "q" => {
                    let i = self.cur;
                    self.kill_buffer_idx(i);
                    return true;
                }
                "n" => {
                    self.next_line();
                    return true;
                }

                // --- Let SPC leader and C-g through ---
                "SPC" | "C-g" => return false,

                // --- Block everything else ---
                _ => return true,
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

        // Replace mode overwrites existing characters instead of inserting.
        if self.vim_replace_mode {
            match n {
                "ESC" | "C-[" => {
                    self.vim_replace_mode = false;
                    self.vim_mode = VimMode::Normal;
                    if self.buf().cursor.col > 0 { self.buf_mut().cursor.col -= 1; }
                    return true;
                }
                "RET" => { self.newline_indent(); return true; }
                "DEL" => { self.backward_char(); return true; }
                _ => if let Some(c) = key.ch {
                    let b = self.buf_mut();
                    let at = b.cursor;
                    if let Some(next) = b.next_pos(at) {
                        b.group += 1;
                        b.delete(at, next);
                    }
                    b.insert(at, &c.to_string());
                    return true;
                }
            }
        }

        match n {
            "ESC" | "C-[" => {
                self.vim_mode = VimMode::Normal;
                let b = self.buf_mut();
                if b.cursor.col > 0 {
                    b.cursor.col -= 1;
                }
                return true;
            }
            "RET" => {
                self.newline_indent();
                return true;
            }
            "DEL" => {
                // Auto-pair backspace: if between an empty pair (), {}, [], "", '', <>
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

            // Arrow navigation remains available while inserting text.
            "<left>" => {
                let b = self.buf_mut();

                if b.cursor.col > 0 {
                    b.cursor.col -= 1;
                } else if b.cursor.line > 0 {
                    b.cursor.line -= 1;
                    b.cursor.col = b.line_len(b.cursor.line);
                }

                return true;
            }
            "<right>" => {
                let b = self.buf_mut();

                if let Some(next) = b.next_pos(b.cursor) {
                    b.cursor = next;
                }

                return true;
            }
            "<up>" => {
                let b = self.buf_mut();

                if b.cursor.line > 0 {
                    b.cursor.line -= 1;
                    b.cursor.col = b.cursor.col.min(b.line_len(b.cursor.line));
                }

                return true;
            }
            "<down>" => {
                let b = self.buf_mut();

                if b.cursor.line + 1 < b.lines.len() {
                    b.cursor.line += 1;
                    b.cursor.col = b.cursor.col.min(b.line_len(b.cursor.line));
                }

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

                    // If the user types a closing delimiter that is already in
                    // front of the cursor, skip over it instead of inserting it.
                    if b.cursor.col < chars.len()
                        && chars[b.cursor.col] == c
                        && matches!(c, ')' | '}' | ']' | '"' | '\'' | '>')
                    {
                        b.cursor.col += 1;
                        return true;
                    }
                }

                match c {
                    '(' => {
                        self.insert_pair("()");
                        return true;
                    }
                    '{' => {
                        self.insert_pair("{}");
                        return true;
                    }
                    '[' => {
                        self.insert_pair("[]");
                        return true;
                    }
                    '"' => {
                        self.insert_pair("\"\"");
                        return true;
                    }
                    '\'' => {
                        self.insert_pair("''");
                        return true;
                    }
                    '<' => {
                        self.insert_pair("<>");
                        return true;
                    }
                    _ => {}
                }
            }
        }

        // Return false so normal_key can run self-insert-command for regular chars.
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
                return true;
            }

            // Move selected lines: K and J as requested.
            "K" => {
                self.move_selected_lines(true);
                return true;
            }
            "J" => {
                self.move_selected_lines(false);
                return true;
            }

            // Delete selection.
            "d" | "x" => {
                self.delete_visual_selection();
                self.vim_mode = VimMode::Normal;
                return true;
            }

            // Change selection.
            "c" => {
                self.delete_visual_selection();
                self.vim_mode = VimMode::Insert;
                return true;
            }

            // Yank selection.
            "y" => {
                self.yank_visual_selection();
                self.vim_mode = VimMode::Normal;
                self.copy_to_system_clipboard();
                return true;
            }

            // Movements.
            "h" | "<left>" => {
                self.backward_char();
                return true;
            }
            "j" | "<down>" => {
                self.next_line();
                return true;
            }
            "k" | "<up>" => {
                self.previous_line();
                return true;
            }
            "l" | "<right>" => {
                self.forward_char();
                return true;
            }
            "w" => {
                self.vim_forward_word();
                return true;
            }
            "b" => {
                self.bwd_word();
                return true;
            }
            "e" => {
                self.vim_forward_end_word();
                return true;
            }

            // Page navigation is Vim-owned; never reuse Emacs cursor motions.
            "C-f" | "C-d" => {
                self.scroll_down();
                return true;
            }
            "C-b" | "C-u" => {
                self.scroll_up();
                return true;
            }

            "0" | "<home>" => {
                self.bol();
                return true;
            }
            "$" | "<end>" | "L" | "S-l" => {
                self.eol();
                return true;
            }
            "^" | "H" | "S-h" => {
                self.back_to_indentation();
                return true;
            }
            "G" => {
                self.eob();
                return true;
            }

            _ => {}
        }

        false
    }

    // -------------------------------------------------------- Normal Mode
    fn handle_vim_normal(&mut self, key: Key) -> bool {
        let n = key.name.as_str();

        // Multi-key Vim commands: marks, find motions and register selection.
        if matches!(self.vim_pending.as_str(), "mark-set" | "mark-jump" | "mark-line" | "find-f" | "find-F" | "find-t" | "find-T" | "register") {
            let pending = std::mem::take(&mut self.vim_pending);
            if let Some(c) = key.ch {
                match pending.as_str() {
                    "mark-set" if c.is_ascii_lowercase() => { self.vim_marks.insert(c, self.buf().cursor); self.say(format!("Mark {c} set")); }
                    "mark-jump" | "mark-line" if c.is_ascii_lowercase() => {
                        if let Some(p) = self.vim_marks.get(&c).copied() {
                            self.buf_mut().cursor = if pending == "mark-line" { Pos::new(p.line, 0) } else { p };
                        } else { self.say(format!("Unknown mark: {c}")); }
                    }
                    "find-f" | "find-F" | "find-t" | "find-T" => {
                        let direction = if pending == "find-f" || pending == "find-t" { 1 } else { -1 };
                        let till = pending == "find-t" || pending == "find-T";
                        self.vim_last_find = Some((c, if direction > 0 { if till {'t'} else {'f'} } else if till {'T'} else {'F'}));
                        self.vim_find_char(c, direction, till);
                    }
                    "register" => {
                        if c.is_ascii_alphanumeric() || matches!(c, '+' | '*') { self.vim_register = Some(c); }
                    }
                    _ => {}
                }
            }
            return true;
        }

        if self.vim_replace_mode {
            match n {
                "ESC" | "C-[" => { self.vim_replace_mode = false; self.vim_mode = VimMode::Normal; return true; }
                "RET" => { self.newline_indent(); return true; }
                "DEL" => { self.backward_char(); return true; }
                _ => if let Some(c) = key.ch {
                    let b = self.buf_mut();
                    let at = b.cursor;
                    if let Some(next) = b.next_pos(at) { b.group += 1; b.delete(at, next); b.insert(at, &c.to_string()); }
                    else { b.insert(at, &c.to_string()); }
                    return true;
                }
            }
        }

        match n {
            "m" => { self.vim_pending = "mark-set".into(); return true; }
            "'" => { self.vim_pending = "mark-line".into(); return true; }
            "`" => { self.vim_pending = "mark-jump".into(); return true; }
            "\"" => { self.vim_pending = "register".into(); return true; }
            "f" => { self.vim_pending = "find-f".into(); return true; }
            "F" => { self.vim_pending = "find-F".into(); return true; }
            "t" => { self.vim_pending = "find-t".into(); return true; }
            "T" => { self.vim_pending = "find-T".into(); return true; }
            ";" => { if let Some((c, kind)) = self.vim_last_find { let dir = if kind == 'f' || kind == 't' {1} else {-1}; self.vim_find_char(c, dir, kind == 't' || kind == 'T'); } return true; }
            "," => { if let Some((c, kind)) = self.vim_last_find { let dir = if kind == 'f' || kind == 't' {-1} else {1}; self.vim_find_char(c, dir, kind == 't' || kind == 'T'); } return true; }
            "n" | "N" => {
                if !self.last_search.is_empty() {
                    let forward = (n == "n");
                    let from = self.buf().cursor;
                    let found = if forward { self.buf().find_forward(from, &self.last_search, true) } else { self.buf().find_backward(from, &self.last_search, true) };
                    if let Some((a,e)) = found { self.buf_mut().cursor = if forward {a} else {e}; }
                }
                return true;
            }
            "%" => { self.vim_match_pair(); return true; }
            "R" => { self.vim_replace_mode = true; self.vim_mode = VimMode::Insert; self.say("REPLACE -- INSERT -- (ESC to exit)"); return true; }
            "*" | "#" => {
                if let Some(word) = self.word_under_cursor() {
                    self.last_search = word.clone();
                    let from = self.buf().cursor;
                    let found = if n == "*" { self.buf().find_forward(from, &word, true) } else { self.buf().find_backward(from, &word, true) };
                    if let Some((a,e)) = found { self.buf_mut().cursor = if n == "*" {a} else {e}; }
                }
                return true;
            }
            _ => {}
        }

        // Vim count prefix (e.g. 3j, 2dw, 4yy). Zero remains a motion.
        if self.vim_op.is_none() && n.len()==1 && n.chars().next().is_some_and(|c| c.is_ascii_digit() && (c!='0' || self.vim_count>0)) {
            let digit=n.parse::<usize>().unwrap_or(0);
            if self.vim_count < 1_000_000 { self.vim_count=self.vim_count.saturating_mul(10).saturating_add(digit); }
            return true;
        }

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

        // 2. Composable Vim operators: d/c/y + motions, counts and text objects.
        if let Some(op) = self.vim_op {
            if n.len()==1 && n.chars().next().is_some_and(|c| c.is_ascii_digit() && (c!='0' || self.vim_count>0)) {
                let digit=n.parse::<usize>().unwrap_or(0);
                if self.vim_count < 1_000_000 { self.vim_count=self.vim_count.saturating_mul(10).saturating_add(digit); }
                return true;
            }
            if n == "ESC" || n == "C-[" {
                self.vim_op = None;
                self.vim_pending.clear();
                self.vim_count = 0;
                self.vim_operator_count = 0;
                return true;
            }
            if self.vim_pending == "op-g" {
                self.vim_pending.clear();
                if n=="g" {
                    let start=self.buf().cursor;
                    let end=Pos::new(0,0);
                    self.vim_apply_operator(op,start,end,true);
                    self.vim_op=None; self.vim_count=0; self.vim_operator_count=0;
                } else { self.vim_op=None; self.say("Operator cancelled"); }
                return true;
            }
            if self.vim_pending == "op-i" || self.vim_pending == "op-a" {
                let around = self.vim_pending == "op-a";
                self.vim_pending.clear();
                if let Some((a,z)) = self.vim_object_range(n, around) {
                    self.vim_apply_operator(op,a,z,false);
                } else {
                    self.say("Unsupported text object");
                }
                self.vim_op=None;
                self.vim_count=0;
                return true;
            }
            if n == "i" || n == "a" {
                self.vim_pending = (if n=="i" {"op-i"} else {"op-a"}).to_string();
                return true;
            }
            if n=="g" {
                self.vim_pending="op-g".to_string();
                return true;
            }
            let count = self.vim_operator_count.max(1) * self.vim_count.max(1);
            let start = self.buf().cursor;
            let mut end = start;
            let mut linewise = false;
            if n.len()==1 && n.chars().next()==Some(op) {
                linewise=true;
                end=Pos::new((start.line+count-1).min(self.buf().lines.len()-1),0);
            } else { match n {
                "w" => end=self.vim_word_forward_pos(start,count),
                "b" => end=self.vim_word_backward_pos(start,count),
                "e" => {
                    end=start;
                    for _ in 0..count {
                        self.buf_mut().cursor=end;
                        self.vim_forward_end_word();
                        end=self.buf().cursor;
                    }
                    self.buf_mut().cursor=start;
                    if let Some(next)=self.buf().next_pos(end) { end=next; }
                }
                "$" => { linewise=false; end=Pos::new(start.line,(start.line..(start.line+count).min(self.buf().lines.len())).last().map(|l| self.buf().line_len(l)).unwrap_or(start.col)); }
                "0" | "^" | "_" => { end=Pos::new(start.line, if n=="0" {0} else {self.buf().indent_of(start.line).chars().count()}); }
                "h" | "<left>" => { for _ in 0..count { if let Some(p)=self.buf().prev_pos(end) {end=p;} } }
                "l" | "<right>" => { for _ in 0..count { if let Some(p)=self.buf().next_pos(end) {end=p;} } }
                "j" | "<down>" => { linewise=true; end=Pos::new((start.line+count).min(self.buf().lines.len()-1),start.col); }
                "k" | "<up>" => { linewise=true; end=Pos::new(start.line.saturating_sub(count),start.col); }
                "G" => { linewise=true; end=self.buf().end(); }
                _ => {
                    self.vim_op=None; self.vim_pending.clear();
                    self.vim_count=0; self.vim_operator_count=0;
                    self.say("Unsupported operator motion");
                    return true;
                }
            }}
            self.vim_apply_operator(op,start,end,linewise);
            self.vim_op=None;
            self.vim_count=0;
            self.vim_operator_count=0;
            return true;
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

                // Git namespace: <leader>g ...
                "SPC g s" => {
                    self.vim_pending.clear();
                    self.git_status();
                    return true;
                }
                "SPC g d" => {
                    self.vim_pending.clear();
                    self.git_diff_file();
                    return true;
                }
                "SPC g D" => {
                    self.vim_pending.clear();
                    self.git_diff_repo();
                    return true;
                }
                "SPC g l" => {
                    self.vim_pending.clear();
                    self.git_log(false);
                    return true;
                }
                "SPC g L" => {
                    self.vim_pending.clear();
                    self.git_log(true);
                    return true;
                }
                "SPC g b" => {
                    self.vim_pending.clear();
                    self.git_blame();
                    return true;
                }
                "SPC g B" => {
                    self.vim_pending.clear();
                    self.git_blame_line();
                    return true;
                }
                "SPC g a" => {
                    self.vim_pending.clear();
                    self.git_stage_file(true);
                    return true;
                }
                "SPC g u" => {
                    self.vim_pending.clear();
                    self.git_stage_file(false);
                    return true;
                }
                "SPC g c" => {
                    self.vim_pending.clear();
                    self.git_commit_cmd(false);
                    return true;
                }
                "SPC g A" => {
                    self.vim_pending.clear();
                    self.git_commit_cmd(true);
                    return true;
                }
                "SPC g o" => {
                    self.vim_pending.clear();
                    self.git_checkout_cmd();
                    return true;
                }
                "SPC g r" => {
                    self.vim_pending.clear();
                    self.git_restore_cmd();
                    return true;
                }
                "SPC g n" => {
                    self.vim_pending.clear();
                    self.git_hunk(true);
                    return true;
                }
                "SPC g p" => {
                    self.vim_pending.clear();
                    self.git_hunk(false);
                    return true;
                }
                "SPC g F" => {
                    self.vim_pending.clear();
                    self.git_pull();
                    return true;
                }
                "SPC g P" => {
                    self.vim_pending.clear();
                    self.git_push();
                    return true;
                }

                // Project grep moved here to free <leader>g for Git.
                "SPC p g" => {
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

                // Window resize.
                "SPC <left>" => {
                    self.vim_pending.clear();
                    self.win_tree.resize_split(
                        self.active_win_id,
                        SplitDir::Vertical,
                        -0.05,
                    );
                    return true;
                }
                "SPC <right>" => {
                    self.vim_pending.clear();
                    self.win_tree.resize_split(
                        self.active_win_id,
                        SplitDir::Vertical,
                        0.05,
                    );
                    return true;
                }
                "SPC <up>" => {
                    self.vim_pending.clear();
                    self.win_tree.resize_split(
                        self.active_win_id,
                        SplitDir::Horizontal,
                        -0.05,
                    );
                    return true;
                }
                "SPC <down>" => {
                    self.vim_pending.clear();
                    self.win_tree.resize_split(
                        self.active_win_id,
                        SplitDir::Horizontal,
                        0.05,
                    );
                    return true;
                }

                // <leader>x: Select directory
                "SPC x" => {
                    self.vim_pending.clear();
                    self.vim_select_directory();
                    return true;
                }

                // Prefixes that wait for the next key.
                "SPC w" | "SPC r" | "SPC g" | "SPC p" => {
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

        // Leader trigger.
        if n == "SPC" {
            self.vim_pending = "SPC".to_string();
            self.say("SPC -");
            return true;
        }

        // First key of Vim's gg motion. The second 'g' is handled below.
        if n == "g" && self.vim_pending.is_empty() {
            self.vim_pending = "g".to_string();
            self.say("g-");
            return true;
        }

        // 'gg' sequence.
        if self.vim_pending == "g" {
            self.vim_pending.clear();

            if n == "g" {
                let target=if self.vim_count>0 { self.vim_count.saturating_sub(1) } else {0};
                let last=self.buf().lines.len().saturating_sub(1);
                self.buf_mut().cursor=Pos::new(target.min(last),0);
                self.vim_count=0;
                return true;
            }
        }

        // 4. Direct bindings.
        match n {
            "C-a" => {
                self.mark_whole();
                self.vim_mode = VimMode::VisualLine;
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

        // 5. Normal mode Vim motions and edits.
        let had_count=self.vim_count>0;
        let count=self.vim_count.max(1);
        if self.vim_count>0 { self.vim_count=0; }
        match n {
            // Enter insert mode.
            "i" => {
                self.vim_mode = VimMode::Insert;
                return true;
            }
            "I" => {
                self.back_to_indentation();
                self.vim_mode = VimMode::Insert;
                return true;
            }
            "a" => {
                let b = self.buf_mut();

                if b.cursor.col < b.line_len(b.cursor.line) {
                    b.cursor.col += 1;
                }

                self.vim_mode = VimMode::Insert;
                return true;
            }
            "A" => {
                self.eol();
                self.vim_mode = VimMode::Insert;
                return true;
            }
            "o" => {
                self.eol();
                self.newline_indent();
                self.vim_mode = VimMode::Insert;
                return true;
            }
            "O" => {
                self.bol();
                self.open_line();
                self.vim_mode = VimMode::Insert;
                return true;
            }

            // Enter visual mode.
            //
            // `v` is character-wise Visual mode: the selection follows the
            // cursor exactly. `V` is line-wise Visual mode.
            "v" => {
                self.vim_mode = VimMode::Visual;

                let b = self.buf_mut();
                b.mark = Some(b.cursor);
                b.mark_active = true;

                return true;
            }
            "V" => {
                self.vim_mode = VimMode::VisualLine;

                let b = self.buf_mut();
                b.mark = Some(b.cursor);
                b.mark_active = true;

                return true;
            }

            // Motions.
            "h" | "<left>" => {
                for _ in 0..count { self.backward_char(); }
                return true;
            }
            "j" | "<down>" => {
                for _ in 0..count { self.next_line(); }
                return true;
            }
            "k" | "<up>" => {
                for _ in 0..count { self.previous_line(); }
                return true;
            }
            "l" | "<right>" => {
                for _ in 0..count { self.forward_char(); }
                return true;
            }
            "w" => {
                let p=self.vim_word_forward_pos(self.buf().cursor,count);
                self.buf_mut().cursor=p;
                return true;
            }
            "b" => {
                let p=self.vim_word_backward_pos(self.buf().cursor,count);
                self.buf_mut().cursor=p;
                return true;
            }
            "e" => {
                for _ in 0..count { self.vim_forward_end_word(); }
                return true;
            }
            "C-f" => {
                self.scroll_down();
                return true;
            }
            "0" | "<home>" => {
                self.bol();
                return true;
            }
            "^" => {
                self.back_to_indentation();
                return true;
            }
            "$" | "<end>" => {
                self.eol();
                return true;
            }
            "G" => {
                if had_count {
                    let line=count.saturating_sub(1).min(self.buf().lines.len()-1);
                    self.buf_mut().cursor=Pos::new(line,0);
                } else { self.eob(); }
                self.vim_count=0;
                return true;
            }

            // Edits.
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
            "u" => {
                self.undo_cmd();
                return true;
            }
            "C-r" => {
                self.redo_cmd();
                return true;
            }
            "p" => {
                let b = self.buf_mut();

                if b.cursor.col < b.line_len(b.cursor.line) {
                    b.cursor.col += 1;
                }

                if let Some(reg) = self.vim_register.take() {
                    if let Some(text) = self.vim_registers.get(&reg).cloned() { self.insert_yank(&text); }
                } else { self.yank(); }
                return true;
            }
            "P" => {
                if let Some(reg) = self.vim_register.take() {
                    if let Some(text) = self.vim_registers.get(&reg).cloned() { self.insert_yank(&text); }
                } else { self.yank(); }
                return true;
            }
            "D" => {
                self.kill_line();
                return true;
            }
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

            // Operator triggers.
            "d" => {
                self.vim_op = Some('d');
                self.vim_operator_count=count;
                self.vim_count=0;
                self.say("d-");
                return true;
            }
            "c" => {
                self.vim_op = Some('c');
                self.vim_operator_count=count;
                self.vim_count=0;
                self.say("c-");
                return true;
            }
            "y" => {
                self.vim_op = Some('y');
                self.vim_operator_count=count;
                self.vim_count=0;
                self.say("y-");
                return true;
            }

            // Search & commands.
            "/" => {
                self.isearch_start(true);
                return true;
            }
            "?" => {
                self.isearch_start(false);
                return true;
            }
            ":" => {
                self.extended_command();
                return true;
            }

            // ESC in normal mode cancels any pending state.
            "ESC" | "C-[" => {
                self.vim_pending.clear();
                self.vim_op = None;
                self.vim_replace_next = false;
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

        // In normal mode, unknown plain characters should NOT insert text.
        if key.ch.is_some() {
            self.say(format!("Undefined key: {n}"));
            return true;
        }

        false
    }

    fn vim_find_char(&mut self, target: char, direction: i32, till: bool) {
        let b = self.buf();
        let line: Vec<char> = b.lines.get(b.cursor.line).map(|s| s.chars().collect()).unwrap_or_default();
        let col = b.cursor.col.min(line.len());
        let found = if direction > 0 {
            ((col + 1)..line.len()).find(|&i| line[i] == target)
        } else {
            (0..col).rev().find(|&i| line[i] == target)
        };
        if let Some(mut i) = found {
            if till { if direction > 0 { i = i.saturating_sub(1); } else { i = (i + 1).min(line.len().saturating_sub(1)); } }
            self.buf_mut().cursor.col = i;
        }
    }

    fn vim_match_pair(&mut self) {
        let b = self.buf();
        let mut p = b.cursor;
        let pairs = [('(', ')'), ('[', ']'), ('{', '}')];
        let is_pair = |c: char| pairs.iter().any(|(o, z)| c == *o || c == *z);
        if !b.char_at(p).is_some_and(is_pair) {
            let Some(next) = b.next_pos(p) else { return; };
            p = next;
        }
        let mut found = None;
        for _ in 0..b.lines.iter().map(|l| l.chars().count()).sum::<usize>() {
            if let Some(c) = b.char_at(p) {
                if let Some((open, close)) = pairs.iter().find(|(o, c2)| c == *o || c == *c2) {
                    let forward = c == *open;
                    let mut depth = 0usize;
                    let mut q = p;
                    loop {
                        if let Some(ch) = b.char_at(q) {
                            if (forward && ch == *open) || (!forward && ch == *close) { depth += 1; }
                            if (forward && ch == *close) || (!forward && ch == *open) {
                                depth = depth.saturating_sub(1);
                                if depth == 0 { found = Some(q); break; }
                            }
                        }
                        let next = if forward { b.next_pos(q) } else { b.prev_pos(q) };
                        let Some(next) = next else { break; };
                        q = next;
                    }
                    break;
                }
            }
            let Some(next) = b.next_pos(p) else { break; };
            p = next;
        }
        if let Some(p) = found { self.buf_mut().cursor = p; }
    }

    pub fn copy_to_system_clipboard(&mut self) {
        let b = self.buf();
        let cur = b.cursor;

        let (start, end) = if b.mark_active {
            let m = b.mark.unwrap();

            if m <= cur {
                (m, cur)
            } else {
                (cur, m)
            }
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

            if let Some(mark) = &mut b.mark {
                mark.line -= 1;
            }
        } else {
            if max_l + 1 >= b.lines.len() {
                return;
            }

            let below = b.lines.remove(max_l + 1);
            b.lines.insert(min_l, below);

            b.cursor.line += 1;

            if let Some(mark) = &mut b.mark {
                mark.line += 1;
            }
        }

        b.modified = true;
    }


    fn vim_word_forward_pos(&self, mut p: Pos, count: usize) -> Pos {
        let b = self.buf();
        for _ in 0..count.max(1) {
            if let Some(c) = b.char_at(p) {
                if crate::buffer::is_word(c) {
                    while let Some(c2) = b.char_at(p) {
                        if !crate::buffer::is_word(c2) { break; }
                        if let Some(n) = b.next_pos(p) { p = n; } else { break; }
                    }
                } else if !c.is_whitespace() {
                    while let Some(c2) = b.char_at(p) {
                        if crate::buffer::is_word(c2) || c2.is_whitespace() { break; }
                        if let Some(n) = b.next_pos(p) { p = n; } else { break; }
                    }
                }
            }
            while let Some(c) = b.char_at(p) {
                if !c.is_whitespace() { break; }
                if let Some(n) = b.next_pos(p) { p = n; } else { break; }
            }
        }
        p
    }

    fn vim_word_backward_pos(&self, mut p: Pos, count: usize) -> Pos {
        let b = self.buf();
        for _ in 0..count.max(1) {
            if let Some(n) = b.prev_pos(p) { p = n; }
            while b.char_at(p).is_some_and(|c| c.is_whitespace()) {
                if let Some(n) = b.prev_pos(p) { p = n; } else { break; }
            }
            if b.char_at(p).is_some_and(crate::buffer::is_word) {
                while let Some(n) = b.prev_pos(p) {
                    if !b.char_at(n).is_some_and(crate::buffer::is_word) { break; }
                    p = n;
                }
            } else if b.char_at(p).is_some_and(|c| !c.is_whitespace()) {
                while let Some(n) = b.prev_pos(p) {
                    if b.char_at(n).is_none_or(|c| c.is_whitespace() || crate::buffer::is_word(c)) { break; }
                    p = n;
                }
            }
        }
        p
    }

    fn vim_object_range(&self, object: &str, around: bool) -> Option<(Pos, Pos)> {
        let b = self.buf();
        let p = b.cursor;
        let line: Vec<char> = b.lines.get(p.line)?.chars().collect();
        if line.is_empty() { return None; }
        let pair = match object {
            "w" => {
                let mut s = p.col.min(line.len());
                let mut e = s;
                if s == line.len() && s > 0 { s -= 1; e = s; }
                if line.get(s).is_some_and(|&c| crate::buffer::is_word(c)) {
                    while s > 0 && crate::buffer::is_word(line[s-1]) { s -= 1; }
                    e = s;
                    while e < line.len() && crate::buffer::is_word(line[e]) { e += 1; }
                } else {
                    while s > 0 && !line[s-1].is_whitespace() && !crate::buffer::is_word(line[s-1]) { s -= 1; }
                    e = s;
                    while e < line.len() && !line[e].is_whitespace() && !crate::buffer::is_word(line[e]) { e += 1; }
                }
                if around {
                    while s > 0 && line[s-1].is_whitespace() { s -= 1; }
                    if e < line.len() && line[e].is_whitespace() {
                        while e < line.len() && line[e].is_whitespace() { e += 1; }
                    }
                }
                Some((Pos::new(p.line,s), Pos::new(p.line,e)))
            }
            "\"" | "'" | "`" | "(" | ")" | "[" | "]" | "{" | "}" | "<" | ">" => {
                let (open, close) = match object {
                    "\"" => ('"','"'), "'" => ('\'','\''), "`" => ('`','`'),
                    "(" | ")" => ('(',')'), "[" | "]" => ('[',']'),
                    "{" | "}" => ('{','}'), _ => ('<','>')
                };
                let mut left = None;
                let mut right = None;
                if open == close {
                    for i in (0..=p.col.min(line.len().saturating_sub(1))).rev() {
                        if line.get(i)==Some(&open) { left=Some(i); break; }
                    }
                    if let Some(s)=left { right=((s+1)..line.len()).find(|&i| line[i]==close); }
                } else {
                    let mut depth=0usize;
                    for i in (0..=p.col.min(line.len().saturating_sub(1))).rev() {
                        if line[i]==close { depth+=1; }
                        if line[i]==open {
                            if depth==0 { left=Some(i); break; } else { depth-=1; }
                        }
                    }
                    if let Some(s)=left {
                        depth=0;
                        for i in s+1..line.len() {
                            if line[i]==open { depth+=1; }
                            if line[i]==close {
                                if depth==0 { right=Some(i); break; } else { depth-=1; }
                            }
                        }
                    }
                }
                let s=left?;
                let e=right?;
                Some((Pos::new(p.line, if around {s} else {s+1}), Pos::new(p.line, if around {e+1} else {e})))
            }
            _ => None,
        }?;
        Some(pair)
    }

    fn vim_apply_operator(&mut self, op: char, start: Pos, end: Pos, linewise: bool) {
        let b = self.buf();
        let (a, z) = if linewise {
            let lo = start.line.min(end.line);
            let hi = start.line.max(end.line);
            (Pos::new(lo,0), if hi+1 < b.lines.len() { Pos::new(hi+1,0) } else { b.end() })
        } else {
            if start <= end {(start,end)} else {(end,start)}
        };
        let text = b.text_in(a,z);
        if let Some(reg) = self.vim_register.take() { self.vim_registers.insert(reg, text.clone()); }
        if op == 'y' {
            self.vim_registers.insert('0', text.clone());
            let pk=self.prev_kill; self.prev_kill=false;
            self.kill(&text,false); self.prev_kill=pk; self.this_kill=false;
            self.buf_mut().cursor=start;
            self.say("Yanked");
        } else {
            self.buf_mut().group += 1;
            self.buf_mut().delete(a,z);
            self.buf_mut().cursor=a;
            self.buf_mut().mark_active=false;
            self.kill(&text,false);
            if op=='c' { self.vim_mode=VimMode::Insert; }
        }
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
            self.start_prompt(Prompt::new(
                &format!("Replace '{w}' with: "),
                PKind::QrTo(w),
                None,
            ));
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
                        if let Some(next) = b.next_pos(p) {
                            p = next;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            } else if !c.is_whitespace() {
                while let Some(c2) = b.char_at(p) {
                    if !is_word(c2) && !c2.is_whitespace() {
                        if let Some(next) = b.next_pos(p) {
                            p = next;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        while let Some(c) = b.char_at(p) {
            if c.is_whitespace() {
                if let Some(next) = b.next_pos(p) {
                    p = next;
                } else {
                    break;
                }
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
            let (start, mut end) = if m <= cur {
                (m, cur)
            } else {
                (cur, m)
            };

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
            let (s, mut e) = if m <= cur {
                (m, cur)
            } else {
                (cur, m)
            };

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

        let ignored = [
            ".git",
            "node_modules",
            "dist",
            "build",
            ".cache",
            ".npm",
            ".cargo",
            ".rustup",
            "venv",
            ".venv",
            "__pycache__",
            "target",
            "vendor",
            "staticfiles",
            "coverage",
        ];

        let mut cmd = format!("find {} -mindepth 1 -maxdepth 6 ", self.cwd.display());

        if !ignored.is_empty() {
            let expr = ignored
                .iter()
                .map(|name| format!("-name '{name}'"))
                .collect::<Vec<_>>()
                .join(" -o ");

            cmd.push_str(&format!("\\( {expr} \\) -prune -o -type d -print"));
        } else {
            cmd.push_str("-type d -print");
        }

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&format!("{cmd} 2>/dev/null"))
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
            Some(entries),
        ));
    }
}
