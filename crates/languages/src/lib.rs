use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use arborium::tree_sitter::{Language as ParserGrammar, Query};
use lazy_static::lazy_static;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use warp_editor::content::text::IndentUnit;
use warp_util::standardized_path::StandardizedPath;

#[derive(RustEmbed)]
#[folder = "grammars"]
struct Grammars;

lazy_static! {
    static ref LANGUAGE_REGISTRY: LanguageRegistry = LanguageRegistry::new();
}

pub const SUPPORTED_LANGUAGES: [&str; 35] = [
    "rust",
    "golang",
    "yaml",
    "python",
    "javascript",
    "jsx",
    "typescript",
    "tsx",
    "java",
    "cpp",
    "shell",
    "csharp",
    "html",
    "css",
    "c",
    "json",
    "jq",
    "hcl",
    "lua",
    "ruby",
    "php",
    "toml",
    "swift",
    "kotlin",
    "scala",
    "powershell",
    "elixir",
    "sql",
    "starlark",
    "objective-c",
    "xml",
    "vue",
    "dockerfile",
    "nix",
    "markdown",
];

/// Registry that holds all of the supported languages.
pub struct LanguageRegistry {
    /// List of languages we support mapped from their display name. They are hold in Arc so they could be shared
    /// between different editors.
    languages: Mutex<HashMap<String, Arc<Language>>>,
}

impl LanguageRegistry {
    fn new() -> Self {
        Self {
            languages: Mutex::new(HashMap::new()),
        }
    }

    pub fn language_by_name(&self, name: &str) -> Option<Arc<Language>> {
        if !SUPPORTED_LANGUAGES.contains(&name) {
            return None;
        }

        let mut languages = self.languages.lock().expect("Mutex should not be poisoned");

        if let Some(lang) = languages.get(name) {
            return Some(lang.clone());
        }

        let language = Arc::new(load_language(name)?);
        languages.insert(name.to_string(), language.clone());
        Some(language)
    }
}

/// Find the corresponding language entry by a standardized filename.
pub fn language_by_filename(path: &StandardizedPath) -> Option<Arc<Language>> {
    language_by_filename_parts(path.file_name(), path.extension())
}

/// Find the corresponding language entry by a local filesystem filename.
pub fn language_by_local_filename(path: &Path) -> Option<Arc<Language>> {
    language_by_filename_parts(
        path.file_name().and_then(|file_name| file_name.to_str()),
        path.extension().and_then(|extension| extension.to_str()),
    )
}

/// Normalizes common language-name aliases to their canonical internal names.
/// For example, "go" -> "golang", "bash" -> "shell", "md" -> "markdown".
fn normalize_language_name(name: &str) -> &str {
    match name {
        "go" => "golang",
        "bash" | "sh" | "zsh" => "shell",
        "js" => "javascript",
        "ts" => "typescript",
        "py" => "python",
        "rb" => "ruby",
        "rs" => "rust",
        "cs" | "c#" => "csharp",
        "c++" => "cpp",
        "objc" | "objective_c" => "objective-c",
        "terraform" | "tf" => "hcl",
        "kt" => "kotlin",
        "docker" | "containerfile" => "dockerfile",
        "md" => "markdown",
        other => other,
    }
}

pub fn language_by_name(name: &str) -> Option<Arc<Language>> {
    let normalized = normalize_language_name(name);
    LANGUAGE_REGISTRY.language_by_name(normalized)
}

