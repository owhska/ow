// Included into editor.rs: the Emacs command set.

impl Editor {
    // ----------------------------------------------------------- movement
    fn forward_char(&mut self) {
        let b = self.buf_mut();
        match b.next_pos(b.cursor) {
            Some(p) => b.cursor = p,
            None => self.say("End of buffer"),
        }
    }
    fn backward_char(&mut self) {
        let b = self.buf_mut();
        match b.prev_pos(b.cursor) {
            Some(p) => b.cursor = p,
            None => self.say("Beginning of buffer"),
        }
    }
    pub fn next_line(&mut self) {
        let b = self.buf_mut();
        if b.cursor.line + 1 >= b.lines.len() {
            b.cursor = b.end();
            self.say("End of buffer");
            return;
        }
        let goal = *b.goal.get_or_insert(b.cursor.col);
        b.cursor.line += 1;
        b.cursor.col = goal.min(b.line_len(b.cursor.line));
    }
    pub fn previous_line(&mut self) {
        let b = self.buf_mut();
        if b.cursor.line == 0 {
            b.cursor.col = 0;
            self.say("Beginning of buffer");
            return;
        }
        let goal = *b.goal.get_or_insert(b.cursor.col);
        b.cursor.line -= 1;
        b.cursor.col = goal.min(b.line_len(b.cursor.line));
    }
    fn bol(&mut self) {
        self.buf_mut().cursor.col = 0;
    }
    fn eol(&mut self) {
        let b = self.buf_mut();
        b.cursor.col = b.line_len(b.cursor.line);
    }
    fn fwd_word(&mut self) {
        let b = self.buf_mut();
        b.cursor = b.forward_word(b.cursor);
    }
    fn bwd_word(&mut self) {
        let b = self.buf_mut();
        b.cursor = b.backward_word(b.cursor);
    }
    fn bob(&mut self) {
        let b = self.buf_mut();
        b.mark = Some(b.cursor);
        b.mark_active = false;
        b.cursor = Pos::default();
    }
    fn eob(&mut self) {
        let b = self.buf_mut();
        b.mark = Some(b.cursor);
        b.mark_active = false;
        b.cursor = b.end();
    }
    fn page(&self) -> usize {
        self.text_h.saturating_sub(2).max(1)
    }
    fn scroll_up(&mut self) {
        let page = self.page();
        let b = self.buf_mut();
        let last = b.lines.len() - 1;
        if b.cursor.line >= last {
            self.say("End of buffer");
            return;
        }
        let goal = *b.goal.get_or_insert(b.cursor.col);
        b.top = (b.top + page).min(last);
        b.cursor.line = (b.cursor.line + page).min(last);
        b.cursor.col = goal.min(b.line_len(b.cursor.line));
    }
    fn scroll_down(&mut self) {
        let page = self.page();
        let b = self.buf_mut();
        if b.cursor.line == 0 {
            self.say("Beginning of buffer");
            return;
        }
        let goal = *b.goal.get_or_insert(b.cursor.col);
        b.top = b.top.saturating_sub(page);
        b.cursor.line = b.cursor.line.saturating_sub(page);
        b.cursor.col = goal.min(b.line_len(b.cursor.line));
    }
    fn recenter(&mut self) {
        let h = self.text_h;
        let b = self.buf_mut();
        b.top = b.cursor.line.saturating_sub(h / 2);
    }
    fn goto_line_cmd(&mut self) {
        self.start_prompt(Prompt::new("Goto line: ", PKind::Goto, None));
    }
    fn back_to_indentation(&mut self) {
        let b = self.buf_mut();
        b.cursor.col = b.indent_of(b.cursor.line).chars().count();
    }
    fn forward_paragraph(&mut self) {
        let b = self.buf_mut();
        let n = b.lines.len();
        let mut i = b.cursor.line;
        while i < n && b.lines[i].trim().is_empty() {
            i += 1;
        }
        while i < n && !b.lines[i].trim().is_empty() {
            i += 1;
        }
        b.cursor = if i >= n { b.end() } else { Pos::new(i, 0) };
    }
    fn backward_paragraph(&mut self) {
        let b = self.buf_mut();
        let mut i = b.cursor.line as isize;
        while i >= 0 && b.lines[i as usize].trim().is_empty() {
            i -= 1;
        }
        while i >= 0 && !b.lines[i as usize].trim().is_empty() {
            i -= 1;
        }
        b.cursor = Pos::new(i.max(0) as usize, 0);
    }

    // ------------------------------------------------------------ editing
    fn self_insert(&mut self) {
        let Some(c) = self.last_char else { return };
        let b = self.buf_mut();
        let p = b.cursor;
        let e = b.insert(p, &c.to_string());
        b.cursor = e;
    }

    fn indent_unit_str(&self) -> String {
        let b = self.buf();
        if b.use_tabs { "\t".to_string() } else { " ".repeat(b.indent_unit) }
    }

