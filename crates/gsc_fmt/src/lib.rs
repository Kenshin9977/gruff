//! GSC formatter. Two styles: `full` (the default) reprints the tree through a Prettier-style
//! document IR ([`doc`]) and breaks long lines at [`Options::max_width`]; `minimal` keeps every
//! line break and only rewrites horizontal whitespace and indentation.
//!
//! Both keep every comment, string, preprocessor line and token; files with syntax errors are
//! left alone, and every changed result is verified (it parses, its non-whitespace tokens equal
//! the input's). Rules and the corpus measurements behind the defaults: `README.md`.

mod config;
mod diff;
mod doc;
mod full;
mod minimal;
mod range;

use gsc_syntax::{Dialect, SyntaxError, SyntaxKind};
use SyntaxKind::*;

pub use config::{find_config, ConfigError, FormatConfig};
pub use diff::unified_diff;
pub use range::{format_range, RangeEdit};

/// How lines are indented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indent {
    Tabs,
    /// This many spaces per level (1 to 16).
    Spaces(u8),
}

/// Spacing inside brackets: the parentheses of calls, parameter lists, control statements,
/// parenthesised expressions, vectors and literals ([`Options::paren_spacing`]), or the `[ ]` of
/// an index ([`Options::bracket_spacing`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParenSpacing {
    /// `foo( a, b )`, `if( x )`.
    Padded,
    /// `foo(a, b)`, `if(x)`.
    Compact,
    /// Leave the inside of the parentheses as written.
    Preserve,
}

impl ParenSpacing {
    pub fn from_name(name: &str) -> Option<ParenSpacing> {
        Some(match name {
            "padded" => ParenSpacing::Padded,
            "compact" => ParenSpacing::Compact,
            "preserve" => ParenSpacing::Preserve,
            _ => return None,
        })
    }
}

/// Which formatter runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Reprint with line breaking (the default).
    Full,
    /// Whitespace normalisation only; every line break stays.
    Minimal,
}

impl Style {
    pub fn from_name(name: &str) -> Option<Style> {
        Some(match name {
            "full" => Style::Full,
            "minimal" => Style::Minimal,
            _ => return None,
        })
    }
}

/// Where the `{` of a function, class or control statement goes (full style).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BraceStyle {
    /// On a line of its own (Allman): the style of every studio source in the corpus.
    NextLine,
    /// At the end of the header line (K&R); `} else {` on one line.
    SameLine,
}

impl BraceStyle {
    pub fn from_name(name: &str) -> Option<BraceStyle> {
        Some(match name {
            "next_line" => BraceStyle::NextLine,
            "same_line" => BraceStyle::SameLine,
            _ => return None,
        })
    }
}

/// The gap between `if`, `while`, `for`, `foreach`, `switch` and their `(` (full style).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeywordSpace {
    /// `if ( x )`.
    Space,
    /// `if( x )`.
    None,
    /// As written.
    Preserve,
}

impl KeywordSpace {
    pub fn from_name(name: &str) -> Option<KeywordSpace> {
        Some(match name {
            "space" => KeywordSpace::Space,
            "none" => KeywordSpace::None,
            "preserve" => KeywordSpace::Preserve,
            _ => return None,
        })
    }
}

/// How a list that does not fit on one line is broken (full style).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    /// As many items per line as fit, continuation lines indented one level.
    Fill,
    /// One item per line, the closing parenthesis on a line of its own.
    OnePerLine,
}

impl Wrap {
    pub fn from_name(name: &str) -> Option<Wrap> {
        Some(match name {
            "fill" => Wrap::Fill,
            "one_per_line" => Wrap::OnePerLine,
            _ => return None,
        })
    }
}

/// Formatter options. Build them with [`Options::for_game`] or [`Options::default`] and change
/// fields; new fields may be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub indent: Indent,
    pub paren_spacing: ParenSpacing,
    pub style: Style,
    /// The line width the full style breaks at (default 100).
    pub max_width: u16,
    /// The width of a tab when measuring lines (default 4).
    pub tab_width: u8,
    pub brace_style: BraceStyle,
    /// Spacing inside the `[ ]` of an index (full style).
    pub bracket_spacing: ParenSpacing,
    pub keyword_space: KeywordSpace,
    pub wrap: Wrap,
    /// Blank lines kept in a row between statements and declarations (full style; default 1).
    pub max_blank_lines: u8,
}

impl Default for Options {
    /// The style of BO3 (`t7`), the default game.
    fn default() -> Options {
        Options::for_game("t7")
    }
}