fn language_by_filename_parts(
    filename: Option<&str>,
    extension: Option<&str>,
) -> Option<Arc<Language>> {
    // First check for specific filenames that don't use extensions.
    if let Some(filename) = filename {
        match filename {
            // Bash config files
            ".bashrc" | ".bash_profile" => {
                return language_by_name("shell");
            }
            // ZSH config files
            ".zshrc" | ".zsh_profile" | ".zprofile" => {
                return language_by_name("shell");
            }
            // Bazel build files
            "BUILD" | "WORKSPACE" => {
                return language_by_name("starlark");
            }
            // Dockerfiles
            "Dockerfile" | "Containerfile" | "dockerfile" | "containerfile" => {
                return language_by_name("dockerfile");
            }
            _ => {
                // Also match Dockerfile variants like Dockerfile.dev, Dockerfile.prod
                if filename.starts_with("Dockerfile.") || filename.starts_with("Containerfile.") {
                    return language_by_name("dockerfile");
                }
            }
        }
    }

    let extension = extension?;
    match extension {
        "rs" => language_by_name("rust"),
        "go" => language_by_name("golang"),
        "yml" | "yaml" => language_by_name("yaml"),
        "py" | "py3" | "pyw" | "pyi" => language_by_name("python"),
        "js" | "cjs" | "mjs" => language_by_name("javascript"),
        "jsx" => language_by_name("jsx"),
        "tsx" => language_by_name("tsx"),
        "ts" | "cts" | "mts" => language_by_name("typescript"),
        "java" | "groovy" | "gvy" | "gy" | "gsh" => language_by_name("java"),
        "cpp" | "cxx" | "cc" | "h" | "hh" | "hpp" | "hxx" | "H" | "h++" => language_by_name("cpp"),
        "sh" | "zsh" | "bash" | "command" => language_by_name("shell"),
        "cs" => language_by_name("csharp"),
        "html" | "htm" => language_by_name("html"),
        "css" => language_by_name("css"),
        "c" => language_by_name("c"),
        "json" => language_by_name("json"),
        "jq" => language_by_name("jq"),
        "tf" | "hcl" | "tfvars" => language_by_name("hcl"),
        "lua" => language_by_name("lua"),
        "nix" => language_by_name("nix"),
        "rb" => language_by_name("ruby"),
        "php" | "phtml" => language_by_name("php"),
        "toml" => language_by_name("toml"),
        "swift" => language_by_name("swift"),
        "kt" | "kts" => language_by_name("kotlin"),
        "scala" | "sbt" | "sc" => language_by_name("scala"),
        "ps1" | "pwsh" => language_by_name("powershell"),
        "ex" | "exs" => language_by_name("elixir"),
        "sql" => language_by_name("sql"),
        "bzl" | "bazel" => language_by_name("starlark"),
        "m" | "mm" => language_by_name("objective-c"),
        "xml" => language_by_name("xml"),
        "vue" => language_by_name("vue"),
        "dockerfile" => language_by_name("dockerfile"),
        "md" | "markdown" => language_by_name("markdown"),
        _ => None,
    }
}

/// The tree-sitter parser grammar for a language, together with every query that is
/// only meaningful in its presence.
///
/// Held behind an `Option` on [`Language`] because a grammar is a compile-time
/// artifact: it exists only when the matching `arborium` `lang-*` feature is enabled.
pub struct LanguageGrammar {
    /// Tree-sitter parser grammar.
    pub grammar: ParserGrammar,
    /// Query for syntax highlighting.
    pub highlight_query: Query,
    /// Query for auto indent.
    pub indents_query: Option<Query>,
    /// Query for parsing symbols.
    pub symbols_query: Option<Query>,
}

/// A supported language: the metadata that always applies, plus the tree-sitter
/// grammar when one is compiled in.
///
/// Everything outside `grammar` comes from the language's embedded `config.yaml`
/// and needs no parser, so it is available even in builds that ship no grammars.
/// In the future this will also be the entry point for LSP.
pub struct Language {
    /// Tree-sitter grammar and the queries derived from it.
    ///
    /// `None` when no tree-sitter grammar is compiled in for this language, which
    /// in this fork is every language (see the `arborium` dependency in the
    /// workspace `Cargo.toml`). Callers must degrade to unhighlighted text rather
    /// than substitute anything for a missing parse.
    pub grammar: Option<LanguageGrammar>,
    /// Unit for each indent action.
    pub indent_unit: IndentUnit,
    /// Comment prefix.
    pub comment_prefix: Option<String>,
    /// Language-specific bracket pairs.
    pub bracket_pairs: Vec<(char, char)>,
    /// Display name for the language.
    pub display_name: String,
}

impl Language {
    /// Returns the display name of the language.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct LanguageConfig {
    display_name: String,
    indent_unit: IndentUnit,
    comment_prefix: Option<String>,
    #[serde(default)]
    brackets: Vec<BracketPair>,
}

