//! Built-in git support on top of libgit2 (no `git` binary required).
use git2::{
    build::CheckoutBuilder, Cred, DiffFormat, DiffOptions, ErrorCode, FetchOptions, ObjectType,
    Patch, PushOptions, RemoteCallbacks, Repository, Sort, StatusOptions,
};
use std::path::{Path, PathBuf};

pub type Res<T> = Result<T, git2::Error>;

pub fn open(path: &Path) -> Option<Repository> {
    Repository::discover(path).ok()
}

pub fn workdir(repo: &Repository) -> Option<PathBuf> {
    repo.workdir().map(|p| p.to_path_buf())
}

/// Path of `abs` relative to the repository work tree (symlink-tolerant).
pub fn rel_path(repo: &Repository, abs: &Path) -> Option<String> {
    let wd = std::fs::canonicalize(repo.workdir()?).ok()?;
    let file = abs.file_name()?;
    let parent = std::fs::canonicalize(abs.parent()?).ok()?;
    let full = parent.join(file);
    let rel = full.strip_prefix(&wd).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

#[derive(Default, Clone, Debug)]
pub struct Info {
    pub branch: String,
    pub staged: usize,
    pub modified: usize,
    pub untracked: usize,
    pub ahead: usize,
    pub behind: usize,
}

pub fn branch_name(repo: &Repository) -> String {
    match repo.head() {
        Ok(h) if h.is_branch() => h.shorthand().unwrap_or("?").to_string(),
        Ok(h) => format!("({})", h.target().map(|o| o.to_string()[..7].to_string()).unwrap_or_default()),
        Err(_) => repo
            .find_reference("HEAD")
            .ok()
            .and_then(|r| r.symbolic_target().ok().flatten().map(|s| s.trim_start_matches("refs/heads/").to_string()))
            .unwrap_or_else(|| "?".into()),
    }
}

pub fn ahead_behind(repo: &Repository) -> (usize, usize) {
    let go = || -> Option<(usize, usize)> {
        let head = repo.head().ok()?;
        let local = head.target()?;
        let up = git2::Branch::wrap(head).upstream().ok()?.get().target()?;
        repo.graph_ahead_behind(local, up).ok()
    };
    go().unwrap_or((0, 0))
}

pub fn info(repo: &Repository) -> Info {
    let mut i = Info { branch: branch_name(repo), ..Default::default() };
    let mut o = StatusOptions::new();
    o.include_untracked(true).include_ignored(false).recurse_untracked_dirs(false);
    if let Ok(st) = repo.statuses(Some(&mut o)) {
        for e in st.iter() {
            let s = e.status();
            if s.intersects(
                git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_DELETED
                    | git2::Status::INDEX_RENAMED
                    | git2::Status::INDEX_TYPECHANGE,
            ) {
                i.staged += 1;
            }
            if s.intersects(
                git2::Status::WT_MODIFIED | git2::Status::WT_DELETED | git2::Status::WT_RENAMED | git2::Status::WT_TYPECHANGE,
            ) {
                i.modified += 1;
            }
            if s.contains(git2::Status::WT_NEW) {
                i.untracked += 1;
            }
        }
    }
    let (a, b) = ahead_behind(repo);
    i.ahead = a;
    i.behind = b;
    i
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub index: Option<char>,
    pub wt: Option<char>,
}

pub fn entries(repo: &Repository) -> Res<Vec<Entry>> {
    let mut o = StatusOptions::new();
    o.include_untracked(true).include_ignored(false).recurse_untracked_dirs(true).renames_head_to_index(true);
    let st = repo.statuses(Some(&mut o))?;
    let mut v = Vec::new();
    for e in st.iter() {
        let s = e.status();
        let Ok(p) = e.path() else { continue };
        let index = if s.contains(git2::Status::INDEX_NEW) {
            Some('A')
        } else if s.contains(git2::Status::INDEX_MODIFIED) {
            Some('M')
        } else if s.contains(git2::Status::INDEX_DELETED) {
            Some('D')
        } else if s.contains(git2::Status::INDEX_RENAMED) {
            Some('R')
        } else if s.contains(git2::Status::INDEX_TYPECHANGE) {
            Some('T')
        } else {
            None
        };
        let wt = if s.contains(git2::Status::CONFLICTED) {
            Some('U')
        } else if s.contains(git2::Status::WT_NEW) {
            Some('?')
        } else if s.contains(git2::Status::WT_MODIFIED) {
            Some('M')
        } else if s.contains(git2::Status::WT_DELETED) {
            Some('D')
        } else if s.contains(git2::Status::WT_RENAMED) {
            Some('R')
        } else if s.contains(git2::Status::WT_TYPECHANGE) {
            Some('T')
        } else {
            None
        };
        if index.is_some() || wt.is_some() {
            v.push(Entry { path: p.to_string(), index, wt });
        }
    }
    Ok(v)
}

pub fn stage(repo: &Repository, rel: &str) -> Res<()> {
    let mut idx = repo.index()?;
    let full = repo.workdir().ok_or_else(|| git2::Error::from_str("bare repository"))?.join(rel);
    if full.exists() {
        idx.add_path(Path::new(rel))?;
    } else {
        idx.remove_path(Path::new(rel))?;
    }
    idx.write()
}

pub fn unstage(repo: &Repository, rel: &str) -> Res<()> {
    match repo.head() {
        Ok(h) => {
            let obj = h.peel(ObjectType::Commit)?;
            repo.reset_default(Some(&obj), [rel])
        }
        Err(_) => {
            let mut idx = repo.index()?;
            idx.remove_path(Path::new(rel))?;
            idx.write()
        }
    }
}

pub fn stage_all(repo: &Repository) -> Res<()> {
    let mut idx = repo.index()?;
    idx.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    idx.update_all(["*"].iter(), None)?;
    idx.write()
}

pub fn commit(repo: &Repository, msg: &str, amend: bool) -> Res<String> {
    let sig = repo
        .signature()
        .map_err(|_| git2::Error::from_str("user.name / user.email not configured (git config --global user.name ...)"))?;
    let mut idx = repo.index()?;
    let tree_id = idx.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let head = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    if amend {
        let h = head.ok_or_else(|| git2::Error::from_str("nothing to amend"))?;
        let oid = h.amend(Some("HEAD"), None, Some(&sig), None, Some(msg), Some(&tree))?;
        return Ok(oid.to_string()[..7].to_string());
    }
    if let Some(h) = &head {
        if h.tree_id() == tree_id {
            return Err(git2::Error::from_str("nothing to commit (no staged changes)"));
        }
    }
    let parents: Vec<&git2::Commit> = head.iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)?;
    Ok(oid.to_string()[..7].to_string())
}