    fn newline(&mut self) {
        if self.auto_indent {
            self.newline_indent();
        } else {
            let b = self.buf_mut();
            let p = b.cursor;
            let e = b.insert(p, "\n");
            b.cursor = e;
        }
    }

    fn newline_indent(&mut self) {
        let unit = self.indent_unit_str();
        let python = self.buf().hl.name == "Python";
        let b = self.buf_mut();
        let p = b.cursor;
        let line: Vec<char> = b.lines[p.line].chars().collect();
        let lead: String = line.iter().take_while(|c| **c == ' ' || **c == '\t').take(p.col).collect();
        let before: String = line[..p.col].iter().collect();
        let t = before.trim_end();
        let opener = t.ends_with('{') || t.ends_with('(') || t.ends_with('[') || (python && t.ends_with(':'));
        let next = line.get(p.col).copied();
        let closer = matches!((t.chars().last(), next), (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']')));
        if closer {
            let text = format!("\n{lead}{unit}\n{lead}");
            let e = b.insert(p, &text);
            b.cursor = Pos::new(p.line + 1, lead.chars().count() + unit.chars().count());
            let _ = e;
        } else {
            let ind = if opener { format!("{lead}{unit}") } else { lead };
            let text = format!("\n{ind}");
            // don't leave trailing whitespace behind on whitespace-only lines
            let e = b.insert(p, &text);
            b.cursor = e;
        }
    }

    fn open_line(&mut self) {
        let b = self.buf_mut();
        let p = b.cursor;
        b.insert(p, "\n");
        b.cursor = p;
    }

    fn delete_char(&mut self) {
        let b = self.buf_mut();
        match b.next_pos(b.cursor) {
            Some(n) => b.delete(b.cursor, n),
            None => self.say("End of buffer"),
        }
    }

    fn delete_backward(&mut self) {
        let b = self.buf_mut();
        match b.prev_pos(b.cursor) {
            Some(p) => {
                b.delete(p, b.cursor);
                b.cursor = p;
            }
            None => self.say("Beginning of buffer"),
        }
    }

    pub fn kill(&mut self, text: &str, prepend: bool) {
        if text.is_empty() {
            return;
        }
        if self.prev_kill && !self.kill_ring.is_empty() {
            let last = self.kill_ring.last_mut().unwrap();
            if prepend {
                *last = format!("{text}{last}");
            } else {
                last.push_str(text);
            }
        } else {
            self.kill_ring.push(text.to_string());
            if self.kill_ring.len() > 100 {
                self.kill_ring.remove(0);
            }
        }
        self.clip = self.kill_ring.last().cloned();
        if let Some(c) = &self.clip {
            crate::clipboard::set_clipboard(c);
        }
        self.this_kill = true;
    }

    fn kill_line(&mut self) {
        let b = self.buf();
        let p = b.cursor;
        let len = b.line_len(p.line);
        let end = if p.col < len {
            Pos::new(p.line, len)
        } else if p.line + 1 < b.lines.len() {
            Pos::new(p.line + 1, 0)
        } else {
            self.say("End of buffer");
            return;
        };
        let text = b.text_in(p, end);
        self.buf_mut().delete(p, end);
        self.kill(&text, false);
    }

    fn kill_word(&mut self) {
        let b = self.buf();
        let p = b.cursor;
        let e = b.forward_word(p);
        let text = b.text_in(p, e);
        self.buf_mut().delete(p, e);
        self.kill(&text, false);
    }

    fn backward_kill_word(&mut self) {
        let b = self.buf();
        let p = b.cursor;
        let s = b.backward_word(p);
        let text = b.text_in(s, p);
        let bm = self.buf_mut();
        bm.delete(s, p);
        bm.cursor = s;
        self.kill(&text, true);
    }

    fn kill_region(&mut self) {
        let b = self.buf();
        let Some(m) = b.mark else {
            self.say("The mark is not set now, so there is no region");
            return;
        };
        let p = b.cursor;
        let (a, e) = if m <= p { (m, p) } else { (p, m) };
        let text = b.text_in(a, e);
        let bm = self.buf_mut();
        bm.delete(a, e);
        bm.cursor = a;
        bm.mark_active = false;
        self.kill(&text, false);
    }

    fn copy_region(&mut self) {
        let b = self.buf();
        let Some(m) = b.mark else {
            self.say("The mark is not set now, so there is no region");
            return;
        };
        let text = b.text_in(m, b.cursor);
        self.buf_mut().mark_active = false;
        let pk = self.prev_kill;
        self.prev_kill = false;
        self.kill(&text, false);
        self.prev_kill = pk;
        self.this_kill = false;
        self.say("Region copied");
    }

    fn insert_yank(&mut self, t: &str) {
        let b = self.buf_mut();
        let p = b.cursor;
        let e = b.insert(p, t);
        b.cursor = e;
        b.mark = Some(p);
        b.mark_active = false;
        self.yank_span = Some((p, e));
    }

    fn yank(&mut self) {
        if let Some(sys) = crate::clipboard::get_clipboard() {
            if self.kill_ring.last().map(|s| s.as_str()) != Some(&sys) {
                self.kill_ring.push(sys);
                if self.kill_ring.len() > 100 {
                    self.kill_ring.remove(0);
                }
            }
        }
        let Some(t) = self.kill_ring.last().cloned() else {
            self.say("Kill ring is empty");
            return;
        };
        self.yank_n = 0;
        self.insert_yank(&t);
    }

    fn yank_pop(&mut self) {
        let (true, Some((a, e))) = (matches!(self.last_cmd.as_str(), "yank" | "yank-pop"), self.yank_span) else {
            self.say("Previous command was not a yank");
            return;
        };
        let n = self.kill_ring.len();
        self.yank_n = (self.yank_n + 1) % n;
        let t = self.kill_ring[n - 1 - self.yank_n].clone();
        let b = self.buf_mut();
        b.delete(a, e);
        b.cursor = a;
        self.insert_yank(&t);
    }

    fn set_mark(&mut self) {
        let b = self.buf_mut();
        b.mark = Some(b.cursor);
        b.mark_active = true;
        self.say("Mark set");
    }

    fn exchange_mark(&mut self) {
        let b = self.buf_mut();
        if let Some(m) = b.mark {
            b.mark = Some(b.cursor);
            b.cursor = b.clamp(m);
            b.mark_active = true;
        } else {
            self.say("No mark set in this buffer");
        }
    }

    fn mark_whole(&mut self) {
        let b = self.buf_mut();
        b.mark = Some(b.end());
        b.cursor = Pos::default();
        b.mark_active = true;
    }

    fn transpose_chars(&mut self) {
        let b = self.buf_mut();
        let p = b.cursor;
        let len = b.line_len(p.line);
        let (a, e) = if p.col >= len {
            if len < 2 {
                return;
            }
            (len - 2, len)
        } else if p.col == 0 {
            return;
        } else {
            (p.col - 1, p.col + 1)
        };
        let s = b.text_in(Pos::new(p.line, a), Pos::new(p.line, e));
        let sw: String = s.chars().rev().collect();
        b.replace(Pos::new(p.line, a), Pos::new(p.line, e), &sw);
        b.cursor = Pos::new(p.line, e);
    }

    fn case_word(&mut self, mode: u8) {
        let b = self.buf_mut();
        let p = b.cursor;
        let e = b.forward_word(p);
        let t = b.text_in(p, e);
        let new: String = match mode {
            0 => t.to_uppercase(),
            1 => t.to_lowercase(),
            _ => {
                let mut first = true;
                t.chars()
                    .map(|c| {
                        if c.is_alphanumeric() && first {
                            first = false;
                            c.to_uppercase().collect::<String>()
                        } else {
                            c.to_lowercase().collect::<String>()
                        }
                    })
                    .collect()
            }
        };
        let end = b.replace(p, e, &new);
        b.cursor = end;
    }

    fn ws_around(&self) -> (Pos, Pos) {
        let b = self.buf();
        let p = b.cursor;
        let chars: Vec<char> = b.lines[p.line].chars().collect();
        let (mut s, mut e) = (p.col, p.col);
        while s > 0 && (chars[s - 1] == ' ' || chars[s - 1] == '\t') {
            s -= 1;
        }
        while e < chars.len() && (chars[e] == ' ' || chars[e] == '\t') {
            e += 1;
        }
        (Pos::new(p.line, s), Pos::new(p.line, e))
    }

    fn delete_horizontal_space(&mut self) {
        let (s, e) = self.ws_around();
        let b = self.buf_mut();
        b.delete(s, e);
        b.cursor = s;
    }

    fn just_one_space(&mut self) {
        let (s, e) = self.ws_around();
        let b = self.buf_mut();
        let end = b.replace(s, e, " ");
        b.cursor = end;
    }

    fn delete_indentation(&mut self) {
        let b = self.buf_mut();
        let l = b.cursor.line;
        if l == 0 {
            return;
        }
        let prev_len = b.line_len(l - 1);
        let ws = b.indent_of(l).chars().count();
        let empty_after = b.line_len(l) == ws;
        let sep = if prev_len == 0 || empty_after { "" } else { " " };
        b.replace(Pos::new(l - 1, prev_len), Pos::new(l, ws), sep);
        b.cursor = Pos::new(l - 1, prev_len);
    }

    fn region_lines(&self) -> Option<(usize, usize)> {
        let b = self.buf();
        let m = b.mark?;
        if !b.mark_active {
            return None;
        }
        let (a, e) = if m <= b.cursor { (m, b.cursor) } else { (b.cursor, m) };
        let end = if e.col == 0 && e.line > a.line { e.line - 1 } else { e.line };
        Some((a.line, end))
    }

    fn indent_tab(&mut self) {
        let unit = self.indent_unit_str();
        if let Some((l0, l1)) = self.region_lines() {
            let b = self.buf_mut();
            for i in l0..=l1 {
                if !b.lines[i].is_empty() {
                    b.insert(Pos::new(i, 0), &unit);
                }
            }
            b.cursor = b.clamp(b.cursor);
            return;
        }
        let b = self.buf_mut();
        let p = b.cursor;
        let text = if b.use_tabs {
            "\t".to_string()
        } else {
            let u = b.indent_unit.max(1);
            " ".repeat(u - p.col % u)
        };
        let e = b.insert(p, &text);
        b.cursor = e;
    }

    fn unindent(&mut self) {
        let (l0, l1) = self.region_lines().unwrap_or((self.buf().cursor.line, self.buf().cursor.line));
        let b = self.buf_mut();
        let unit = b.indent_unit;
        for i in l0..=l1 {
            let n = if b.lines[i].starts_with('\t') {
                1
            } else {
                b.lines[i].chars().take(unit).take_while(|c| *c == ' ').count()
            };
            if n > 0 {
                b.delete(Pos::new(i, 0), Pos::new(i, n));
            }
        }
        b.cursor = b.clamp(b.cursor);
    }

    fn comment_dwim(&mut self) {
        let Some(tok) = self.buf().hl.comment_token() else {
            self.say("No comment syntax known for this language");
            return;
        };
        let cur = self.buf().cursor.line;
        let (l0, l1) = self.region_lines().unwrap_or((cur, cur));
        let b = self.buf_mut();
        let idxs: Vec<usize> = (l0..=l1).filter(|&i| !b.lines[i].trim().is_empty()).collect();
        if idxs.is_empty() {
            return;
        }
        let tlen = tok.chars().count();
        if idxs.iter().all(|&i| b.lines[i].trim_start().starts_with(tok)) {
            for &i in &idxs {
                let ws = b.indent_of(i).chars().count();
                let after = b.lines[i].chars().nth(ws + tlen);
                let n = tlen + usize::from(after == Some(' '));
                b.delete(Pos::new(i, ws), Pos::new(i, ws + n));
            }
        } else {
            let col = idxs.iter().map(|&i| b.indent_of(i).chars().count()).min().unwrap_or(0);
            for &i in &idxs {
                b.insert(Pos::new(i, col), &format!("{tok} "));
            }
        }
        b.cursor = b.clamp(b.cursor);
        b.mark_active = false;
    }

    fn delete_trailing_ws(&mut self) {
        let b = self.buf_mut();
        let mut n = 0;
        for i in 0..b.lines.len() {
            let t = b.lines[i].trim_end().chars().count();
            let len = b.line_len(i);
            if t < len {
                b.delete(Pos::new(i, t), Pos::new(i, len));
                n += 1;
            }
        }
        b.cursor = b.clamp(b.cursor);
        self.say(format!("Deleted trailing whitespace on {n} line(s)"));
    }

    fn undo_cmd(&mut self) {
        if self.buf_mut().undo() {
            self.say("Undo!");
        } else {
            self.say("No further undo information");
        }
    }
    fn redo_cmd(&mut self) {
        if self.buf_mut().redo() {
            self.say("Redo!");
        } else {
            self.say("Nothing to redo");
        }
    }

    // -------------------------------------------------------- files/buffers
    fn find_file_cmd(&mut self) {
        let dir = self.buf().path.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf());
        let pre = match dir {
            Some(d) if d != self.cwd => {
                let rel = d.strip_prefix(&self.cwd).map(|r| r.to_path_buf()).unwrap_or(d);
                format!("{}/", rel.display())
            }
            _ => String::new(),
        };
        let pre = if pre == "/" { pre } else { pre };
        self.start_prompt(Prompt::new("Find file: ", PKind::FindFile, None).with_input(&pre));
    }

    fn save_cmd(&mut self) {
        if self.buf().kind != BufKind::File {
            self.say("Not a file buffer");
        } else if self.buf().path.is_none() {
            self.start_prompt(Prompt::new("Write file: ", PKind::WriteFile, None));
        } else if !self.buf().modified && self.buf().path.as_ref().is_some_and(|p| p.exists()) {
            self.say("(No changes need to be saved)");
        } else {
            self.save_current(None);
        }
    }

    fn write_file_cmd(&mut self) {
        let pre = self.buf().path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.start_prompt(Prompt::new("Write file: ", PKind::WriteFile, None).with_input(&pre));
    }

    fn save_some(&mut self) {
        let mut n = 0;
        let mut errs = 0;
        for i in 0..self.bufs.len() {
            let b = &mut self.bufs[i];
            if b.kind == BufKind::File && b.modified && b.path.is_some() {
                if b.save(None).is_ok() {
                    n += 1;
                } else {
                    errs += 1;
                }
            }
        }
        self.refresh_git_info();
        self.say(if errs > 0 { format!("Saved {n} buffer(s), {errs} failed") } else { format!("Saved {n} buffer(s)") });
    }

    fn buffer_choices(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        if let Some(p) = self.prev_cur.filter(|p| *p < self.bufs.len() && *p != self.cur) {
            v.push(self.bufs[p].name.clone());
        }
        for (i, b) in self.bufs.iter().enumerate() {
            if i != self.cur && Some(i) != self.prev_cur {
                v.push(b.name.clone());
            }
        }
        v.push(self.bufs[self.cur].name.clone());
        v
    }

    fn switch_buffer_cmd(&mut self) {
        let ch = self.buffer_choices();
        self.start_prompt(Prompt::new("Switch to buffer: ", PKind::Switch, Some(ch)));
    }

    fn kill_buffer_cmd(&mut self) {
        let mut ch = self.buffer_choices();
        let cur = ch.pop().unwrap_or_default();
        ch.insert(0, cur);
        self.start_prompt(Prompt::new("Kill buffer: ", PKind::Kill, Some(ch)));
    }

    fn list_buffers(&mut self) {
        let mut text = String::from(" MR  Name                     Lines  File\n");
        let mut meta = vec![LineMeta::None];
        for b in &self.bufs {
            text.push_str(&format!(
                " {}{}  {:<24} {:>5}  {}\n",
                if b.modified { '*' } else { ' ' },
                if b.read_only { '%' } else { ' ' },
                b.name,
                b.lines.len(),
                b.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
            ));
            meta.push(LineMeta::Buf(b.name.clone()));
        }
        text.push_str("\n RET switch   d kill   q close\n");
        meta.push(LineMeta::None);
        meta.push(LineMeta::None);
        self.open_special("*Buffers*", BufKind::List, &text, meta, false);
        self.buf_mut().cursor = Pos::new(1, 0);
    }

    fn next_buffer(&mut self) {
        let i = (self.cur + 1) % self.bufs.len();
        self.switch_to(i);
    }
    fn prev_buffer(&mut self) {
        let i = (self.cur + self.bufs.len() - 1) % self.bufs.len();
        self.switch_to(i);
    }

    fn quit_cmd(&mut self) {
        let n = self.bufs.iter().filter(|b| b.kind == BufKind::File && b.modified && b.path.is_some()).count();
        if n > 0 {
            self.mode = Mode::YesNo(YesNo { q: format!("{n} modified buffer(s) not saved; exit anyway? (y or n) "), act: YAct::Quit });
        } else {
            self.quit = true;
        }
    }

    fn dired_cmd(&mut self) {
        let dir = self.buf().path.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf()).unwrap_or_else(|| self.cwd.clone());
        self.open_dir(&dir);
    }

    fn revert_cmd(&mut self) {
        if self.buf().path.is_some() {
            let i = self.cur;
            self.reload_buffer(i);
            self.say("Reverted");
        }
    }

    fn extended_command(&mut self) {
        let names: Vec<String> = COMMANDS.iter().map(|c| c.0.to_string()).collect();
        self.start_prompt(Prompt::new("M-x ", PKind::Command, Some(names)));
    }

    fn universal_argument(&mut self) {
        self.arg = Some(self.arg.map(|a| a * 4).unwrap_or(4));
        self.arg_typing = true;
        self.arg_digits = false;
        let a = self.arg.unwrap_or(4);
        self.say(format!("C-u {a}"));
    }

    fn describe_bindings(&mut self) {
        let mut by_cmd: Vec<(&str, Vec<&str>)> = Vec::new();
        for (k, c) in BINDINGS {
            match by_cmd.iter_mut().find(|e| e.0 == *c) {
                Some(e) => e.1.push(k),
                None => by_cmd.push((c, vec![k])),
            }
        }
        by_cmd.sort_by(|a, b| a.0.cmp(b.0));
        let mut t = String::from("Key bindings (C- = Ctrl, M- = Alt or ESC prefix)\n\n");
        for (c, ks) in &by_cmd {
            t.push_str(&format!("  {:<30} {}\n", ks.join(", "), c));
        }
        t.push_str("\nOther commands (M-x name):\n\n");
        for (c, _) in COMMANDS {
            if !by_cmd.iter().any(|e| e.0 == *c) {
                t.push_str(&format!("  {c}\n"));
            }
        }
        t.push_str("\nDired buffer (C-x d):\n");
        t.push_str("  Navigation (vim-style):\n");
        t.push_str("    j / k / down / up   next / previous line\n");
        t.push_str("    l / RET / right      open file or enter directory\n");
        t.push_str("    h / left             go up to parent directory\n");
        t.push_str("    gg                   top of listing\n");
        t.push_str("    G                    bottom of listing\n");
        t.push_str("    C-f / C-d            page down\n");
        t.push_str("    C-b / C-u            page up\n");
        t.push_str("  File operations:\n");
        t.push_str("    %                    create file (append / for directory)\n");
        t.push_str("    r                    rename / move entry under cursor\n");
        t.push_str("    d                    delete file or directory (recursive)\n");
        t.push_str("    c                    copy entry to clipboard\n");
        t.push_str("    x                    cut (move) entry to clipboard\n");
        t.push_str("    p                    paste clipboard into current directory\n");
        t.push_str("  Other:\n");
        t.push_str("    g                    refresh listing\n");
        t.push_str("    q                    close dired buffer\n");
        t.push_str("    SPC ...              leader key (same as normal mode)\n");
        t.push_str("\nGit status buffer (C-x g):\n");
        t.push_str("  s / u            stage / unstage entry\n");
        t.push_str("  RET              visit file\n");
        t.push_str("  d / D            diff file / diff all\n");
        t.push_str("  c / C            commit / amend\n");
        t.push_str("  l                log\n");
        t.push_str("  b                checkout branch\n");
        t.push_str("  x                discard changes\n");
        t.push_str("  P / F            push / pull\n");
        t.push_str("  g                refresh\n");
        t.push_str("  q                quit\n");
        t.push_str("\nGit log buffer: RET show commit.\n");
        t.push_str("Diff buffers: n / p next / previous hunk.\n");
        t.push_str("\nISearch (C-s / C-r):\n");
        t.push_str("  C-s / C-r        search forward / backward (repeat to advance)\n");
        t.push_str("  DEL              remove last char from query\n");
        t.push_str("  C-w              append word at point to query\n");
        t.push_str("  RET              finish search\n");
        t.push_str("  C-g              cancel and return to origin\n");
        t.push_str("\nQuery-replace (M-%):\n");
        t.push_str("  y / SPC          replace this occurrence and advance\n");
        t.push_str("  n / DEL          skip this occurrence\n");
        t.push_str("  !                replace all remaining\n");
        t.push_str("  .                replace this one and stop\n");
        t.push_str("  q / RET / C-g    quit\n");
        t.push_str("\nYes/No prompts:\n");
        t.push_str("  y / Y / SPC      confirm\n");
        t.push_str("  n / N / C-g / ESC  cancel\n");
        t.push_str("\nMinibuffer / Prompt:\n");
        t.push_str("  TAB              complete / next match\n");
        t.push_str("  S-TAB            previous match\n");
        t.push_str("  C-n / down       next candidate\n");
        t.push_str("  up               previous candidate\n");
        t.push_str("  C-a / home       beginning of input\n");
        t.push_str("  C-e / end        end of input\n");
        t.push_str("  C-b / left       backward char\n");
        t.push_str("  C-f / right      forward char\n");
        t.push_str("  DEL              delete backward\n");
        t.push_str("  C-d / delete     delete forward\n");
        t.push_str("  C-k              kill to end\n");
        t.push_str("  C-u              clear input\n");
        t.push_str("  C-w / M-DEL      kill word backward\n");
        t.push_str("  C-y              yank from kill ring\n");
        t.push_str("  RET              confirm\n");
        t.push_str("  C-g / ESC        cancel\n");
        self.open_special("*Help*", BufKind::Help, &t, Vec::new(), false);
    }

    fn set_language_cmd(&mut self) {
        self.start_prompt(Prompt::new("Language: ", PKind::SetLang, Some(Highlighter::list_names())));
    }
}