#[derive(Serialize, Deserialize, Debug)]
struct BracketPair {
    start: String,
    end: String,
}

/// Map our internal language name to the canonical arborium language name.
fn to_arborium_name(lang: &str) -> &str {
    match lang {
        "golang" => "go",
        "shell" => "bash",
        "csharp" => "c-sharp",
        "jsx" => "javascript",
        "objective-c" => "objc",
        "sql" => "sql",
        other => other,
    }
}

/// Get the bundled highlight query from arborium for a given language.
///
/// LOCAL FORK: always `None`. Upstream matched 34 languages onto
/// `arborium::lang_*::HIGHLIGHTS_QUERY` constants; those grammars are no longer
/// compiled in (see the `arborium` dependency in the workspace Cargo.toml), so
/// the match arms would not resolve. Returning `None` leaves [`Language::grammar`]
/// empty; the language's metadata is unaffected.
fn get_arborium_highlight_query(_lang: &str) -> Option<&str> {
    None
}

/// Load a language's metadata, plus its tree-sitter grammar if one is compiled in.
///
/// Returns `None` only when `lang` is not one of [`SUPPORTED_LANGUAGES`] or its
/// embedded `config.yaml` is missing. A missing grammar is not a failure: the
/// returned [`Language`] simply has `grammar: None`, which is what lets file-type
/// detection, display names, comment prefixes, bracket pairs and the indent unit
/// keep working in a build that ships no grammars.
fn load_language(lang: &str) -> Option<Language> {
    if !SUPPORTED_LANGUAGES.contains(&lang) {
        return None;
    }

    let config = load_config(lang)?;
    let bracket_pairs = config
        .brackets
        .into_iter()
        .filter_map(|bracket_pair| {
            let start = bracket_pair.start.chars().next()?;
            let end = bracket_pair.end.chars().next()?;
            Some((start, end))
        })
        .collect();

    Some(Language {
        grammar: load_grammar(lang),
        indent_unit: config.indent_unit,
        comment_prefix: config.comment_prefix,
        bracket_pairs,
        display_name: config.display_name,
    })
}

/// Load the tree-sitter grammar for `lang` and the queries that depend on it.
///
/// `None` when arborium has no grammar compiled in for the language, or when no
/// bundled highlight query is available for it.
fn load_grammar(lang: &str) -> Option<LanguageGrammar> {
    let grammar = arborium::get_language(to_arborium_name(lang))?;

    let highlight_query_str = get_arborium_highlight_query(lang)?;
    let highlight_query = Query::new(&grammar, highlight_query_str)
        .expect("arborium highlight query should be valid");

    let indents_query_path = [lang, "indents.scm"].join("\\");
    let indents_query = load_query(&indents_query_path, &grammar);

    let symbols_query_path = [lang, "identifiers.scm"].join("\\");
    let symbols_query = load_query(&symbols_query_path, &grammar);

    Some(LanguageGrammar {
        grammar,
        highlight_query,
        indents_query,
        symbols_query,
    })
}

/// Read and parse a language's embedded `config.yaml`.
///
/// `None` when no config is embedded for `lang`. A config that is present but
/// malformed is a broken build rather than a missing language, so it panics.
fn load_config(lang: &str) -> Option<LanguageConfig> {
    let path = [lang, "config.yaml"].join("\\");
    let file = <Grammars as RustEmbed>::get(&path)?;
    Some(
        serde_yaml::from_slice(&file.data).unwrap_or_else(|err| {
            panic!("Unable to deserialize the YAML content of {path}: {err}")
        }),
    )
}

fn load_query(path: &str, grammar: &ParserGrammar) -> Option<Query> {
    let file = <Grammars as RustEmbed>::get(path)?;
    let query_content = match file.data {
        Cow::Borrowed(inner) => Cow::Borrowed(std::str::from_utf8(inner).unwrap()),
        Cow::Owned(inner) => Cow::Owned(String::from_utf8(inner).unwrap()),
    };

    Some(
        Query::new(grammar, &query_content)
            .unwrap_or_else(|err| panic!("TSQuery creation should work from {path}: {err}")),
    )
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
