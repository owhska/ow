# ow

A [Micro GNU Emacs](https://github.com/troglobit/mg)-style editor rewritten in Rust, with **Vim modal editing**, **built-in Git** (libgit2, no `git` binary needed) and **wide syntax highlighting**.

Binary name: `ow`.

```bash
cargo build --release      # target/release/ow
ow [+LINE] [FILE...]       # ow --help, ow --list-languages
```

## Editing model

OW combines Vim's modal editing with the non-navigation commands and workflow of Emacs.

- **Vim** owns cursor navigation, motions, operators and modal editing.
- **Emacs-style commands** provide editing operations, kill ring, search, buffers, windows, files and command execution.
- **Space Leader** provides a consistent namespace for project, window, buffer and Git operations.

## Emacs core

Emacs-style functionality for operations outside Vim's navigation and modal editing:

- Kill ring: `C-k`, `C-w`, `M-w`, `C-y`, `M-y`, `M-d`, `M-DEL`.
- Mark and region: `C-SPC`, `C-x C-x`, `C-x h`.
- Undo/redo: `C-/`, `C-_`, `C-x u`, `M-_`.
- Incremental search: `C-s`, `C-r` (smart-case).
- Query-replace: `M-%` (`y` / `n` / `!` / `.` / `q`).
- Numeric arguments: `C-u`, `M-3`.
- Files and buffers: `C-x C-f`, `C-x C-s`, `C-x C-w`, `C-x s`, `C-x b`, `C-x k`, `C-x C-b`, `C-x d`.
- `M-x` with fuzzy command completion.
- Windows: `C-x 2` split below, `C-x 3` split right, `C-x 1` delete other windows, `C-x 0` close current window, `C-x o` switch window.
- Auto-indent, `M-;` comment toggle, `TAB` / `S-TAB` indent/unindent.
- Tab-vs-space detection, CRLF preservation, bracketed paste, OSC 52 clipboard, auto-revert of unmodified buffers.

`C-h b` lists the available key bindings.

## Vim modal editing

### Modes

- `NORMAL`
- `INSERT`
- `VISUAL`
- `VISUAL LINE`

The current mode is displayed in the status line.

### Normal mode

| Keys | Action |
|---|---|
| `h` / `j` / `k` / `l` | Left / down / up / right |
| `w` / `b` / `e` | Next word / previous word / word end |
| `0` / `^` / `$` | Line start / first nonblank / line end |
| `gg` / `G` | Buffer start / buffer end |
| `C-u` / `C-d` | Scroll up / down |
| `i` / `I` | Insert at cursor / first nonblank |
| `a` / `A` | Append after cursor / end of line |
| `o` / `O` | Open line below / above |
| `v` / `V` | Visual / Visual Line mode |
| `d{motion}` | Delete (`dd`, `dw`, `d$`) |
| `c{motion}` | Change (`cc`, `cw`, `c$`) |
| `y{motion}` | Yank (`yy`, `yw`, `y$`) |
| `x` / `D` / `Y` | Delete character / to line end / yank line |
| `p` / `P` | Paste after / before cursor |
| `r{char}` / `~` | Replace character / toggle case |
| `u` / `C-r` | Undo / redo |

### Insert mode

`i`, `I`, `a`, `A`, `o` and `O` enter Insert mode.

- `ESC` / `C-[` returns to Normal mode.
- `RET` inserts a newline with auto-indent.
- `TAB` / `S-TAB` indent / unindent.
- Automatic pairing for `()`, `{}`, `[]`, `""`, `''` and `<>`.

### Visual mode

- `v`: character-wise selection.
- `V`: line-wise selection.
- `C-a`: select the entire buffer.
- `J` / `K`: move selected lines down / up.
- `d`: delete selection.
- `y`: yank selection.
- `c`: change selection.

## Space Leader

`SPC` is the Leader key.

| Key | Action |
|---|---|
| `SPC f` | Find file in project |
| `SPC p g` | Search text in project |
| `SPC e` | Open directory explorer |
| `SPC TAB` | Next buffer |
| `SPC S-TAB` | Previous buffer |
| `SPC t` | Reserved |
| `SPC w w` | Save buffer |
| `SPC w q` | Save and close / quit |
| `SPC q` | Close buffer |
| `SPC s` | Search current buffer |
| `SPC w v` | Vertical split |
| `SPC w s` | Horizontal split |
| `SPC w h/j/k/l` | Switch window focus |
| `SPC r r` | Query-replace word under cursor |
| `SPC ←/→/↑/↓` | Resize window split |
| `SPC x` | Select directory |

Additional existing shortcuts:

| Key | Action |
|---|---|
| `C-p` | Find file in project |
| `C-e` | Switch buffer |
| `S-Right` / `S-Left` | Next / previous buffer |

## Git

All Git operations use **libgit2**, without requiring the `git` executable.

### Space Leader Git (`SPC g`)

| Key | Action |
|---|---|
| `SPC g s` | Git status |
| `SPC g d` | Diff current file |
| `SPC g D` | Diff entire repository |
| `SPC g l` | Repository log |
| `SPC g L` | Current file log |
| `SPC g b` | File blame |
| `SPC g B` | Blame current line |
| `SPC g a` | Stage current file |
| `SPC g u` | Unstage current file |
| `SPC g c` | Commit |
| `SPC g A` | Amend commit |
| `SPC g o` | Checkout branch (`-b name` creates a branch) |
| `SPC g r` | Restore file from HEAD (confirmation required) |
| `SPC g n` | Next changed hunk |
| `SPC g p` | Previous changed hunk |
| `SPC g F` | Pull (fast-forward only) |
| `SPC g P` | Push |

## Dired — Explorador de arquivos

Abra o Dired com `SPC e` ou `Ctrl-x d`.

### Navegação

| Tecla | Ação |
|---|---|
| `j` / `k` | próxima / entrada anterior |
| `h` | subir para o diretório pai |
| `l` / `Enter` | abrir arquivo ou entrar no diretório |
| `gg` / `G` | topo / fim da listagem |
| `Ctrl-f` / `Ctrl-d` | rolar para baixo |
| `Ctrl-b` / `Ctrl-u` | rolar para cima |
| `q` | fechar Dired |

### Operações

| Tecla | Ação |
|---|---|
| `c` | copiar entrada selecionada |
| `x` | recortar entrada selecionada |
| `p` | colar no diretório atual |
| `r` | renomear entrada selecionada |
| `d` | excluir entrada selecionada |
| `%` | criar arquivo ou diretório |
| `R` | atualizar a listagem |

A cópia pode ser colada repetidamente. O recorte move a entrada quando colada. Para criar um diretório, informe um nome terminado em `/`.

### Original Emacs-style Git shortcuts

The existing `C-x` bindings remain available.

| Key | Action |
|---|---|
| `C-x g` / `C-x v d` | Open Git status |
| `C-x v =` / `C-x v D` | Diff current file / entire repository |
| `C-x v l` / `C-x v L` | Repository log / current file log |
| `C-x v g` / `C-x v b` | File blame / current-line blame |
| `C-x v a` / `C-x v u` | Stage / unstage current file |
| `C-x v c` / `C-x v A` | Commit / amend |
| `C-x v o` | Checkout branch |
| `C-x v r` | Restore file from HEAD |
| `C-x v n` / `C-x v p` | Next / previous changed hunk |
| `C-x v F` / `C-x v P` | Pull / push |

### Git status buffer

The status buffer provides interactive operations:

| Key | Action |
|---|---|
| `s` | Stage |
| `u` | Unstage |
| `RET` | Visit file |
| `d` | Diff |
| `D` | Full repository diff |
| `c` | Commit |
| `C` | Amend |
| `l` | Log |
| `b` | Checkout |
| `x` | Discard changes |
| `P` | Push |
| `F` | Pull |
| `g` | Refresh |

### Live Git integration

The mode line displays branch, ahead/behind status and staged/modified/untracked counts.

A Git gutter compares the buffer with HEAD:

- `▎` added or modified lines.
- `▁` deleted lines.

The gutter updates live while editing.

## Project commands

| Key | Action |
|---|---|
| `C-x p f` | Fuzzy find-file over tracked and untracked files |
| `C-x p g` | Regex grep over the project |
| `SPC f` | Project file finder |
| `SPC p g` | Project grep |

Project file search falls back to a directory walk outside Git repositories.

Project grep opens a results buffer. Use `RET`, `M-g n` or `M-g p` to visit matches.

## Syntax highlighting

Approximately 70 languages are supported through syntect's bundled syntax definitions and additional built-in tokenizers.

Syntect support includes Rust, C/C++, Go, Python, JavaScript, Java, C#, PHP, Ruby, Lua, Haskell, SQL, Shell, Markdown, YAML, JSON, HTML, CSS, LaTeX, Diff and more.

Additional built-in tokenizers cover TypeScript, TSX, Kotlin, Swift, Zig, Dart, Elixir, Julia, TOML, INI, `.conf`, `.env`, Dockerfile, CMake, HCL/Terraform, Nix, Protobuf, GraphQL, PowerShell and Vim script.

Highlighting is incremental and restarts near the viewport in huge files.

- `M-x set-language` overrides language detection.
- Default colors come from the custom palette in `syntax::palette`.
- `OW_THEME=dark|light|<syntect theme name>` selects a built-in theme.
- Add language support through `src/langs.rs`.

## Not implemented (vs. upstream mg)

Keyboard macros, rectangles, advanced Dired operations, `.mg` startup files/extensions, registers and tags/cscope.

Push/pull use ssh-agent or the Git credential helper. These operations have not been tested against a real remote.

## Layout

| File | Responsibility |
|---|---|
| `buffer.rs` | Text, undo and search |
| `editor.rs` | Modes, prompts and dispatch |
| `commands.rs` | Emacs commands and keymap |
| `vim.rs` | Vim modal editing and Leader bindings |
| `gitcmds.rs` | Git and project commands |
| `git.rs` | libgit2 wrapper |
| `syntax.rs` / `langs.rs` | Syntax highlighting |
| `ui.rs` | ratatui rendering |

## Tests

```bash
cargo test
```

Includes unit tests and end-to-end tests through simulated key events, including a complete Git workflow.

