//! Lightweight language definitions for languages the bundled syntect set lacks
//! (TypeScript, Kotlin, Swift, TOML, Dockerfile, ...).

pub struct Lang {
    pub name: &'static str,
    pub exts: &'static [&'static str],
    pub files: &'static [&'static str],
    pub keywords: &'static [&'static str],
    pub types: &'static [&'static str],
    pub builtins: &'static [&'static str],
    pub line_comments: &'static [&'static str],
    pub block: Option<(&'static str, &'static str)>,
    pub nested: bool,
    /// Single-line string quote characters.
    pub quotes: &'static str,
    /// Multi-line string delimiters (triple quotes, backticks...).
    pub multi: &'static [&'static str],
    /// Case-insensitive keywords (stored lowercase).
    pub ci: bool,
    /// `@decorator` / `@Annotation`.
    pub attrs: bool,
    /// `[section]` lines.
    pub section: bool,
    /// `key = value` lines.
    pub kv: bool,
    /// Capitalised identifiers are types.
    pub cap_types: bool,
}

const BASE: Lang = Lang {
    name: "",
    exts: &[],
    files: &[],
    keywords: &[],
    types: &[],
    builtins: &[],
    line_comments: &[],
    block: None,
    nested: false,
    quotes: "\"'",
    multi: &[],
    ci: false,
    attrs: false,
    section: false,
    kv: false,
    cap_types: false,
};

const C_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));

