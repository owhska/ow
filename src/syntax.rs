//! Syntax highlighting: syntect for the languages it bundles, a small
//! built-in tokenizer (see `langs.rs`) for the rest. Both are incremental:
//! per-line start states are cached and invalidated from the edited line.
use crate::langs::{self, Lang};
use ratatui::style::{Color, Modifier, Style};
use std::path::Path;
use std::sync::OnceLock;
use syntect::highlighting::{
    FontStyle, HighlightIterator, HighlightState, Highlighter as SyHl, Style as SyStyle, Theme, ThemeSet,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEMES: OnceLock<(String, ThemeSet)> = OnceLock::new();

fn ss() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn themes() -> &'static (String, ThemeSet) {
    THEMES.get_or_init(|| {
        let mut ts = ThemeSet::load_defaults();
        ts.themes.insert(CUSTOM.to_string(), custom_theme());
        // OW_THEME: unset/"ow" = custom palette, "dark"/"light" = built-ins, or any syntect theme name
        let want = match std::env::var("OW_THEME").ok().as_deref() {
            Some("light") => "InspiredGitHub".to_string(),
            Some("dark") => "base16-ocean.dark".to_string(),
            None => CUSTOM.to_string(),
            Some(n) => n.to_string(),
        };
        let name = if ts.themes.contains_key(&want) { want } else { CUSTOM.to_string() };
        (name, ts)
    })
}

fn theme() -> &'static Theme {
    let (n, ts) = themes();
    &ts.themes[n]
}

fn is_light() -> bool {
    let n = &themes().0;
    n.contains("light") || n == "InspiredGitHub"
}

fn truecolor() -> bool {
    static TC: OnceLock<bool> = OnceLock::new();
    *TC.get_or_init(|| {
        std::env::var("COLORTERM").map(|v| v.contains("truecolor") || v.contains("24bit")).unwrap_or(false)
    })
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    if truecolor() {
        return Color::Rgb(r, g, b);
    }
    let (ri, gi, bi) = (r as i32, g as i32, b as i32);
    if (ri - gi).abs() < 10 && (gi - bi).abs() < 10 {
        let avg = (ri + gi + bi) / 3;
        if avg < 4 {
            return Color::Indexed(16);
        }
        if avg > 247 {
            return Color::Indexed(231);
        }
        return Color::Indexed(232 + ((avg - 8).max(0) * 24 / 247).min(23) as u8);
    }
    let q = |v: i32| ((v * 5 + 127) / 255) as u8;
    Color::Indexed(16 + 36 * q(ri) + 6 * q(gi) + q(bi))
}

// ------------------------------------------------------- custom "ow" theme
/// The default colour scheme. Edit the values here to retheme the whole editor
/// (syntax, built-in tokenizer, git gutter, selection, current line).
pub mod palette {
    use ratatui::style::Color;
    pub type Rgb = (u8, u8, u8);
    pub const FG: Rgb = (0xFF, 0xFF, 0xFF);
    pub const DIM: Rgb = (0x98, 0x98, 0x98);
    pub const KEYWORD: Rgb = (0xFE, 0xAC, 0xD0); // pink
    pub const FUNC: Rgb = (0x79, 0xA8, 0xFF); // blue
    pub const STRING: Rgb = (0x6A, 0xE4, 0xB9); // aqua green
    pub const TYPE: Rgb = (0x00, 0xD3, 0xD0); // cyan
    pub const CONST: Rgb = (0xD0, 0xBC, 0x00); // yellow
    pub const VARIABLE: Rgb = FG;
    pub const COMMENT: Rgb = DIM;
    pub const SPECIAL: Rgb = (0xB6, 0xA0, 0xFF); // lilac
    pub const ERROR: Rgb = (0xFF, 0x80, 0x59); // coral
    pub const WARN: Rgb = CONST;
    pub const INFO: Rgb = (0x44, 0xBC, 0x44); // green
    pub const SEL: Rgb = (0x2F, 0x44, 0x7F);
    pub const LINE: Rgb = (0x1A, 0x1A, 0x1A);

    /// Palette entry -> terminal colour (truecolor or nearest of 256).
    pub fn c(rgb: Rgb) -> Color {
        super::rgb(rgb.0, rgb.1, rgb.2)
    }
}

const CUSTOM: &str = "ow";

fn is_custom() -> bool {
    themes().0 == CUSTOM
}