fn diff_to_string(diff: &git2::Diff) -> String {
    let mut out = String::new();
    let _ = diff.print(DiffFormat::Patch, |_d, _h, l| {
        let o = l.origin();
        if matches!(o, '+' | '-' | ' ') {
            out.push(o);
        }
        out.push_str(&String::from_utf8_lossy(l.content()));
        true
    });
    out
}

#[derive(Clone, Copy, PartialEq)]
pub enum DiffKind {
    /// working tree vs index
    Unstaged,
    /// index vs HEAD
    Staged,
    /// working tree vs HEAD
    All,
}

pub fn diff(repo: &Repository, kind: DiffKind, rel: Option<&str>) -> Res<String> {
    let mut o = DiffOptions::new();
    if let Some(p) = rel {
        o.pathspec(p);
    }
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let d = match kind {
        DiffKind::Unstaged => repo.diff_index_to_workdir(None, Some(&mut o))?,
        DiffKind::Staged => repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut o))?,
        DiffKind::All => repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut o))?,
    };
    Ok(diff_to_string(&d))
}

// ------------------------------------------------------------- time utils
pub fn civil(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32)
}

pub fn fmt_date(t: git2::Time) -> String {
    let (y, m, d, _, _) = civil(t.seconds() + t.offset_minutes() as i64 * 60);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn fmt_datetime(t: git2::Time) -> String {
    let (y, m, d, hh, mm) = civil(t.seconds() + t.offset_minutes() as i64 * 60);
    let off = t.offset_minutes();
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} {}{:02}{:02}", if off < 0 { '-' } else { '+' }, off.abs() / 60, off.abs() % 60)
}

pub fn ago(secs: i64, now: i64) -> String {
    let d = (now - secs).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        86400..=2591999 => format!("{}d ago", d / 86400),
        2592000..=31535999 => format!("{}mo ago", d / 2592000),
        _ => format!("{}y ago", d / 31536000),
    }
}