type Cmd = fn(&mut Editor);

pub static COMMANDS: &[(&str, Cmd)] = &[
    ("forward-char", |e| e.forward_char()),
    ("backward-char", |e| e.backward_char()),
    ("next-line", |e| e.next_line()),
    ("previous-line", |e| e.previous_line()),
    ("move-beginning-of-line", |e| e.bol()),
    ("move-end-of-line", |e| e.eol()),
    ("forward-word", |e| e.fwd_word()),
    ("backward-word", |e| e.bwd_word()),
    ("beginning-of-buffer", |e| e.bob()),
    ("end-of-buffer", |e| e.eob()),
    ("scroll-up-command", |e| e.scroll_up()),
    ("scroll-down-command", |e| e.scroll_down()),
    ("recenter", |e| e.recenter()),
    ("goto-line", |e| e.goto_line_cmd()),
    ("back-to-indentation", |e| e.back_to_indentation()),
    ("forward-paragraph", |e| e.forward_paragraph()),
    ("backward-paragraph", |e| e.backward_paragraph()),
    ("self-insert-command", |e| e.self_insert()),
    ("newline", |e| e.newline()),
    ("newline-and-indent", |e| e.newline_indent()),
    ("open-line", |e| e.open_line()),
    ("delete-char", |e| e.delete_char()),
    ("delete-backward-char", |e| e.delete_backward()),
    ("kill-line", |e| e.kill_line()),
    ("kill-word", |e| e.kill_word()),
    ("backward-kill-word", |e| e.backward_kill_word()),
    ("kill-region", |e| e.kill_region()),
    ("kill-ring-save", |e| e.copy_region()),
    ("yank", |e| e.yank()),
    ("yank-pop", |e| e.yank_pop()),
    ("set-mark-command", |e| e.set_mark()),
    ("exchange-point-and-mark", |e| e.exchange_mark()),
    ("mark-whole-buffer", |e| e.mark_whole()),
    ("transpose-chars", |e| e.transpose_chars()),
    ("upcase-word", |e| e.case_word(0)),
    ("downcase-word", |e| e.case_word(1)),
    ("capitalize-word", |e| e.case_word(2)),
    ("delete-horizontal-space", |e| e.delete_horizontal_space()),
    ("just-one-space", |e| e.just_one_space()),
    ("delete-indentation", |e| e.delete_indentation()),
    ("indent-for-tab-command", |e| e.indent_tab()),
    ("unindent", |e| e.unindent()),
    ("comment-dwim", |e| e.comment_dwim()),
    ("delete-trailing-whitespace", |e| e.delete_trailing_ws()),
    ("undo", |e| e.undo_cmd()),
    ("redo", |e| e.redo_cmd()),
    ("quoted-insert", |e| {
        e.quote_next = true;
        e.say("C-q-");
    }),
    ("keyboard-quit", |e| {
        e.buf_mut().mark_active = false;
        e.say("Quit");
    }),
    ("isearch-forward", |e| e.isearch_start(true)),
    ("isearch-backward", |e| e.isearch_start(false)),
    ("query-replace", |e| e.start_prompt(Prompt::new("Query replace: ", PKind::QrFrom, None))),
    ("find-file", |e| e.find_file_cmd()),
    ("save-buffer", |e| e.save_cmd()),
    ("write-file", |e| e.write_file_cmd()),
    ("save-some-buffers", |e| e.save_some()),
    ("switch-to-buffer", |e| e.switch_buffer_cmd()),
    ("kill-buffer", |e| e.kill_buffer_cmd()),
    ("list-buffers", |e| e.list_buffers()),
    ("next-buffer", |e| e.next_buffer()),
    ("previous-buffer", |e| e.prev_buffer()),
    ("revert-buffer", |e| e.revert_cmd()),
    ("dired", |e| e.dired_cmd()),
    ("save-buffers-kill-terminal", |e| e.quit_cmd()),
    ("execute-extended-command", |e| e.extended_command()),
    ("universal-argument", |e| e.universal_argument()),
    ("describe-bindings", |e| e.describe_bindings()),
    ("set-language", |e| e.set_language_cmd()),
    ("toggle-line-numbers", |e| {
        e.line_numbers = !e.line_numbers;
        let s = if e.line_numbers { "on" } else { "off" };
        e.say(format!("Line numbers {s}"));
    }),
    ("auto-indent-mode", |e| {
        e.auto_indent = !e.auto_indent;
        let s = if e.auto_indent { "on" } else { "off" };
        e.say(format!("Auto indent {s}"));
    }),
    ("set-tab-width", |e| e.start_prompt(Prompt::new("Tab width: ", PKind::TabWidth, None))),
    ("toggle-indent-tabs-mode", |e| {
        let b = e.buf_mut();
        b.use_tabs = !b.use_tabs;
        let s = if b.use_tabs { "tabs" } else { "spaces" };
        e.say(format!("Indenting with {s}"));
    }),
    // git
    ("git-status", |e| e.git_status()),
    ("git-diff", |e| e.git_diff_repo()),
    ("git-diff-staged", |e| e.git_diff_staged()),
    ("git-diff-file", |e| e.git_diff_file()),
    ("git-log", |e| e.git_log(false)),
    ("git-log-file", |e| e.git_log(true)),
    ("git-blame", |e| e.git_blame()),
    ("git-blame-line", |e| e.git_blame_line()),
    ("git-stage-file", |e| e.git_stage_file(true)),
    ("git-unstage-file", |e| e.git_stage_file(false)),
    ("git-stage-all", |e| e.git_stage_all()),
    ("git-commit", |e| e.git_commit_cmd(false)),
    ("git-commit-amend", |e| e.git_commit_cmd(true)),
    ("git-checkout", |e| e.git_checkout_cmd()),
    ("git-restore-file", |e| e.git_restore_cmd()),
    ("git-next-hunk", |e| e.git_hunk(true)),
    ("git-previous-hunk", |e| e.git_hunk(false)),
    ("git-pull", |e| e.git_pull()),
    ("git-push", |e| e.git_push()),
    ("git-init", |e| e.git_init()),
    ("project-find-file", |e| e.project_find_file()),
    ("project-grep", |e| e.project_grep_cmd()),
    ("next-error", |e| e.next_error(true)),
    ("previous-error", |e| e.next_error(false)),
    ("split-window-below", |e| e.split_window_below()),
    ("split-window-right", |e| e.split_window_right()),
    ("delete-other-windows", |e| e.delete_other_windows()),
    ("delete-window", |e| e.delete_window()),
    ("other-window", |e| e.other_window()),
    ("balance-windows", |e| e.balance_windows()),
];