impl Options {
    /// The default style of a game's engine family, measured on the studios' sources (README):
    /// tabs, braces on their own line, padded parentheses except in CoD1, United Offensive and
    /// CoD2; compact index brackets where the sources mostly write `a[i]`; no space between a
    /// control keyword and `(` in WaW and BO1.
    pub fn for_game(game: &str) -> Options {
        let compact = matches!(game, "iw1" | "iw1-uo" | "iw2");
        let compact_index = matches!(
            game,
            "iw1" | "iw1-uo" | "iw2" | "t6" | "t7" | "iw6" | "iw7" | "iw8" | "s1" | "s2" | "h1"
        );
        let keyword_space = if matches!(game, "t4" | "t5") {
            KeywordSpace::None
        } else {
            KeywordSpace::Space
        };
        Options {
            indent: Indent::Tabs,
            paren_spacing: if compact {
                ParenSpacing::Compact
            } else {
                ParenSpacing::Padded
            },
            style: Style::Full,
            max_width: 100,
            tab_width: 4,
            brace_style: BraceStyle::NextLine,
            bracket_spacing: if compact_index {
                ParenSpacing::Compact
            } else {
                ParenSpacing::Padded
            },
            keyword_space,
            wrap: Wrap::Fill,
            max_blank_lines: 1,
        }
    }

    /// These options with another style.
    pub fn with_style(self, style: Style) -> Options {
        Options { style, ..self }
    }
}

/// Why a file was not formatted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    /// The input has syntax errors; it is left alone.
    Syntax(Vec<SyntaxError>),
    /// The formatter produced text that would change the program. This is a bug; the input is
    /// left alone.
    Unsafe(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Syntax(errors) => {
                write!(f, "{} syntax error(s)", errors.len())?;
                if let Some(e) = errors.first() {
                    write!(f, ", first: {} {}", e.code, e.message)?;
                }
                Ok(())
            }
            FormatError::Unsafe(msg) => write!(f, "internal formatter error: {msg}"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Format `text`, parsed as `dialect`. Returns the formatted text (equal to `text` when nothing
/// changes), or why the file is left alone.
pub fn format(text: &str, dialect: &Dialect, opts: &Options) -> Result<String, FormatError> {
    let parse = gsc_syntax::parse(text, dialect);
    let errors: Vec<SyntaxError> = parse
        .errors()
        .iter()
        .filter(|e| e.is_syntax())
        .cloned()
        .collect();
    if !errors.is_empty() {
        return Err(FormatError::Syntax(errors));
    }
    let root = parse.syntax();
    let out = match opts.style {
        Style::Minimal => minimal::format(&root, text, opts),
        Style::Full => full::format(&root, text, opts).map_err(FormatError::Unsafe)?,
    };
    if out != text {
        verify(text, &out, dialect).map_err(FormatError::Unsafe)?;
    }
    Ok(out)
}

/// A token of a file, reduced to what formatting must preserve.
#[derive(Debug, PartialEq, Eq)]
pub struct Lexeme<'a> {
    pub kind: SyntaxKind,
    pub text: &'a str,
}

/// Every token of `text` except whitespace, in order: code tokens, comments (a `//` comment
/// without its trailing blanks), preprocessor lines and inactive `#if` text. Two files with the
/// same lexemes differ only in whitespace.
pub fn lexemes<'a>(text: &'a str, dialect: &Dialect) -> Vec<Lexeme<'a>> {
    let (tokens, _) = gsc_syntax::tokenize(text, dialect);
    let mut out = Vec::with_capacity(tokens.len() / 2);
    let mut pos = 0usize;
    for t in tokens {
        let s = &text[pos..pos + t.len as usize];
        pos += t.len as usize;
        match t.kind {
            WHITESPACE => {}
            COMMENT => out.push(Lexeme {
                kind: t.kind,
                text: trim_hspace_end(s),
            }),
            kind => out.push(Lexeme { kind, text: s }),
        }
    }
    out
}

/// Check that `after` is `before` with only whitespace changed, and that it still parses.
pub fn verify(before: &str, after: &str, dialect: &Dialect) -> Result<(), String> {
    let a = lexemes(before, dialect);
    let b = lexemes(after, dialect);
    if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
        return Err(format!(
            "token {i} changed: {:?} became {:?}",
            a.get(i),
            b.get(i)
        ));
    }
    let parse = gsc_syntax::parse(after, dialect);
    if let Some(e) = parse.errors().iter().find(|e| e.is_syntax()) {
        return Err(format!(
            "the output does not parse: {} {}",
            e.code, e.message
        ));
    }
    Ok(())
}

pub(crate) fn is_hspace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{0B}' | '\u{0C}' | '\u{a0}' | '\u{feff}')
}

/// `s` without trailing horizontal whitespace, unless that would leave a `\` at the end of the
/// line (a line splice for a C-like preprocessor).
pub(crate) fn trim_hspace_end(s: &str) -> &str {
    let t = s.trim_end_matches(is_hspace);
    if t.ends_with('\\') && t.len() != s.len() {
        s
    } else {
        t
    }
}

/// The dominant line ending of a file: `\r\n` when it has more CRLF than lone LF.
pub(crate) fn line_ending(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    if crlf > lf {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests;