// -------------------------------------------------------------------- log
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub secs: i64,
}

pub fn log(repo: &Repository, rel: Option<&str>, n: usize) -> Res<Vec<LogEntry>> {
    let mut w = repo.revwalk()?;
    w.push_head()?;
    w.set_sorting(Sort::TIME)?;
    let mut out = Vec::new();
    for (scanned, oid) in w.enumerate() {
        if out.len() >= n || scanned > 20000 {
            break;
        }
        let c = repo.find_commit(oid?)?;
        if let Some(p) = rel {
            let mut o = DiffOptions::new();
            o.pathspec(p);
            let tree = c.tree()?;
            let parent = c.parent(0).ok().and_then(|p| p.tree().ok());
            let d = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut o))?;
            if d.deltas().len() == 0 {
                continue;
            }
        }
        out.push(LogEntry {
            id: c.id().to_string(),
            summary: c.summary().ok().flatten().unwrap_or("").to_string(),
            author: c.author().name().unwrap_or("?").to_string(),
            secs: c.time().seconds(),
        });
    }
    Ok(out)
}

pub fn show(repo: &Repository, id: &str) -> Res<String> {
    let obj = repo.revparse_single(id)?;
    let c = obj.peel_to_commit()?;
    let mut s = String::new();
    s.push_str(&format!("commit {}\n", c.id()));
    s.push_str(&format!("Author: {} <{}>\n", c.author().name().unwrap_or("?"), c.author().email().unwrap_or("?")));
    s.push_str(&format!("Date:   {}\n\n", fmt_datetime(c.time())));
    for l in c.message().unwrap_or("").lines() {
        s.push_str("    ");
        s.push_str(l);
        s.push('\n');
    }
    s.push('\n');
    let tree = c.tree()?;
    let parent = c.parent(0).ok().and_then(|p| p.tree().ok());
    let d = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), None)?;
    let stats = d.stats()?;
    s.push_str(&format!(" {} files changed, {} insertions(+), {} deletions(-)\n\n", stats.files_changed(), stats.insertions(), stats.deletions()));
    s.push_str(&diff_to_string(&d));
    Ok(s)
}

pub fn blame(repo: &Repository, rel: &str) -> Res<Vec<String>> {
    let b = repo.blame_file(Path::new(rel), None)?;
    let mut lines: Vec<String> = Vec::new();
    for h in b.iter() {
        let start = h.final_start_line().saturating_sub(1);
        let id = h.final_commit_id();
        let author = h.final_signature().and_then(|s| s.name().ok().map(String::from)).unwrap_or_else(|| "?".into());
        let date = repo.find_commit(id).map(|c| fmt_date(c.time())).unwrap_or_default();
        let label = format!("{:.7} {:<14.14} {}", id, author, date);
        for k in 0..h.lines_in_hunk() {
            if lines.len() <= start + k {
                lines.resize(start + k + 1, String::new());
            }
            lines[start + k] = label.clone();
        }
    }
    Ok(lines)
}

/// Gutter marks (see buffer::MARK_*) for `text` compared against HEAD.
pub fn line_marks(repo: &Repository, rel: &str, text: &[u8], n_lines: usize) -> Option<Vec<u8>> {
    if text.len() > 2_000_000 {
        return None;
    }
    let mut marks = vec![0u8; n_lines];
    let blob = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_tree().ok())
        .and_then(|t| t.get_path(Path::new(rel)).ok())
        .and_then(|e| repo.find_blob(e.id()).ok());
    let Some(blob) = blob else {
        if repo.status_should_ignore(Path::new(rel)).unwrap_or(true) {
            return None;
        }
        marks.iter_mut().for_each(|m| *m = 1);
        return Some(marks);
    };
    let mut o = DiffOptions::new();
    o.context_lines(0);
    let patch = Patch::from_blob_and_buffer(&blob, Some(Path::new(rel)), text, Some(Path::new(rel)), Some(&mut o)).ok()?;
    for i in 0..patch.num_hunks() {
        let Ok((h, _)) = patch.hunk(i) else { continue };
        let (ns, nl, ol) = (h.new_start() as usize, h.new_lines() as usize, h.old_lines());
        if nl == 0 {
            let idx = ns.saturating_sub(1).min(n_lines.saturating_sub(1));
            if marks[idx] == 0 {
                marks[idx] = 3;
            }
        } else {
            let kind = if ol == 0 { 1 } else { 2 };
            for l in ns.saturating_sub(1)..(ns - 1 + nl).min(n_lines) {
                marks[l] = kind;
            }
        }
    }
    Some(marks)
}

