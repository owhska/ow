# ow

A [Micro GNU Emacs](https://github.com/troglobit/mg)-style editor rewritten in Rust, with
**built-in git** (libgit2, no `git` binary needed) and **wide syntax highlighting**.
Binary name: `ow`.

    cargo build --release      # target/release/ow
    ow [+LINE] [FILE...]       # ow --help, ow --list-languages

## Emacs core
Movement (`C-f C-b C-n C-p C-a C-e M-f M-b M-< M-> C-v M-v C-l M-g g M-m M-{ M-}`), kill ring
(`C-k C-w M-w C-y M-y M-d M-DEL`), mark/region (`C-SPC C-x C-x C-x h`), undo/redo (`C-/ C-_ C-x u`, `M-_`),
incremental search (`C-s C-r`, smart-case), query-replace (`M-%`, y/n/!/./q), numeric args (`C-u`, `M-3`),
files and buffers (`C-x C-f C-x C-s C-x C-w C-x s C-x b C-x k C-x C-b C-x d`), `M-x` with fuzzy completion,
windows (`C-x 2` split below, `C-x 3` split right, `C-x 1` delete others, `C-x 0` close current, `C-x o` other window),
auto-indent, `M-;` comment toggle (any language), `TAB`/`S-TAB` indent/unindent, tab-vs-space detection,
CRLF preserved, bracketed paste, OSC 52 clipboard on kill/copy, auto-revert of unmodified buffers.
`C-h b` lists every binding.

## Vim Modal Editing & Space Leader
* **Modes**: `NORMAL`, `INSERT`, `VISUAL`, `VISUAL LINE` (displayed in status line).
* **Normal Mode Motions**: `h`, `j`, `k`, `l`, `w`, `b`, `e`, `0`, `^`, `$`, `H` (`^`), `L` (`g_`), `gg`, `G`, `C-u`, `C-d`.
* **Insert Mode**: `i`, `I`, `a`, `A`, `o`, `O`. Auto-pairing delimiters: `()`, `{}`, `[]`, `""`, `''`, `<>`. `ESC` / `C-[` returns to Normal.
* **Operators & Edits**: `dd`, `dw`, `d$`, `cc`, `cw`, `c$`, `yy`, `yw`, `y$`, `p`, `P`, `x`, `r<char>`, `~`, `u`, `C-r`.
* **Visual Mode**: `v`, `V`, `C-a` (select all). Move selected line(s) up with `K` and down with `J`. `d` to delete, `y` to yank, `c` to change.
* **Leader Key (`SPC`)**:
  | Key | Action |
  |---|---|
  | `SPC f` | Project file search (`project-find-file`) |
  | `SPC SPC` | Project grep (`project-grep`) |
  | `SPC e` | Directory explorer (`:Explore` / `dired`) |
  | `SPC TAB` / `SPC S-TAB` | Next / previous buffer (`:tabnext` / `:tabprevious`) |
  | `SPC t` | Open native interactive shell / terminal (`:term`) |
  | `SPC w w` | Save buffer (`:w`) |
  | `SPC w q` | Save and quit (`:wq`) |
  | `SPC q` | Close buffer (`:tabclose`) |
  | `SPC s` | Search buffer (`vimgrep`) |
  | `SPC w v` | Vertical split (`:vsplit`) |
  | `SPC w s` | Horizontal split (`:split`) |
  | `SPC w h/l/j/k` | Switch window focus |
  | `SPC r r` | Query replace word under cursor |
  | `SPC <arrow>` | Resize window split |
  | `C-p` | Find file in project |
  | `C-e` | Switch buffer |
  | `S-Right` / `S-Left` | Next / previous buffer |


## Git (all via libgit2)
| Key | Command |
|---|---|
| `C-x g` / `C-x v d` | status buffer: `s` stage, `u` unstage, `RET` visit, `d` diff, `D` full diff, `c` commit, `C` amend, `l` log, `b` checkout, `x` discard, `P` push, `F` pull, `g` refresh |
| `C-x v =` / `C-x v D` | diff of this file / whole repo (`n`/`p` jump between hunks) |
| `C-x v l` / `C-x v L` | log of repo / current file (`RET` shows the commit) |
| `C-x v g` / `C-x v b` | blame buffer / blame summary of current line |
| `C-x v a` / `C-x v u` | stage / unstage current file |
| `C-x v c` / `C-x v A` | commit / amend |
| `C-x v o` | checkout branch (fuzzy list; `-b name` creates) |
| `C-x v r` | restore file from HEAD (asks) |
| `C-x v n` / `C-x v p` | next / previous changed hunk |
| `C-x v F` / `C-x v P` | pull (fast-forward only) / push |

Always on: branch + ahead/behind + staged/modified/untracked counts in the mode line, and a gutter
(`▎` added/modified, `▁` deleted) comparing the buffer with HEAD, live while you type.

## Project
`C-x p f` fuzzy find-file over tracked+untracked files (falls back to a directory walk outside git),
`C-x p g` regex grep over the project (results buffer, `RET` / `M-g n` / `M-g p` to visit).

## Syntax highlighting
~70 languages: syntect's bundled set (Rust, C/C++, Go, Python, JS, Java, C#, PHP, Ruby, Lua, Haskell, SQL,
Shell, Markdown, YAML, JSON, HTML, CSS, LaTeX, Diff, ...) plus a built-in tokenizer for languages syntect
lacks: TypeScript, TSX, Kotlin, Swift, Zig, Dart, Elixir, Julia, TOML, INI/conf/.env, Dockerfile, CMake,
HCL/Terraform, Nix, Protobuf, GraphQL, PowerShell, Vim script. Highlighting is incremental and restarts
near the viewport in huge files. `M-x set-language` overrides detection.
Default colours are the custom palette in `syntax::palette` (edit it to retheme everything);
`OW_THEME=dark|light|<syntect theme name>` selects a built-in theme instead.
Adding a language = one entry in `src/langs.rs`.

## Not implemented (vs. upstream mg)
Keyboard macros, rectangles, `dired` beyond browsing/opening, `.mg` startup files/extensions,
registers, tags/cscope. Push/pull use ssh-agent or the git credential helper and were not tested against a
real remote.

## Layout
`buffer.rs` text/undo/search · `editor.rs` modes, prompts, dispatch · `commands.rs` Emacs commands + keymap ·
`gitcmds.rs` git/project commands · `git.rs` libgit2 wrapper · `syntax.rs` + `langs.rs` highlighting · `ui.rs` ratatui rendering.
Tests: `cargo test` (unit + end-to-end through simulated key events, including a full git workflow).