pub static REPEATABLE: &[&str] = &[
    "other-window",
    "forward-char",
    "backward-char",
    "next-line",
    "previous-line",
    "forward-word",
    "backward-word",
    "delete-char",
    "delete-backward-char",
    "kill-word",
    "backward-kill-word",
    "self-insert-command",
    "undo",
    "redo",
    "transpose-chars",
    "newline",
    "open-line",
    "scroll-up-command",
    "scroll-down-command",
    "forward-paragraph",
    "backward-paragraph",
    "kill-line",
    "yank",
    "upcase-word",
    "downcase-word",
    "capitalize-word",
];

pub static BINDINGS: &[(&str, &str)] = &[
    ("C-f", "forward-char"),
    ("<right>", "forward-char"),
    ("C-b", "backward-char"),
    ("<left>", "backward-char"),
    ("C-n", "next-line"),
    ("<down>", "next-line"),
    ("C-p", "previous-line"),
    ("<up>", "previous-line"),
    ("C-a", "move-beginning-of-line"),
    ("<home>", "move-beginning-of-line"),
    ("C-e", "move-end-of-line"),
    ("<end>", "move-end-of-line"),
    ("M-f", "forward-word"),
    ("C-<right>", "forward-word"),
    ("M-b", "backward-word"),
    ("C-<left>", "backward-word"),
    ("M-<", "beginning-of-buffer"),
    ("C-<home>", "beginning-of-buffer"),
    ("M->", "end-of-buffer"),
    ("C-<end>", "end-of-buffer"),
    ("C-v", "scroll-up-command"),
    ("<pagedown>", "scroll-up-command"),
    ("M-v", "scroll-down-command"),
    ("<pageup>", "scroll-down-command"),
    ("C-l", "recenter"),
    ("M-g g", "goto-line"),
    ("M-g M-g", "goto-line"),
    ("M-g n", "next-error"),
    ("M-g p", "previous-error"),
    ("M-m", "back-to-indentation"),
    ("M-}", "forward-paragraph"),
    ("M-{", "backward-paragraph"),
    ("C-d", "delete-char"),
    ("<delete>", "delete-char"),
    ("DEL", "delete-backward-char"),
    ("C-k", "kill-line"),
    ("M-d", "kill-word"),
    ("M-DEL", "backward-kill-word"),
    ("C-DEL", "backward-kill-word"),
    ("C-w", "kill-region"),
    ("M-w", "kill-ring-save"),
    ("C-y", "yank"),
    ("M-y", "yank-pop"),
    ("C-SPC", "set-mark-command"),
    ("C-x C-x", "exchange-point-and-mark"),
    ("C-x h", "mark-whole-buffer"),
    ("RET", "newline"),
    ("C-j", "newline-and-indent"),
    ("C-o", "open-line"),
    ("C-t", "transpose-chars"),
    ("M-u", "upcase-word"),
    ("M-l", "downcase-word"),
    ("M-c", "capitalize-word"),
    ("M-\\", "delete-horizontal-space"),
    ("M-SPC", "just-one-space"),
    ("M-^", "delete-indentation"),
    ("TAB", "indent-for-tab-command"),
    ("S-TAB", "unindent"),
    ("M-;", "comment-dwim"),
    ("C-q", "quoted-insert"),
    ("C-_", "undo"),
    ("C-/", "undo"),
    ("C-x u", "undo"),
    ("M-_", "redo"),
    ("C-s", "isearch-forward"),
    ("C-r", "isearch-backward"),
    ("M-%", "query-replace"),
    ("C-x C-f", "find-file"),
    ("C-x C-s", "save-buffer"),
    ("C-x C-w", "write-file"),
    ("C-x s", "save-some-buffers"),
    ("C-x b", "switch-to-buffer"),
    ("C-x k", "kill-buffer"),
    ("C-x C-b", "list-buffers"),
    ("C-x <right>", "next-buffer"),
    ("C-x <left>", "previous-buffer"),
    ("C-x d", "dired"),
    ("C-x C-c", "save-buffers-kill-terminal"),
    ("M-x", "execute-extended-command"),
    ("C-u", "universal-argument"),
    ("C-h b", "describe-bindings"),
    ("C-x g", "git-status"),
    ("C-x v d", "git-status"),
    ("C-x v =", "git-diff-file"),
    ("C-x v D", "git-diff"),
    ("C-x v l", "git-log"),
    ("C-x v L", "git-log-file"),
    ("C-x v g", "git-blame"),
    ("C-x v b", "git-blame-line"),
    ("C-x v a", "git-stage-file"),
    ("C-x v u", "git-unstage-file"),
    ("C-x v c", "git-commit"),
    ("C-x v A", "git-commit-amend"),
    ("C-x v o", "git-checkout"),
    ("C-x v r", "git-restore-file"),
    ("C-x v n", "git-next-hunk"),
    ("C-x v p", "git-previous-hunk"),
    ("C-x v F", "git-pull"),
    ("C-x v P", "git-push"),
    ("C-x p f", "project-find-file"),
    ("C-x p g", "project-grep"),
    ("C-x 0", "delete-window"),
    ("C-x 1", "delete-other-windows"),
    ("C-x 2", "split-window-below"),
    ("C-x 3", "split-window-right"),
    ("C-x o", "other-window"),
];

include!("gitcmds.rs");