pub static LANGS: &[Lang] = &[
    Lang {
        name: "TypeScript",
        exts: &["ts", "mts", "cts"],
        keywords: &[
            "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else",
            "enum", "export", "extends", "false", "finally", "for", "function", "if", "import", "in", "instanceof",
            "new", "null", "return", "super", "switch", "this", "throw", "true", "try", "typeof", "var", "void",
            "while", "with", "yield", "let", "static", "implements", "interface", "package", "private", "protected",
            "public", "as", "async", "await", "abstract", "declare", "from", "get", "set", "of", "type", "namespace",
            "module", "readonly", "keyof", "infer", "is", "satisfies", "undefined", "override", "never", "unique",
            "asserts", "accessor", "using",
        ],
        types: &[
            "string", "number", "boolean", "any", "unknown", "object", "symbol", "bigint", "Promise", "Array",
            "Record", "Partial", "Readonly", "Map", "Set", "Required", "Pick", "Omit",
        ],
        builtins: &[
            "Array", "async", "await", "Boolean", "break", "catch", "class", "const", "console",
            "Date", "debugger", "document", "Error", "export", "extends", "fetch", "finally",
            "for", "function", "if", "import", "JSON", "let", "Map", "Math", "new", "null",
            "Number", "Object", "Promise", "prototype", "return", "Set", "String", "super",
            "switch", "this", "throw", "try", "undefined", "var", "window", "yield",
        ],
        line_comments: &["//"],
        block: C_BLOCK,
        multi: &["`"],
        attrs: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "TSX",
        exts: &["tsx"],
        keywords: &[
            "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "enum",
            "export", "extends", "false", "finally", "for", "function", "if", "import", "in", "instanceof", "new",
            "null", "return", "super", "switch", "this", "throw", "true", "try", "typeof", "var", "void", "while",
            "yield", "let", "static", "implements", "interface", "private", "protected", "public", "as", "async",
            "await", "abstract", "declare", "from", "of", "type", "namespace", "readonly", "keyof", "undefined",
        ],
        types: &["string", "number", "boolean", "any", "unknown", "object", "never", "void", "Promise", "Array"],
        builtins: &[
            "Array", "async", "await", "Boolean", "break", "catch", "class", "const", "console",
            "Date", "debugger", "document", "Error", "export", "extends", "fetch", "finally",
            "for", "function", "if", "import", "JSON", "let", "Map", "Math", "new", "null",
            "Number", "Object", "Promise", "prototype", "return", "Set", "String", "super",
            "switch", "this", "throw", "try", "undefined", "var", "window", "yield",
        ],
        line_comments: &["//"],
        block: C_BLOCK,
        multi: &["`"],
        attrs: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "Kotlin",
        exts: &["kt", "kts"],
        keywords: &[
            "as", "break", "class", "continue", "do", "else", "false", "for", "fun", "if", "in", "interface", "is",
            "null", "object", "package", "return", "super", "this", "throw", "true", "try", "typealias", "val", "var",
            "when", "while", "by", "catch", "constructor", "delegate", "dynamic", "field", "file", "finally", "get",
            "import", "init", "param", "property", "receiver", "set", "setparam", "where", "abstract", "actual",
            "annotation", "companion", "const", "crossinline", "data", "enum", "expect", "external", "final", "infix",
            "inline", "inner", "internal", "lateinit", "noinline", "open", "operator", "out", "override", "private",
            "protected", "public", "reified", "sealed", "suspend", "tailrec", "vararg",
        ],
        types: &[
            "Int", "Long", "Short", "Byte", "Float", "Double", "Boolean", "Char", "String", "Any", "Unit", "Nothing",
            "List", "Map", "Set", "Array",
        ],
        builtins: &[],
        line_comments: &["//"],
        block: C_BLOCK,
        nested: true,
        multi: &["\"\"\""],
        attrs: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "Swift",
        exts: &["swift"],
        keywords: &[
            "associatedtype", "class", "deinit", "enum", "extension", "fileprivate", "func", "import", "init",
            "inout", "internal", "let", "open", "operator", "private", "precedencegroup", "protocol", "public",
            "rethrows", "static", "struct", "subscript", "typealias", "var", "break", "case", "catch", "continue",
            "default", "defer", "do", "else", "fallthrough", "for", "guard", "if", "in", "repeat", "return", "throw",
            "switch", "where", "while", "Any", "as", "await", "async", "false", "is", "nil", "self", "Self", "super",
            "throws", "true", "try", "some", "any", "actor", "lazy", "weak", "final", "override", "mutating",
        ],
        types: &["Int", "UInt", "Double", "Float", "Bool", "String", "Character", "Array", "Dictionary", "Set", "Optional", "Void"],
        builtins: &[],
        line_comments: &["//"],
        block: C_BLOCK,
        nested: true,
        multi: &["\"\"\""],
        attrs: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "Zig",
        exts: &["zig", "zon"],
        keywords: &[
            "addrspace", "align", "allowzero", "and", "anyframe", "anytype", "asm", "async", "await", "break",
            "callconv", "catch", "comptime", "const", "continue", "defer", "else", "enum", "errdefer", "error",
            "export", "extern", "fn", "for", "if", "inline", "linksection", "noalias", "nosuspend", "noinline",
            "opaque", "or", "orelse", "packed", "pub", "resume", "return", "struct", "suspend", "switch", "test",
            "threadlocal", "try", "union", "unreachable", "usingnamespace", "var", "volatile", "while", "true",
            "false", "null", "undefined",
        ],
        types: &[
            "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f16", "f32",
            "f64", "f128", "bool", "void", "type", "noreturn", "anyerror", "comptime_int", "comptime_float",
        ],
        builtins: &[],
        line_comments: &["//"],
        quotes: "\"'",
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "Dart",
        exts: &["dart"],
        keywords: &[
            "abstract", "as", "assert", "async", "await", "break", "case", "catch", "class", "const", "continue",
            "covariant", "default", "deferred", "do", "dynamic", "else", "enum", "export", "extends", "extension",
            "external", "factory", "false", "final", "finally", "for", "Function", "get", "hide", "if", "implements",
            "import", "in", "interface", "is", "late", "library", "mixin", "new", "null", "on", "operator", "part",
            "required", "rethrow", "return", "set", "show", "static", "super", "switch", "sync", "this", "throw",
            "true", "try", "typedef", "var", "void", "while", "with", "yield",
        ],
        types: &["int", "double", "num", "bool", "String", "List", "Map", "Set", "Future", "Stream", "Object"],
        builtins: &[],
        line_comments: &["//"],
        block: C_BLOCK,
        nested: true,
        multi: &["'''", "\"\"\""],
        attrs: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "Elixir",
        exts: &["ex", "exs", "heex"],
        keywords: &[
            "def", "defp", "defmodule", "defmacro", "defmacrop", "defstruct", "defprotocol", "defimpl", "defguard",
            "defdelegate", "defexception", "do", "end", "fn", "if", "else", "unless", "case", "cond", "when", "with",
            "for", "in", "and", "or", "not", "true", "false", "nil", "import", "alias", "use", "require", "quote",
            "unquote", "raise", "rescue", "try", "catch", "after", "receive", "send", "spawn", "then",
        ],
        builtins: &[],
        line_comments: &["#"],
        multi: &["\"\"\""],
        attrs: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "Julia",
        exts: &["jl"],
        keywords: &[
            "baremodule", "begin", "break", "catch", "const", "continue", "do", "else", "elseif", "end", "export",
            "false", "finally", "for", "function", "global", "if", "import", "let", "local", "macro", "module",
            "quote", "return", "struct", "mutable", "abstract", "primitive", "true", "try", "using", "while", "in",
            "isa", "where",
        ],
        types: &["Int", "Int64", "Float64", "String", "Bool", "Vector", "Matrix", "Array", "Dict", "Set", "Any", "Nothing"],
        builtins: &[],
        line_comments: &["#"],
        block: Some(("#=", "=#")),
        nested: true,
        multi: &["\"\"\""],
        attrs: true,
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "TOML",
        exts: &["toml"],
        files: &["Cargo.lock", "Pipfile", "poetry.lock"],
        keywords: &["true", "false", "inf", "nan"],
        builtins: &[],
        line_comments: &["#"],
        multi: &["\"\"\"", "'''"],
        section: true,
        kv: true,
        ..BASE
    },
    Lang {
        name: "INI",
        exts: &["ini", "cfg", "conf", "env", "properties", "editorconfig", "gitignore", "gitattributes", "service", "desktop", "rc"],
        files: &[".env", ".gitignore", ".gitattributes", ".editorconfig", ".gitconfig", ".npmrc", "config", "credentials"],
        keywords: &["true", "false", "yes", "no", "on", "off"],
        builtins: &[],
        line_comments: &["#", ";"],
        section: true,
        kv: true,
        ..BASE
    },
    Lang {
        name: "Dockerfile",
        exts: &["dockerfile"],
        files: &["Dockerfile", "Containerfile"],
        keywords: &[
            "from", "run", "cmd", "label", "maintainer", "expose", "env", "add", "copy", "entrypoint", "volume",
            "user", "workdir", "arg", "onbuild", "stopsignal", "healthcheck", "shell", "as",
        ],
        builtins: &[],
        line_comments: &["#"],
        ci: true,
        ..BASE
    },
    Lang {
        name: "CMake",
        exts: &["cmake"],
        files: &["CMakeLists.txt"],
        keywords: &[
            "if", "else", "elseif", "endif", "foreach", "endforeach", "while", "endwhile", "function", "endfunction",
            "macro", "endmacro", "return", "set", "unset", "add_executable", "add_library", "add_subdirectory",
            "target_link_libraries", "target_include_directories", "target_compile_definitions", "project",
            "cmake_minimum_required", "find_package", "include", "option", "message", "install", "list", "string",
            "file", "configure_file", "add_custom_command", "add_custom_target", "add_definitions", "set_target_properties",
        ],
        builtins: &[],
        line_comments: &["#"],
        block: Some(("#[[", "]]")),
        ci: true,
        ..BASE
    },
    Lang {
        name: "HCL",
        exts: &["tf", "tfvars", "hcl", "nomad"],
        keywords: &[
            "resource", "variable", "output", "module", "provider", "data", "locals", "terraform", "for", "in", "if",
            "true", "false", "null", "dynamic", "backend", "required_providers",
        ],
        builtins: &[],
        line_comments: &["#", "//"],
        block: C_BLOCK,
        quotes: "\"",
        kv: false,
        ..BASE
    },
    Lang {
        name: "Nix",
        exts: &["nix"],
        keywords: &[
            "let", "in", "if", "then", "else", "with", "inherit", "rec", "import", "assert", "true", "false", "null",
            "or", "builtins",
        ],
        builtins: &[],
        line_comments: &["#"],
        block: C_BLOCK,
        quotes: "\"",
        multi: &["''"],
        ..BASE
    },
    Lang {
        name: "Protobuf",
        exts: &["proto"],
        keywords: &[
            "syntax", "package", "import", "option", "message", "enum", "service", "rpc", "returns", "stream",
            "repeated", "optional", "required", "oneof", "map", "reserved", "extend", "extensions", "true", "false",
            "public", "weak",
        ],
        types: &[
            "double", "float", "int32", "int64", "uint32", "uint64", "sint32", "sint64", "fixed32", "fixed64",
            "sfixed32", "sfixed64", "bool", "string", "bytes",
        ],
        builtins: &[],
        line_comments: &["//"],
        block: C_BLOCK,
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "GraphQL",
        exts: &["graphql", "gql"],
        keywords: &[
            "query", "mutation", "subscription", "fragment", "on", "type", "interface", "union", "enum", "input",
            "scalar", "schema", "extend", "directive", "implements", "true", "false", "null",
        ],
        types: &["Int", "Float", "String", "Boolean", "ID"],
        builtins: &[],
        line_comments: &["#"],
        quotes: "\"",
        multi: &["\"\"\""],
        cap_types: true,
        ..BASE
    },
    Lang {
        name: "PowerShell",
        exts: &["ps1", "psm1", "psd1"],
        keywords: &[
            "begin", "break", "catch", "class", "continue", "data", "do", "dynamicparam", "else", "elseif", "end",
            "exit", "filter", "finally", "for", "foreach", "from", "function", "if", "in", "param", "process",
            "return", "switch", "throw", "trap", "try", "until", "using", "while", "workflow",
        ],
        builtins: &[],
        line_comments: &["#"],
        block: Some(("<#", "#>")),
        ci: true,
        ..BASE
    },
    Lang {
        name: "Vim script",
        exts: &["vim"],
        files: &[".vimrc", "vimrc"],
        keywords: &[
            "if", "else", "elseif", "endif", "for", "endfor", "while", "endwhile", "function", "endfunction", "let",
            "unlet", "set", "call", "return", "execute", "autocmd", "augroup", "command", "map", "noremap", "nnoremap",
            "inoremap", "vnoremap", "syntax", "highlight", "try", "catch", "finally", "endtry", "source",
        ],
        builtins: &[],
        line_comments: &["\""],
        quotes: "'",
        ..BASE
    },
];