// ------------------------------------------------------- branches/remotes
pub fn branches(repo: &Repository) -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(bs) = repo.branches(None) {
        for b in bs.flatten() {
            if let Ok(Some(n)) = b.0.name() {
                if !n.ends_with("/HEAD") {
                    v.push(n.to_string());
                }
            }
        }
    }
    v
}

pub fn checkout(repo: &Repository, spec: &str) -> Res<String> {
    let mut name = spec.trim().to_string();
    let mut created = false;
    if let Some(new) = name.strip_prefix("-b ") {
        let c = repo.head()?.peel_to_commit()?;
        repo.branch(new.trim(), &c, false)?;
        name = new.trim().to_string();
        created = true;
    }
    let (obj, reference) = match repo.revparse_ext(&name) {
        Ok(x) => x,
        Err(e) if e.code() == ErrorCode::NotFound => {
            // "origin/foo" style remote branch -> create local tracking branch
            return Err(git2::Error::from_str(&format!("unknown branch or revision '{name}'")));
        }
        Err(e) => return Err(e),
    };
    repo.checkout_tree(&obj, Some(CheckoutBuilder::new().safe()))?;
    match reference {
        Some(r) if r.is_branch() => repo.set_head(r.name()?)?,
        Some(r) if r.is_remote() => {
            let short = r.shorthand().unwrap_or("").split_once('/').map(|x| x.1.to_string()).unwrap_or_default();
            let c = obj.peel_to_commit()?;
            let b = repo.branch(&short, &c, false)?;
            repo.set_head(b.get().name().unwrap_or(""))?;
            return Ok(format!("Switched to new branch '{short}' (from {})", r.shorthand().unwrap_or("")));
        }
        _ => repo.set_head_detached(obj.id())?,
    }
    Ok(if created { format!("Created and switched to '{name}'") } else { format!("Switched to '{name}'") })
}

pub fn restore_file(repo: &Repository, rel: &str) -> Res<()> {
    let mut cb = CheckoutBuilder::new();
    cb.force().path(rel);
    repo.checkout_head(Some(&mut cb))
}

fn callbacks<'a>() -> RemoteCallbacks<'a> {
    let mut cb = RemoteCallbacks::new();
    let mut tries = 0;
    cb.credentials(move |url, user, allowed| {
        tries += 1;
        if tries > 4 {
            return Err(git2::Error::from_str("authentication failed (ssh-agent / credential helper)"));
        }
        if allowed.contains(git2::CredentialType::SSH_KEY) {
            return Cred::ssh_key_from_agent(user.unwrap_or("git"));
        }
        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            if let Ok(cfg) = git2::Config::open_default() {
                return Cred::credential_helper(&cfg, url, user);
            }
        }
        Cred::default()
    });
    cb
}

fn remote_for_head(repo: &Repository) -> Res<(String, String)> {
    let head = repo.head()?;
    let refname = head.name()?.to_string();
    let branch = head.shorthand().unwrap_or("").to_string();
    let remote = repo
        .branch_upstream_remote(&refname)
        .ok()
        .and_then(|b| b.as_str().ok().map(|s| s.to_string()))
        .unwrap_or_else(|| "origin".to_string());
    Ok((remote, branch))
}

pub fn push(repo: &Repository) -> Res<String> {
    let (remote_name, branch) = remote_for_head(repo)?;
    let mut remote = repo.find_remote(&remote_name)?;
    let mut po = PushOptions::new();
    po.remote_callbacks(callbacks());
    let spec = format!("refs/heads/{branch}:refs/heads/{branch}");
    remote.push(&[spec], Some(&mut po))?;
    Ok(format!("Pushed {branch} to {remote_name}"))
}

