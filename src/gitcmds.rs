// Included into editor.rs: git + project commands built on libgit2.

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Editor {
    fn need_repo(&mut self) -> Option<git2::Repository> {
        let r = self.repo_here();
        if r.is_none() {
            self.say("Not a git repository (M-x git-init to create one)");
        }
        r
    }

    fn cur_rel(&mut self, repo: &git2::Repository) -> Option<String> {
        let rel = self.buf().path.as_ref().and_then(|p| git::rel_path(repo, p));
        if rel.is_none() {
            self.say("Buffer is not visiting a file inside the repository");
        }
        rel
    }

    fn gerr(&mut self, what: &str, e: git2::Error) {
        self.say(format!("git {what}: {}", e.message()));
    }

    // -------------------------------------------------------------- status
    fn status_text(&self, repo: &git2::Repository) -> Result<(String, Vec<LineMeta>), git2::Error> {
        let ents = git::entries(repo)?;
        let info = git::info(repo);
        let root = git::workdir(repo).unwrap_or_else(|| self.cwd.clone());
        let mut t = String::new();
        let mut m: Vec<LineMeta> = Vec::new();
        let push = |t: &mut String, m: &mut Vec<LineMeta>, s: String, meta: LineMeta| {
            t.push_str(&s);
            t.push('\n');
            m.push(meta);
        };
        push(&mut t, &mut m, format!("Repository: {}", root.display()), LineMeta::None);
        let mut b = format!("Branch:     {}", info.branch);
        if info.ahead > 0 || info.behind > 0 {
            b.push_str(&format!("  (ahead {}, behind {})", info.ahead, info.behind));
        }
        push(&mut t, &mut m, b, LineMeta::None);
        push(&mut t, &mut m, String::new(), LineMeta::None);
        let staged: Vec<_> = ents.iter().filter(|e| e.index.is_some()).collect();
        let unstaged: Vec<_> = ents.iter().filter(|e| e.wt.is_some_and(|c| c != '?')).collect();
        let untracked: Vec<_> = ents.iter().filter(|e| e.wt == Some('?')).collect();
        if ents.is_empty() {
            push(&mut t, &mut m, "Nothing to commit, working tree clean".into(), LineMeta::None);
        }
        if !staged.is_empty() {
            push(&mut t, &mut m, format!("Staged changes ({}):", staged.len()), LineMeta::None);
            for e in staged {
                push(&mut t, &mut m, format!("  {}  {}", e.index.unwrap_or(' '), e.path), LineMeta::File { path: e.path.clone(), staged: true });
            }
            push(&mut t, &mut m, String::new(), LineMeta::None);
        }
        if !unstaged.is_empty() {
            push(&mut t, &mut m, format!("Unstaged changes ({}):", unstaged.len()), LineMeta::None);
            for e in unstaged {
                push(&mut t, &mut m, format!("  {}  {}", e.wt.unwrap_or(' '), e.path), LineMeta::File { path: e.path.clone(), staged: false });
            }
            push(&mut t, &mut m, String::new(), LineMeta::None);
        }
        if !untracked.is_empty() {
            push(&mut t, &mut m, format!("Untracked files ({}):", untracked.len()), LineMeta::None);
            for e in untracked {
                push(&mut t, &mut m, format!("  ?  {}", e.path), LineMeta::File { path: e.path.clone(), staged: false });
            }
            push(&mut t, &mut m, String::new(), LineMeta::None);
        }
        push(
            &mut t,
            &mut m,
            " s stage  u unstage  RET visit  d diff  D diff all  c commit  C amend  l log  b branch  x discard  P push  F pull  g refresh  q quit".into(),
            LineMeta::None,
        );
        Ok((t, m))
    }

    fn git_status(&mut self) {
        self.git_status_inner(false);
    }
    fn git_status_refresh(&mut self) {
        self.git_status_inner(true);
    }

    fn git_status_inner(&mut self, keep: bool) {
        let Some(repo) = self.need_repo() else { return };
        self.proj_root = git::workdir(&repo);
        match self.status_text(&repo) {
            Ok((t, m)) => {
                self.open_special("*git-status*", BufKind::GitStatus, &t, m, keep);
                if !keep {
                    // start on the first file line
                    let b = self.buf_mut();
                    if let Some(i) = b.meta.iter().position(|x| matches!(x, LineMeta::File { .. })) {
                        b.cursor = Pos::new(i, 0);
                    }
                }
                self.git = Some(git::info(&repo));
            }
            Err(e) => self.gerr("status", e),
        }
    }

    /// Rebuild the status buffer in the background (if one exists) after a git operation.
    fn refresh_status_buf(&mut self) {
        let Some(i) = self.bufs.iter().position(|b| b.kind == BufKind::GitStatus) else { return };
        let Some(repo) = self.repo_here() else { return };
        if let Ok((t, m)) = self.status_text(&repo) {
            let b = &mut self.bufs[i];
            let c = b.cursor;
            b.set_special(&t, m);
            b.cursor = b.clamp(c);
        }
        self.git = Some(git::info(&repo));
    }

    fn line_file(&self) -> Option<(String, bool)> {
        let b = self.buf();
        match b.meta.get(b.cursor.line) {
            Some(LineMeta::File { path, staged }) => Some((path.clone(), *staged)),
            _ => None,
        }
    }

    fn status_stage_line(&mut self, stage: bool) {
        let Some((path, staged)) = self.line_file() else {
            self.say("No file on this line");
            return;
        };
        if stage && staged {
            self.say("Already staged");
            return;
        }
        if !stage && !staged {
            self.say("Not staged");
            return;
        }
        let Some(repo) = self.need_repo() else { return };
        // remember a neighbour in the same section so the cursor can stay put after the file moves
        let neighbour = {
            let b = self.buf();
            let pick = |i: usize| match b.meta.get(i) {
                Some(LineMeta::File { path: p, staged: s }) if *s == staged => Some(p.clone()),
                _ => None,
            };
            let l = b.cursor.line;
            (l + 1..b.meta.len()).find_map(pick).or_else(|| (0..l).rev().find_map(pick))
        };
        let r = if stage { git::stage(&repo, &path) } else { git::unstage(&repo, &path) };
        match r {
            Ok(()) => {
                self.git_status_inner(true);
                if let Some(n) = neighbour {
                    let b = self.buf_mut();
                    if let Some(i) = b.meta.iter().position(|m| matches!(m, LineMeta::File { path: p, staged: s } if *p == n && *s == staged)) {
                        b.cursor = Pos::new(i, 0);
                    }
                }
                self.say(format!("{} {path}", if stage { "Staged" } else { "Unstaged" }));
                self.mark_all_marks_stale();
            }
            Err(e) => self.gerr("stage", e),
        }
    }

    fn mark_all_marks_stale(&mut self) {
        for b in &mut self.bufs {
            b.git_marks_version = u64::MAX;
        }
    }

    fn status_diff_line(&mut self) {
        let Some((path, staged)) = self.line_file() else {
            self.say("No file on this line");
            return;
        };
        let Some(repo) = self.need_repo() else { return };
        let kind = if staged { git::DiffKind::Staged } else { git::DiffKind::Unstaged };
        match git::diff(&repo, kind, Some(&path)) {
            Ok(t) if !t.is_empty() => self.open_special("*git-diff*", BufKind::GitDiff, &t, Vec::new(), false),
            Ok(_) => self.say("No diff (untracked file?)"),
            Err(e) => self.gerr("diff", e),
        }
    }

    fn status_restore_line(&mut self) {
        match self.line_file() {
            Some((path, false)) => {
                let is_new = self.buf().lines.get(self.buf().cursor.line).is_some_and(|l| l.trim_start().starts_with('?'));
                if is_new {
                    self.say("Untracked file: delete it from a shell");
                } else {
                    self.mode = Mode::YesNo(YesNo { q: format!("Discard unstaged changes to {path}? (y or n) "), act: YAct::Restore(path) });
                }
            }
            _ => self.say("Only unstaged changes can be discarded here"),
        }
    }

    // --------------------------------------------------------------- diff
    fn show_diff(&mut self, kind: git::DiffKind, rel: Option<String>, empty_msg: &str) {
        let Some(repo) = self.need_repo() else { return };
        self.proj_root = git::workdir(&repo);
        match git::diff(&repo, kind, rel.as_deref()) {
            Ok(t) if t.is_empty() => self.say(empty_msg.to_string()),
            Ok(t) => self.open_special("*git-diff*", BufKind::GitDiff, &t, Vec::new(), false),
            Err(e) => self.gerr("diff", e),
        }
    }
    fn git_diff_repo(&mut self) {
        self.show_diff(git::DiffKind::All, None, "No changes");
    }
    fn git_diff_staged(&mut self) {
        self.show_diff(git::DiffKind::Staged, None, "No staged changes");
    }
    fn git_diff_file(&mut self) {
        let Some(repo) = self.need_repo() else { return };
        let Some(rel) = self.cur_rel(&repo) else { return };
        self.show_diff(git::DiffKind::All, Some(rel), "No changes in this file");
    }

    // ---------------------------------------------------------------- log
    fn git_log(&mut self, file: bool) {
        let Some(repo) = self.need_repo() else { return };
        self.proj_root = git::workdir(&repo);
        let rel = if file {
            match self.cur_rel(&repo) {
                Some(r) => Some(r),
                None => return,
            }
        } else {
            None
        };
        match git::log(&repo, rel.as_deref(), 300) {
            Ok(v) if v.is_empty() => self.say("No commits"),
            Ok(v) => {
                let now = now_secs();
                let mut t = String::new();
                let mut m = Vec::new();
                for c in &v {
                    let (y, mo, d, _, _) = git::civil(c.secs);
                    t.push_str(&format!(
                        "{:.7}  {y:04}-{mo:02}-{d:02}  {:<9}  {:<14.14}  {}\n",
                        c.id,
                        git::ago(c.secs, now),
                        c.author,
                        c.summary
                    ));
                    m.push(LineMeta::Commit(c.id.clone()));
                }
                let name = match &rel {
                    Some(r) => format!("*git-log: {r}*"),
                    None => "*git-log*".to_string(),
                };
                self.open_special(&name, BufKind::GitLog, &t, m, false);
            }
            Err(e) => self.gerr("log", e),
        }
    }

    fn git_show_commit(&mut self, id: &str) {
        let Some(repo) = self.need_repo() else { return };
        match git::show(&repo, id) {
            Ok(t) => {
                let name = format!("*git-show {:.7}*", id);
                self.open_special(&name, BufKind::GitDiff, &t, Vec::new(), false);
            }
            Err(e) => self.gerr("show", e),
        }
    }

    // -------------------------------------------------------------- blame
    const BLAME_W: usize = 33;

    fn git_blame(&mut self) {
        let Some(repo) = self.need_repo() else { return };
        let Some(rel) = self.cur_rel(&repo) else { return };
        let modified = self.buf().modified;
        match git::blame(&repo, &rel) {
            Ok(labels) => {
                let b = self.buf();
                let line = b.cursor.line;
                let name = format!("*blame {}*", b.name);
                let mut t = String::new();
                for (i, l) in b.lines.iter().enumerate() {
                    let lab = labels.get(i).filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| format!("{:<w$}", "Not Committed Yet", w = Self::BLAME_W));
                    t.push_str(&format!("{lab} │ {l}\n"));
                }
                self.open_special(&name, BufKind::GitBlame, &t, Vec::new(), false);
                let bm = self.buf_mut();
                bm.cursor = Pos::new(line, Self::BLAME_W + 3);
                bm.top = line.saturating_sub(5);
                if modified {
                    self.say("Buffer has unsaved changes: blame reflects the committed file");
                }
            }
            Err(e) => self.gerr("blame", e),
        }
    }

    fn git_blame_line(&mut self) {
        let Some(repo) = self.need_repo() else { return };
        let Some(rel) = self.cur_rel(&repo) else { return };
        let line = self.buf().cursor.line;
        match git::blame(&repo, &rel) {
            Ok(labels) => match labels.get(line).filter(|s| !s.is_empty()) {
                Some(l) => {
                    let summary = repo
                        .revparse_single(&l[..7])
                        .ok()
                        .and_then(|o| o.peel_to_commit().ok())
                        .and_then(|c| c.summary().ok().flatten().map(|s| s.to_string()))
                        .unwrap_or_default();
                    self.say(format!("{}  {summary}", l.trim_end()));
                }
                None => self.say("Not committed yet"),
            },
            Err(e) => self.gerr("blame", e),
        }
    }

    // ---------------------------------------------------- stage / commit
    fn git_stage_file(&mut self, stage: bool) {
        let Some(repo) = self.need_repo() else { return };
        let Some(rel) = self.cur_rel(&repo) else { return };
        let warn = self.buf().modified;
        let r = if stage { git::stage(&repo, &rel) } else { git::unstage(&repo, &rel) };
        match r {
            Ok(()) => {
                self.refresh_git_info();
                self.refresh_status_buf();
                let verb = if stage { "Staged" } else { "Unstaged" };
                if warn && stage {
                    self.say(format!("{verb} {rel} (buffer has unsaved changes, only the saved file was staged)"));
                } else {
                    self.say(format!("{verb} {rel}"));
                }
            }
            Err(e) => self.gerr("stage", e),
        }
    }

    fn git_stage_all(&mut self) {
        let Some(repo) = self.need_repo() else { return };
        match git::stage_all(&repo) {
            Ok(()) => {
                self.refresh_git_info();
                self.refresh_status_buf();
                self.say("Staged all changes");
            }
            Err(e) => self.gerr("stage", e),
        }
    }

    fn git_commit_cmd(&mut self, amend: bool) {
        let Some(repo) = self.need_repo() else { return };
        let info = git::info(&repo);
        if !amend && info.staged == 0 {
            self.say("Nothing staged (C-x v a stages this file, C-x g opens the status buffer)");
            return;
        }
        let pre = if amend {
            repo.head()
                .ok()
                .and_then(|h| h.peel_to_commit().ok())
                .and_then(|c| c.summary().ok().flatten().map(|s| s.to_string()))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let label = if amend { "Amend message: " } else { "Commit message: " };
        self.start_prompt(Prompt::new(label, PKind::Commit { amend }, None).with_input(&pre));
    }

    fn git_commit_now(&mut self, msg: &str, amend: bool) {
        if msg.is_empty() {
            self.say("Empty commit message: aborted");
            return;
        }
        let Some(repo) = self.need_repo() else { return };
        match git::commit(&repo, msg, amend) {
            Ok(id) => {
                self.refresh_git_info();
                self.refresh_status_buf();
                self.mark_all_marks_stale();
                self.say(format!("{} {id}: {msg}", if amend { "Amended" } else { "Committed" }));
            }
            Err(e) => self.gerr("commit", e),
        }
    }

    // ---------------------------------------------------------- branches
    fn git_checkout_cmd(&mut self) {
        let Some(repo) = self.need_repo() else { return };
        let cur = git::branch_name(&repo);
        let mut bs = git::branches(&repo);
        bs.retain(|b| *b != cur);
        self.start_prompt(Prompt::new(&format!("Checkout (now on {cur}; -b NAME creates): "), PKind::Checkout, Some(bs)));
    }

    fn git_checkout_now(&mut self, spec: &str) {
        if spec.is_empty() {
            return;
        }
        let Some(repo) = self.need_repo() else { return };
        match git::checkout(&repo, spec) {
            Ok(m) => {
                self.revert_unmodified();
                self.mark_all_marks_stale();
                self.refresh_git_info();
                self.refresh_status_buf();
                self.say(m);
            }
            Err(e) => self.gerr("checkout", e),
        }
    }

    fn git_restore_cmd(&mut self) {
        let Some(repo) = self.need_repo() else { return };
        let Some(rel) = self.cur_rel(&repo) else { return };
        self.mode = Mode::YesNo(YesNo { q: format!("Discard ALL changes to {rel} and restore from HEAD? (y or n) "), act: YAct::Restore(rel) });
    }

    fn restore_file_now(&mut self, rel: &str) {
        let Some(repo) = self.need_repo() else { return };
        match git::restore_file(&repo, rel) {
            Ok(()) => {
                let abs = git::workdir(&repo).map(|w| w.join(rel));
                for i in 0..self.bufs.len() {
                    let same = match (&self.bufs[i].path, &abs) {
                        (Some(p), Some(a)) => std::fs::canonicalize(p).ok() == std::fs::canonicalize(a).ok(),
                        _ => false,
                    };
                    if same {
                        self.reload_buffer(i);
                    }
                }
                self.refresh_git_info();
                self.refresh_status_buf();
                self.say(format!("Restored {rel}"));
            }
            Err(e) => self.gerr("restore", e),
        }
    }

    fn git_hunk(&mut self, next: bool) {
        if self.buf().git_marks_version != self.buf().version {
            self.refresh_marks();
        }
        let b = self.buf_mut();
        let m = &b.git_marks;
        let starts: Vec<usize> = (0..m.len()).filter(|&i| m[i] != 0 && (i == 0 || m[i - 1] == 0)).collect();
        let cur = b.cursor.line;
        let target = if next { starts.iter().copied().find(|&s| s > cur) } else { starts.iter().rev().copied().find(|&s| s < cur) };
        match target {
            Some(l) => {
                b.cursor = Pos::new(l, 0);
                let n = starts.iter().position(|&s| s == l).map(|i| i + 1).unwrap_or(0);
                let total = starts.len();
                self.say(format!("Hunk {n}/{total}"));
            }
            None => self.say(if starts.is_empty() { "No changes against HEAD" } else { "No more hunks" }),
        }
    }

    // ------------------------------------------------------ remote ops
    fn git_pull(&mut self) {
        let Some(repo) = self.need_repo() else { return };
        match git::pull(&repo) {
            Ok(m) => {
                let n = self.revert_unmodified();
                self.mark_all_marks_stale();
                self.refresh_git_info();
                self.refresh_status_buf();
                self.say(if n > 0 { format!("{m} ({n} buffer(s) reloaded)") } else { m });
            }
            Err(e) => self.gerr("pull", e),
        }
    }

    fn git_push(&mut self) {
        let Some(repo) = self.need_repo() else { return };
        match git::push(&repo) {
            Ok(m) => {
                self.refresh_git_info();
                self.refresh_status_buf();
                self.say(m);
            }
            Err(e) => self.gerr("push", e),
        }
    }

    fn git_init(&mut self) {
        match git2::Repository::init(&self.cwd) {
            Ok(r) => {
                self.proj_root = git::workdir(&r);
                self.refresh_git_info();
                self.say(format!("Initialized git repository in {}", self.cwd.display()));
            }
            Err(e) => self.gerr("init", e),
        }
    }

    // ----------------------------------------------------------- project
    fn project_root_and_files(&mut self) -> (PathBuf, Vec<String>) {
        match self.repo_here() {
            Some(r) => {
                let root = git::workdir(&r).unwrap_or_else(|| self.cwd.clone());
                (root, git::project_files(&r))
            }
            None => (self.cwd.clone(), git::walk_files(&self.cwd, 50_000)),
        }
    }

    fn project_find_file(&mut self) {
        let (root, files) = self.project_root_and_files();
        if files.is_empty() {
            self.say("No files found in project");
            return;
        }
        self.proj_root = Some(root.clone());
        let label = format!("Project file ({} files): ", files.len());
        self.start_prompt(Prompt::new(&label, PKind::ProjFile, Some(files)));
    }

    fn project_open(&mut self, rel: &str) {
        let root = self.proj_root.clone().unwrap_or_else(|| self.cwd.clone());
        let p = root.join(rel);
        self.find_file(&p.to_string_lossy());
    }

    fn project_grep_cmd(&mut self) {
        // pre-fill with the selected region (single line) so a fresh query can simply be typed
        let b = self.buf();
        let pre = match b.mark {
            Some(m) if b.mark_active && m.line == b.cursor.line => b.text_in(m, b.cursor),
            _ => String::new(),
        };
        self.start_prompt(Prompt::new("Project grep (regex): ", PKind::ProjGrep, None).with_input(&pre));
    }

    fn project_grep_now(&mut self, pat: &str) {
        if pat.is_empty() {
            return;
        }
        let (root, files) = self.project_root_and_files();
        self.proj_root = Some(root.clone());
        let ci = !pat.chars().any(|c| c.is_uppercase());
        let re = regex::RegexBuilder::new(pat).case_insensitive(ci).build().or_else(|_| regex::RegexBuilder::new(&regex::escape(pat)).case_insensitive(ci).build());
        let Ok(re) = re else {
            self.say("Invalid pattern");
            return;
        };
        let mut t = format!("Grep for /{pat}/ in {}\n\n", root.display());
        let mut m = vec![LineMeta::None, LineMeta::None];
        let mut count = 0;
        let mut nfiles = 0;
        'outer: for f in &files {
            let Ok(meta) = std::fs::metadata(root.join(f)) else { continue };
            if meta.len() > 2_000_000 {
                continue;
            }
            let Ok(bytes) = std::fs::read(root.join(f)) else { continue };
            if bytes.iter().take(8000).any(|&b| b == 0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            let mut hit = false;
            for (i, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    hit = true;
                    let shown: String = line.trim().chars().take(160).collect();
                    t.push_str(&format!("{f}:{}: {shown}\n", i + 1));
                    m.push(LineMeta::Grep { path: f.clone(), line: i + 1 });
                    count += 1;
                    if count >= 3000 {
                        t.push_str("... (truncated at 3000 matches)\n");
                        m.push(LineMeta::None);
                        break 'outer;
                    }
                }
            }
            if hit {
                nfiles += 1;
            }
        }
        if count == 0 {
            self.say(format!("No matches for /{pat}/"));
            return;
        }
        self.open_special("*grep*", BufKind::Grep, &t, m, false);
        self.grep_fresh = true;
        let b = self.buf_mut();
        b.cursor = Pos::new(2, 0);
        self.say(format!("{count} matches in {nfiles} files (RET visits, M-g n next)"));
    }

    fn next_error(&mut self, down: bool) {
        let Some(gi) = self.bufs.iter().position(|b| b.kind == BufKind::Grep) else {
            self.say("No grep results (C-x p g)");
            return;
        };
        let g = &self.bufs[gi];
        let cur = g.cursor.line;
        let is_hit = |i: usize| matches!(g.meta.get(i), Some(LineMeta::Grep { .. }));
        let n = g.meta.len();
        let found = if down {
            let start = if self.grep_fresh { cur } else { cur + 1 };
            (start..n).find(|&i| is_hit(i))
        } else {
            (0..cur).rev().find(|&i| is_hit(i))
        };
        let Some(i) = found else {
            self.say("No more matches");
            return;
        };
        self.grep_fresh = false;
        let Some(LineMeta::Grep { path, line }) = self.bufs[gi].meta.get(i).cloned() else { return };
        self.bufs[gi].cursor = Pos::new(i, 0);
        let total = self.bufs[gi].meta.iter().filter(|m| matches!(m, LineMeta::Grep { .. })).count();
        let k = self.bufs[gi].meta[..=i].iter().filter(|m| matches!(m, LineMeta::Grep { .. })).count();
        let root = self.proj_root.clone().unwrap_or_else(|| self.cwd.clone());
        self.visit_file_line(&root.join(&path), line);
        self.say(format!("{path}:{line} ({k}/{total})"));
    }
}