pub fn find(ext: Option<&str>, filename: Option<&str>) -> Option<&'static Lang> {
    if let Some(f) = filename {
        if let Some(l) = LANGS.iter().find(|l| l.files.contains(&f)) {
            return Some(l);
        }
    }
    let ext = ext?;
    LANGS.iter().find(|l| l.exts.contains(&ext))
}

pub fn by_name(name: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.name.eq_ignore_ascii_case(name))
}

pub fn completion_words(name: &str) -> Vec<&'static str> {
    let lang = by_name(name);
    let mut words: Vec<&'static str> = lang
        .map(|lang| {
            lang.keywords.iter()
                .chain(lang.types.iter())
                .chain(lang.builtins.iter())
                .copied()
                .collect()
        })
        .unwrap_or_default();

    // Specific built-ins migration for Python, Rust, JS, Go, C, C++
    let extra_builtins: &[&str] = match name.to_ascii_lowercase().as_str() {
        "python" => &[
            "abs", "all", "and", "any", "as", "assert", "async", "await", "bin", "bool",
            "break", "bytearray", "bytes", "callable", "class", "classmethod", "chr", "compile",
            "complex", "del", "dict", "dir", "divmod", "elif", "else", "enumerate", "eval",
            "except", "exec", "filter", "finally", "float", "for", "format", "frozenset",
            "False", "None", "True", "from", "getattr", "global", "hasattr", "hash", "help",
            "hex", "if", "import", "in", "input", "int", "is", "isinstance", "issubclass",
            "iter", "lambda", "len", "list", "locals", "map", "max", "memoryview", "min",
            "next", "nonlocal", "not", "object", "oct", "open", "or", "ord", "pass", "pow",
            "print", "property", "raise", "range", "repr",
            "return", "reversed", "round", "set", "setattr", "slice", "sorted", "staticmethod",
            "str", "sum", "super", "try", "tuple", "type", "vars", "while", "with", "yield",
            "zip", "self", "cls", "Path", "os", "sys", "json", "re", "math", "datetime",
            "typing",
        ],
        "rust" => &[
            "as", "async", "await", "bool", "Box", "break", "char", "const", "continue", "crate",
            "dbg", "default", "derive", "else", "enum", "Err", "error", "extern", "false", "fn",
            "for", "format", "From", "HashMap", "HashSet", "if", "impl", "in", "into", "let",
            "loop", "macro_rules", "match", "mod", "move", "mut", "None", "Ok", "Option", "panic",
            "Path", "PathBuf", "print", "println", "pub", "Result", "return", "self", "Self",
            "Some", "String", "struct", "super", "trait", "true", "type", "u8", "usize", "Vec",
            "where", "while",
        ],
        "javascript" | "jsx" | "typescript" | "tsx" => &[
            "Array", "async", "await", "Boolean", "break", "catch", "class", "const", "console",
            "Date", "debugger", "document", "Error", "export", "extends", "fetch", "finally",
            "for", "function", "if", "import", "JSON", "let", "Map", "Math", "new", "null",
            "Number", "Object", "Promise", "prototype", "return", "Set", "String", "super",
            "switch", "this", "throw", "try", "undefined", "var", "window", "yield",
        ],
        "go" => &[
            "append", "bool", "byte", "cap", "case", "chan", "close", "complex", "const",
            "context", "copy", "defer", "delete", "else", "error", "fallthrough", "false",
            "float32", "float64", "for", "func", "go", "goto", "if", "imag", "int", "int32",
            "int64", "iota", "len", "make", "map", "max", "min", "new", "nil", "panic", "print",
            "println", "range", "recover", "return", "rune", "select", "string", "struct", "true",
            "uint", "uint64", "var",
        ],
        "c" => &[
            "auto", "bool", "break", "case", "char", "const", "continue", "default", "do",
            "double", "else", "enum", "extern", "false", "FILE", "float", "for", "free",
            "if", "include", "int", "long", "malloc", "memcpy", "NULL", "printf", "return",
            "scanf", "short", "signed", "sizeof", "static", "stderr", "stdin", "stdout",
            "struct", "switch", "typedef", "union", "unsigned", "void", "while",
        ],
        "c++" | "cpp" => &[
            "auto", "bool", "break", "case", "catch", "char", "class", "const", "constexpr",
            "continue", "cout", "delete", "double", "else", "enum", "false", "float", "for",
            "if", "include", "int", "long", "map", "namespace", "new", "nullptr", "private",
            "public", "return", "size_t", "std", "string", "struct", "switch", "template",
            "this", "true", "try", "typedef", "using", "vector", "void", "while",
        ],
        _ => &[],
    };

    if !extra_builtins.is_empty() {
        words.extend(extra_builtins.iter().copied());
    }
    
    // Fallback for unknown languages: add a generic set of useful words.
    if words.is_empty() && lang.is_none() {
        words.extend([
            "add", "append", "async", "await", "bool", "break", "build", "case", "catch",
            "char", "class", "close", "const", "continue", "create", "debug", "def", "delete",
            "do", "else", "enum", "error", "false", "file", "filter", "find", "float", "for",
            "format", "from", "func", "function", "get", "if", "import", "init", "input", "int",
            "interface", "item", "len", "list", "log", "main", "make", "map", "match", "max",
            "min", "module", "new", "nil", "null", "object", "open", "option", "print",
            "println", "private", "public", "read", "return", "result", "self", "set", "sort",
            "static", "string", "struct", "test", "this", "throw", "true", "try", "type",
            "update", "var", "void", "while", "write", "yield", "Array", "Boolean", "Error",
            "List", "Map", "None", "Ok", "Option", "Path", "Promise", "Result", "Set", "Some",
            "String", "Vec",
        ]);
    }

    words
}