fn custom_theme() -> Theme {
    use palette::*;
    use syntect::highlighting::{Color as C, ScopeSelectors, StyleModifier, ThemeItem, ThemeSettings};
    let col = |p: Rgb| C { r: p.0, g: p.1, b: p.2, a: 255 };
    let mut scopes: Vec<ThemeItem> = Vec::new();
    let mut add = |sel: &str, fg: Option<Rgb>, fs: Option<FontStyle>| {
        if let Ok(scope) = sel.parse::<ScopeSelectors>() {
            scopes.push(ThemeItem { scope, style: StyleModifier { foreground: fg.map(col), background: None, font_style: fs } });
        }
    };
    // Later / more specific selectors win.
    add("comment, punctuation.definition.comment", Some(COMMENT), Some(FontStyle::ITALIC));
    add("string, punctuation.definition.string", Some(STRING), None);
    add("constant, support.constant, variable.other.constant, constant.numeric, keyword.other.unit", Some(CONST), None);
    add("constant.character.escape, constant.other.placeholder, constant.other.placeholder punctuation", Some(SPECIAL), None);
    add("keyword, storage, storage.modifier", Some(KEYWORD), None);
    add("keyword.operator, punctuation, meta.brace", Some(FG), None);
    add("punctuation.definition.string", Some(STRING), None);
    add("punctuation.definition.comment", Some(COMMENT), None);
    add(
        "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.namespace, entity.name.union, entity.other.inherited-class, support.type, support.class, storage.type.primitive, storage.type.numeric, storage.type.core, storage.type.struct, storage.type.enum, storage.type.trait, storage.type.class",
        Some(TYPE),
        None,
    );
    add("variable, support.variable, variable.parameter, variable.other", Some(VARIABLE), None);
    add("entity.name.function, support.function, variable.function, meta.function-call entity.name.function", Some(FUNC), None);
    add("variable.language, variable.parameter.function.language", Some(SPECIAL), None);
    add(
        "support.macro, entity.name.function.macro, entity.name.function.preprocessor, meta.preprocessor, meta.annotation, meta.attribute, punctuation.definition.annotation, variable.annotation, storage.modifier.lifetime, entity.name.lifetime",
        Some(SPECIAL),
        None,
    );
    add("entity.name.tag", Some(KEYWORD), None);
    add("entity.other.attribute-name", Some(FUNC), None);
    add("markup.heading, entity.name.section", Some(FUNC), Some(FontStyle::BOLD));
    add("markup.bold", None, Some(FontStyle::BOLD));
    add("markup.italic", None, Some(FontStyle::ITALIC));
    add("markup.underline.link, string.other.link", Some(FUNC), Some(FontStyle::UNDERLINE));
    add("markup.raw, markup.inline.raw, markup.quote", Some(STRING), None);
    add("markup.inserted, meta.diff.header.to-file", Some(INFO), None);
    add("markup.deleted, meta.diff.header.from-file", Some(ERROR), None);
    add("markup.changed", Some(WARN), None);
    add("meta.diff.header, meta.diff.index, meta.separator.diff", Some(DIM), None);
    add("meta.diff.range, punctuation.definition.range.diff", Some(SPECIAL), None);
    add("invalid.deprecated", Some(WARN), None);
    add("invalid, invalid.illegal", Some(ERROR), None);
    Theme {
        name: Some(CUSTOM.into()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(col(FG)),
            selection: Some(col(SEL)),
            line_highlight: Some(col(LINE)),
            ..Default::default()
        },
        scopes,
    }
}

// ---------------------------------------------------------------- palette
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tok {
    Normal,
    Keyword,
    Type,
    Str,
    Comment,
    Number,
    Func,
    Attr,
    Section,
    Key,
}