pub fn pull(repo: &Repository) -> Res<String> {
    let (remote_name, branch) = remote_for_head(repo)?;
    let mut remote = repo.find_remote(&remote_name)?;
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(callbacks());
    remote.fetch(&[&branch], Some(&mut fo), None)?;
    let fh = repo.find_reference("FETCH_HEAD")?;
    let fc = repo.reference_to_annotated_commit(&fh)?;
    let (an, _) = repo.merge_analysis(&[&fc])?;
    if an.is_up_to_date() {
        return Ok("Already up to date".into());
    }
    if an.is_fast_forward() {
        let refname = format!("refs/heads/{branch}");
        let obj = repo.find_object(fc.id(), None)?;
        repo.checkout_tree(&obj, Some(CheckoutBuilder::new().safe()))?;
        repo.find_reference(&refname)?.set_target(fc.id(), "mg: fast-forward")?;
        repo.set_head(&refname)?;
        return Ok(format!("Fast-forwarded {branch} to {:.7}", fc.id()));
    }
    Err(git2::Error::from_str("not a fast-forward: merge or rebase needed (use a shell for that)"))
}

// ---------------------------------------------------------------- project
pub fn project_files(repo: &Repository) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if let Ok(idx) = repo.index() {
        for e in idx.iter() {
            if let Ok(s) = String::from_utf8(e.path) {
                v.push(s);
            }
        }
    }
    let mut o = StatusOptions::new();
    o.include_untracked(true).include_ignored(false).recurse_untracked_dirs(true);
    if let Ok(st) = repo.statuses(Some(&mut o)) {
        for e in st.iter() {
            if e.status().contains(git2::Status::WT_NEW) {
                if let Ok(p) = e.path() {
                    v.push(p.to_string());
                }
            }
        }
    }
    let wd = repo.workdir().map(|p| p.to_path_buf());
    v.retain(|p| wd.as_ref().is_none_or(|w| w.join(p).is_file()));
    v.sort();
    v.dedup();
    v
}

pub fn walk_files(root: &Path, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(r) = p.strip_prefix(root) {
                out.push(r.to_string_lossy().to_string());
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mgrs-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn full_workflow() {
        let dir = tmp("wf");
        let repo = Repository::init(&dir).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "T").unwrap();
        cfg.set_str("user.email", "t@e.x").unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();

        let e = entries(&repo).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].wt, Some('?'));
        stage(&repo, "a.txt").unwrap();
        assert_eq!(entries(&repo).unwrap()[0].index, Some('A'));
        unstage(&repo, "a.txt").unwrap();
        assert_eq!(entries(&repo).unwrap()[0].wt, Some('?'));
        stage(&repo, "a.txt").unwrap();
        commit(&repo, "first", false).unwrap();
        assert!(commit(&repo, "again", false).is_err());

        // marks
        let marks = line_marks(&repo, "a.txt", b"one\nTWO\nthree\nfour\n", 4).unwrap();
        assert_eq!(marks, vec![0, 2, 0, 1]);
        let marks = line_marks(&repo, "a.txt", b"one\nthree\n", 2).unwrap();
        assert_eq!(marks, vec![3, 0]);

        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let d = diff(&repo, DiffKind::Unstaged, None).unwrap();
        assert!(d.contains("+four"), "{d}");
        stage(&repo, "a.txt").unwrap();
        assert!(diff(&repo, DiffKind::Staged, None).unwrap().contains("+four"));
        commit(&repo, "second\n\nbody", false).unwrap();

        let l = log(&repo, None, 10).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].summary, "second");
        assert!(show(&repo, &l[0].id).unwrap().contains("+four"));
        assert_eq!(log(&repo, Some("nope.txt"), 10).unwrap().len(), 0);
        let b = blame(&repo, "a.txt").unwrap();
        assert_eq!(b.len(), 4);

        // branches
        let name = branch_name(&repo);
        checkout(&repo, "-b feature").unwrap();
        assert_eq!(branch_name(&repo), "feature");
        checkout(&repo, &name).unwrap();
        assert_eq!(branch_name(&repo), name);
        assert!(checkout(&repo, "nope").is_err());
        assert!(branches(&repo).contains(&"feature".to_string()));

        // restore
        std::fs::write(dir.join("a.txt"), "junk\n").unwrap();
        restore_file(&repo, "a.txt").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\ntwo\nthree\nfour\n");

        assert_eq!(project_files(&repo), vec!["a.txt"]);
        assert_eq!(rel_path(&repo, &dir.join("a.txt")).unwrap(), "a.txt");
        let i = info(&repo);
        assert_eq!((i.staged, i.modified, i.untracked), (0, 0, 0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dates() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0));
        assert_eq!(civil(1_700_000_000), (2023, 11, 14, 22, 13));
    }
}