fn tok_style(t: Tok) -> Style {
    if is_custom() {
        use palette::*;
        let f = |p: Rgb| Style::default().fg(c(p));
        return match t {
            Tok::Normal => f(FG),
            Tok::Keyword => f(KEYWORD),
            Tok::Type => f(TYPE),
            Tok::Str => f(STRING),
            Tok::Comment => f(COMMENT).add_modifier(Modifier::ITALIC),
            Tok::Number => f(CONST),
            Tok::Func => f(FUNC),
            Tok::Attr => f(SPECIAL),
            Tok::Section => f(SPECIAL).add_modifier(Modifier::BOLD),
            Tok::Key => f(FUNC),
        };
    }
    let light = is_light();
    let c = |d: (u8, u8, u8), l: (u8, u8, u8)| {
        let (r, g, b) = if light { l } else { d };
        Style::default().fg(rgb(r, g, b))
    };
    match t {
        Tok::Normal => Style::default(),
        Tok::Keyword => c((180, 142, 173), (136, 57, 150)),
        Tok::Type => c((235, 203, 139), (150, 100, 0)),
        Tok::Str => c((163, 190, 140), (40, 120, 40)),
        Tok::Comment => c((101, 115, 126), (110, 120, 130)).add_modifier(Modifier::ITALIC),
        Tok::Number => c((208, 135, 112), (190, 70, 30)),
        Tok::Func => c((143, 161, 179), (30, 80, 170)),
        Tok::Attr => c((150, 181, 180), (0, 120, 130)),
        Tok::Section => c((235, 203, 139), (150, 100, 0)).add_modifier(Modifier::BOLD),
        Tok::Key => c((143, 161, 179), (30, 80, 170)),
    }
}

// ------------------------------------------------------- extra tokenizer
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum St {
    Normal,
    Block(u32),
    Multi(&'static str),
}

fn starts(ch: &[char], i: usize, s: &str) -> bool {
    let mut k = i;
    for c in s.chars() {
        if ch.get(k) != Some(&c) {
            return false;
        }
        k += 1;
    }
    true
}

fn fill(out: &mut [Tok], from: usize, to: usize, t: Tok) {
    let to = to.min(out.len());
    for x in out.iter_mut().take(to).skip(from) {
        *x = t;
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}
fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn tokenize(lang: &Lang, line: &str, mut st: St) -> (Vec<Tok>, St) {
    let ch: Vec<char> = line.chars().collect();
    let n = ch.len();
    let mut out = vec![Tok::Normal; n];
    let mut i = 0;

    if st == St::Normal {
        let first = ch.iter().position(|c| !c.is_whitespace());
        if let Some(s) = first {
            if lang.section && ch[s] == '[' {
                let end = lang
                    .line_comments
                    .iter()
                    .filter_map(|lc| (s..n).find(|&k| starts(&ch, k, lc)))
                    .min()
                    .unwrap_or(n);
                fill(&mut out, s, end, Tok::Section);
                i = end;
            } else if lang.kv {
                if let Some(eq) = ch.iter().position(|&c| c == '=') {
                    let key_ok = ch[..eq].iter().all(|&c| c.is_alphanumeric() || " _.-\"'/".contains(c));
                    if key_ok && eq > s && !lang.line_comments.iter().any(|lc| starts(&ch, s, lc)) {
                        fill(&mut out, s, eq, Tok::Key);
                        i = eq + 1;
                    }
                }
            }
        }
    }

    while i < n {
        match st {
            St::Block(d) => {
                let (open, close) = lang.block.expect("block state without block comment");
                if lang.nested && starts(&ch, i, open) {
                    fill(&mut out, i, i + open.len(), Tok::Comment);
                    i += open.chars().count();
                    st = St::Block(d + 1);
                } else if starts(&ch, i, close) {
                    let l = close.chars().count();
                    fill(&mut out, i, i + l, Tok::Comment);
                    i += l;
                    st = if d <= 1 { St::Normal } else { St::Block(d - 1) };
                } else {
                    out[i] = Tok::Comment;
                    i += 1;
                }
            }
            St::Multi(delim) => {
                if ch[i] == '\\' && delim != "''" {
                    fill(&mut out, i, i + 2, Tok::Str);
                    i += 2;
                } else if starts(&ch, i, delim) {
                    let l = delim.chars().count();
                    fill(&mut out, i, i + l, Tok::Str);
                    i += l;
                    st = St::Normal;
                } else {
                    out[i] = Tok::Str;
                    i += 1;
                }
            }
            St::Normal => {
                let c = ch[i];
                if let Some((open, _)) = lang.block {
                    if starts(&ch, i, open) {
                        let l = open.chars().count();
                        fill(&mut out, i, i + l, Tok::Comment);
                        i += l;
                        st = St::Block(1);
                        continue;
                    }
                }
                if lang.line_comments.iter().any(|lc| starts(&ch, i, lc)) {
                    fill(&mut out, i, n, Tok::Comment);
                    break;
                }
                if let Some(d) = lang.multi.iter().find(|d| starts(&ch, i, d)) {
                    let l = d.chars().count();
                    fill(&mut out, i, i + l, Tok::Str);
                    i += l;
                    st = St::Multi(d);
                    continue;
                }
                if lang.quotes.contains(c) {
                    let mut j = i + 1;
                    while j < n && ch[j] != c {
                        if ch[j] == '\\' {
                            j += 1;
                        }
                        j += 1;
                    }
                    let end = (j + 1).min(n);
                    fill(&mut out, i, end, Tok::Str);
                    i = end;
                    continue;
                }
                if lang.attrs && c == '@' && ch.get(i + 1).is_some_and(|&x| is_ident_start(x)) {
                    let mut j = i + 1;
                    while j < n && (is_ident(ch[j]) || ch[j] == '.') {
                        j += 1;
                    }
                    fill(&mut out, i, j, Tok::Attr);
                    i = j;
                    continue;
                }
                let prev_ident = i > 0 && is_ident(ch[i - 1]);
                if c.is_ascii_digit() && !prev_ident {
                    let mut j = i + 1;
                    while j < n && (ch[j].is_alphanumeric() || ch[j] == '_' || (ch[j] == '.' && ch.get(j + 1).is_some_and(|x| x.is_ascii_digit()))) {
                        j += 1;
                    }
                    fill(&mut out, i, j, Tok::Number);
                    i = j;
                    continue;
                }
                if is_ident_start(c) {
                    let mut j = i + 1;
                    while j < n && is_ident(ch[j]) {
                        j += 1;
                    }
                    let word: String = ch[i..j].iter().collect();
                    let w = if lang.ci { word.to_lowercase() } else { word };
                    let tok = if lang.keywords.contains(&w.as_str()) {
                        Tok::Keyword
                    } else if lang.types.contains(&w.as_str()) {
                        Tok::Type
                    } else if ch[j..].iter().find(|c| !c.is_whitespace()) == Some(&'(') {
                        Tok::Func
                    } else if lang.cap_types && ch[i].is_uppercase() && w.chars().any(|c| c.is_lowercase()) {
                        Tok::Type
                    } else {
                        Tok::Normal
                    };
                    fill(&mut out, i, j, tok);
                    i = j;
                    continue;
                }
                i += 1;
            }
        }
    }
    (out, st)
}

// ------------------------------------------------------------ public API
type SyPair = (ParseState, HighlightState);

enum Engine {
    Plain,
    Syntect { syntax: &'static SyntaxReference, hl: SyHl<'static>, states: Vec<SyPair>, base: usize },
    Extra { lang: &'static Lang, states: Vec<St>, base: usize },
}

pub struct Highlighter {
    engine: Engine,
    pub name: String,
    comment: Option<&'static str>,
}

const MAX_LINE: usize = 4000;
const MAX_LINES: usize = 300_000;
/// Lines parsed sequentially before we give up and restart near the target.
const JUMP_LIMIT: usize = 1500;
const RESTART_CONTEXT: usize = 200;

impl Highlighter {
    pub fn plain() -> Self {
        Highlighter { engine: Engine::Plain, name: "Text".into(), comment: None }
    }

    fn from_syntect(syntax: &'static SyntaxReference) -> Self {
        let hl = SyHl::new(theme());
        let st = init_state(syntax, &hl);
        Highlighter {
            comment: syntect_comment(&syntax.name),
            name: syntax.name.clone(),
            engine: Engine::Syntect { syntax, hl, states: vec![st], base: 0 },
        }
    }

    fn from_extra(lang: &'static Lang) -> Self {
        Highlighter {
            name: lang.name.to_string(),
            comment: lang.line_comments.first().copied(),
            engine: Engine::Extra { lang, states: vec![St::Normal], base: 0 },
        }
    }

    pub fn by_name(name: &str) -> Option<Self> {
        if let Some(l) = langs::by_name(name) {
            return Some(Self::from_extra(l));
        }
        ss().find_syntax_by_name(name).or_else(|| ss().find_syntax_by_extension(name)).map(Self::from_syntect)
    }

    pub fn list_names() -> Vec<String> {
        let mut v: Vec<String> = langs::LANGS.iter().map(|l| l.name.to_string()).collect();
        v.extend(ss().syntaxes().iter().filter(|s| !s.hidden).map(|s| s.name.clone()));
        v.sort();
        v.dedup();
        v
    }

    pub fn detect(path: Option<&Path>, first_line: &str) -> Self {
        let fname = path.and_then(|p| p.file_name()).and_then(|s| s.to_str());
        let ext = path.and_then(|p| p.extension()).and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase());
        if let Some(l) = langs::find(ext.as_deref(), fname) {
            return Self::from_extra(l);
        }
        let alias = match ext.as_deref() {
            Some("jsx" | "mjs" | "cjs" | "es6") => Some("js"),
            Some("jsonc" | "json5" | "geojson" | "webmanifest") => Some("json"),
            Some("vue" | "svelte" | "htm" | "xhtml") => Some("html"),
            Some("zsh" | "ksh" | "fish") => Some("sh"),
            Some("hpp" | "hh" | "hxx" | "cc" | "cxx" | "ino") => Some("cpp"),
            Some("pyw" | "pyi" | "bzl") => Some("py"),
            Some("mdx" | "markdown") => Some("md"),
            Some("kt") => None,
            other => other,
        };
        let s = ss();
        let found = alias
            .and_then(|e| s.find_syntax_by_extension(e))
            .or_else(|| fname.and_then(|f| s.find_syntax_by_extension(f)))
            .or_else(|| s.find_syntax_by_first_line(first_line));
        match found {
            Some(syn) if syn.name != "Plain Text" => Self::from_syntect(syn),
            _ => Self::plain(),
        }
    }

    /// Line-comment token used by `comment-dwim`.
    pub fn comment_token(&self) -> Option<&'static str> {
        self.comment
    }

    pub fn invalidate(&mut self, from_line: usize) {
        match &mut self.engine {
            Engine::Plain => {}
            Engine::Syntect { syntax, hl, states, base } => {
                if from_line < *base {
                    *base = 0;
                    *states = vec![init_state(syntax, hl)];
                } else {
                    states.truncate(from_line - *base + 1);
                }
            }
            Engine::Extra { states, base, .. } => {
                if from_line < *base {
                    *base = 0;
                    *states = vec![St::Normal];
                } else {
                    states.truncate(from_line - *base + 1);
                }
            }
        }
    }

    /// Per-character styles for `lines[idx]`, or None for plain text.
    ///
    /// Start-of-line states are cached. When the requested line is far past the
    /// cached region (e.g. jumping to the end of a huge file) parsing restarts
    /// a short distance before it instead of from line 0, trading perfect
    /// accuracy for responsiveness.
    pub fn line_styles(&mut self, lines: &[String], idx: usize) -> Option<Vec<Style>> {
        if lines.len() > MAX_LINES || lines.get(idx)?.len() > MAX_LINE {
            return None;
        }
        match &mut self.engine {
            Engine::Plain => None,
            Engine::Syntect { syntax, hl, states, base } => {
                if states.is_empty() || idx < *base {
                    *base = 0;
                    *states = vec![init_state(syntax, hl)];
                }
                if idx - *base >= states.len() && idx - (*base + states.len() - 1) > JUMP_LIMIT {
                    *base = idx.saturating_sub(RESTART_CONTEXT);
                    *states = vec![init_state(syntax, hl)];
                }
                while *base + states.len() <= idx {
                    let k = states.len() - 1;
                    let mut st = states[k].clone();
                    run_syntect(&mut st, hl, &lines[*base + k]);
                    states.push(st);
                }
                let mut st = states[idx - *base].clone();
                Some(run_syntect(&mut st, hl, &lines[idx]))
            }
            Engine::Extra { lang, states, base } => {
                if states.is_empty() || idx < *base {
                    *base = 0;
                    *states = vec![St::Normal];
                }
                if idx - *base >= states.len() && idx - (*base + states.len() - 1) > JUMP_LIMIT * 20 {
                    *base = idx.saturating_sub(RESTART_CONTEXT);
                    *states = vec![St::Normal];
                }
                while *base + states.len() <= idx {
                    let k = states.len() - 1;
                    let (_, ns) = tokenize(lang, &lines[*base + k], states[k]);
                    states.push(ns);
                }
                let (toks, _) = tokenize(lang, &lines[idx], states[idx - *base]);
                Some(toks.into_iter().map(tok_style).collect())
            }
        }
    }
}

fn init_state(syntax: &'static SyntaxReference, hl: &SyHl<'static>) -> SyPair {
    (ParseState::new(syntax), HighlightState::new(hl, ScopeStack::new()))
}

fn conv(s: SyStyle) -> Style {
    let fg = theme().settings.foreground;
    let mut st = Style::default();
    if Some(s.foreground) != fg || is_custom() {
        st = st.fg(rgb(s.foreground.r, s.foreground.g, s.foreground.b));
    }
    if s.font_style.contains(FontStyle::BOLD) {
        st = st.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        st = st.add_modifier(Modifier::ITALIC);
    }
    if s.font_style.contains(FontStyle::UNDERLINE) {
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    st
}

fn run_syntect(st: &mut SyPair, hl: &SyHl<'static>, line: &str) -> Vec<Style> {
    let l = format!("{line}\n");
    let ops = match st.0.parse_line(&l, ss()) {
        Ok(o) => o,
        Err(_) => return vec![Style::default(); line.chars().count()],
    };
    let mut out = Vec::with_capacity(line.len());
    for (style, text) in HighlightIterator::new(&mut st.1, &ops, &l, hl) {
        let s = conv(style);
        for c in text.chars() {
            if c != '\n' {
                out.push(s);
            }
        }
    }
    out
}

fn syntect_comment(name: &str) -> Option<&'static str> {
    Some(match name {
        "Rust" | "C" | "C++" | "Go" | "Java" | "JavaScript" | "C#" | "Objective-C" | "Objective-C++" | "PHP"
        | "Scala" | "Groovy" | "D" | "CSS" | "JSON" | "ActionScript" | "Pascal" => "//",
        "Python" | "Ruby" | "Ruby on Rails" | "Perl" | "YAML" | "Makefile" | "R" | "Tcl" | "Shell-Unix-Generic"
        | "Bourne Again Shell (bash)" | "Diff" | "Plain Text" | "Graphviz (DOT)" => "#",
        "Haskell" | "Lua" | "SQL" => "--",
        "LaTeX" | "TeX" | "Erlang" | "MATLAB" | "BibTeX" => "%",
        "Lisp" | "Clojure" => ";",
        "Batch File" => "REM ",
        "AppleScript" => "--",
        "OCaml" => return None,
        n if n.contains("Shell") => "#",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typescript_tokens() {
        let l = langs::by_name("TypeScript").unwrap();
        let (t, _) = tokenize(l, "const x: number = 42; // hi", St::Normal);
        assert_eq!(t[0], Tok::Keyword);
        assert_eq!(t[9], Tok::Type);
        assert_eq!(t[18], Tok::Number);
        assert_eq!(*t.last().unwrap(), Tok::Comment);
    }

    #[test]
    fn multiline_state() {
        let l = langs::by_name("Kotlin").unwrap();
        let (_, st) = tokenize(l, "val s = \"\"\"abc", St::Normal);
        assert!(matches!(st, St::Multi(_)));
        let (t, st) = tokenize(l, "def\"\"\" + 1", st);
        assert_eq!(t[0], Tok::Str);
        assert_eq!(st, St::Normal);
        let (_, st) = tokenize(l, "/* a /* b", St::Normal);
        assert_eq!(st, St::Block(2));
    }

    #[test]
    fn toml_sections() {
        let l = langs::by_name("TOML").unwrap();
        let (t, _) = tokenize(l, "[package]", St::Normal);
        assert!(t.iter().all(|x| *x == Tok::Section));
        let (t, _) = tokenize(l, "name = \"x\"", St::Normal);
        assert_eq!(t[0], Tok::Key);
        assert_eq!(t[8], Tok::Str);
    }

    #[test]
    fn detect_languages() {
        for (f, want) in [
            ("a.rs", "Rust"),
            ("a.py", "Python"),
            ("a.ts", "TypeScript"),
            ("a.toml", "TOML"),
            ("Dockerfile", "Dockerfile"),
            ("Makefile", "Makefile"),
            ("a.go", "Go"),
            ("a.kt", "Kotlin"),
            ("a.jsx", "JavaScript"),
            ("a.md", "Markdown"),
        ] {
            let h = Highlighter::detect(Some(Path::new(f)), "");
            assert_eq!(h.name, want, "{f}");
        }
    }

    #[test]
    fn syntect_styles_len() {
        let mut h = Highlighter::detect(Some(Path::new("a.rs")), "");
        let lines = vec!["fn main() {".to_string(), "    let x = 1;".to_string()];
        let s = h.line_styles(&lines, 1).unwrap();
        assert_eq!(s.len(), lines[1].chars().count());
    }
}
