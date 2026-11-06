//! GSC syntax: lexer, resilient parser, lossless CST, typed AST views and dialect gating.
//!
//! [`parse`] never fails and never panics: it returns a tree that holds every byte of the input
//! plus a list of syntax errors. The parser accepts the superset of every dialect;
//! [`check_dialect`] then reports what the selected game lacks. See `grammar/gsc.ungram`.
//!
//! A [`Dialect`] reads retail feature values by default; [`Dialect::with_mode`] with
//! [`Mode::Community`] reads the community-compiler values (gsc-tool, Plutonium). Macro uses are
//! expanded in a side table ([`macro_expansions`]), never in the tree.

mod check;
mod dialect;
mod grammar;
mod lexer;
mod macros;
mod names;
mod parser;
mod syntax_kind;
mod text;

pub mod ast;

pub use ast::first_significant_token;

pub use check::check_dialect;
pub use dialect::{Dialect, FeatureValue, Gate, Mode, ProfileError};
pub use lexer::{tokenize, Token};
pub use macros::{
    header_paths, macro_expansions, macro_expansions_with, ExpandedToken, HeaderSet, MacroDef,
    MacroExpansion, MacroTable,
};
pub use rowan::{GreenNode, NodeOrToken, TextRange, TextSize, WalkEvent};
pub use syntax_kind::{
    GscLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxNodeChildren, SyntaxToken,
};
pub use text::{is_binary, looks_decompiled, Encoding, LineCol, LineIndex, SourceText};

/// A syntax or dialect diagnostic. Codes starting with `E` are syntax errors; codes starting with
/// `D` are dialect diagnostics.
///
/// `E01xx` are lexical errors, `E02xx` parse errors and `E0301` a decompiler name that cannot be
/// tokenized (see [`SyntaxError::is_decompiler_name`]). `D001` is a feature the game lacks,
/// `D002` a missing `function` keyword, `D003` an import after a declaration, `D004`
/// `goto`/labels (`goto_labels`), `D005` a cast (`casts`) and `D006` a `builtin` call
/// (`builtin_call`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub code: &'static str,
    pub message: String,
    pub range: TextRange,
}

impl SyntaxError {
    pub fn new(code: &'static str, message: impl Into<String>, range: TextRange) -> SyntaxError {
        SyntaxError {
            code,
            message: message.into(),
            range,
        }
    }

    /// Is this a syntax error (as opposed to a dialect diagnostic)?
    pub fn is_syntax(&self) -> bool {
        self.code.starts_with('E')
    }

    /// Is this `E0301`: decompiler output whose hash lookup resolved to text no tokenizer can
    /// read as one name (`init heli sounds max_accuracy = x;`)? Such files cannot be fixed by
    /// the parser; tools may skip them.
    pub fn is_decompiler_name(&self) -> bool {
        self.code == "E0301"
    }
}

/// The result of parsing one file.
#[derive(Clone, Debug)]
pub struct Parse {
    green: GreenNode,
    errors: Vec<SyntaxError>,
}

impl Parse {
    /// The root of the untyped tree.
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// The typed root.
    pub fn tree(&self) -> ast::SourceFile {
        use ast::AstNode;
        ast::SourceFile::cast(self.syntax()).expect("the root is a SOURCE_FILE")
    }

    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// The text of the tree, which is the parsed text byte for byte. Walks the green tree, so it
    /// is cheaper than `syntax().to_string()`.
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(u32::from(self.green.text_len()) as usize);
        let mut stack = vec![self.green.children()];
        while let Some(top) = stack.last_mut() {
            match top.next() {
                Some(rowan::NodeOrToken::Token(t)) => out.push_str(t.text()),
                Some(rowan::NodeOrToken::Node(n)) => stack.push(n.children()),
                None => {
                    stack.pop();
                }
            }
        }
        out
    }

    /// Lexical and syntax errors, sorted by position.
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    /// An indented dump of the tree, one node or token per line.
    pub fn debug_tree(&self) -> String {
        debug_tree(&self.syntax())
    }
}

/// Parse `text` as a GSC file of `dialect`.
pub fn parse(text: &str, dialect: &Dialect) -> Parse {
    let (tokens, mut errors) = lexer::tokenize(text, dialect);
    let mut p = parser::Parser::new(text, &tokens, dialect);
    grammar::source_file(&mut p);
    let (events, parse_errors) = p.finish();
    let green = parser::build_tree(text, &tokens, events);
    errors.extend(parse_errors);
    errors.sort_by_key(|e| (e.range.start(), e.range.end()));
    if dialect.decompiled() && !errors.is_empty() {
        let mut items = Vec::new();
        let mut at = TextSize::from(0);
        for child in green.children() {
            let r = TextRange::at(at, child.text_len());
            if child.as_node().is_some() {
                items.push(r);
            }
            at = r.end();
        }
        names::reclassify(text, &tokens, &items, &mut errors);
    }
    Parse { green, errors }
}

/// An indented dump of a tree, one node or token per line (`KIND@start..end "text"`).
pub fn debug_tree(node: &SyntaxNode) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let mut indent = 0usize;
    for event in node.preorder_with_tokens() {
        match event {
            rowan::WalkEvent::Enter(e) => {
                let _ = write!(out, "{:indent$}", "", indent = indent * 2);
                match e {
                    rowan::NodeOrToken::Node(n) => {
                        let _ = writeln!(out, "{:?}@{:?}", n.kind(), n.text_range());
                        indent += 1;
                    }
                    rowan::NodeOrToken::Token(t) => {
                        let _ = writeln!(out, "{:?}@{:?} {:?}", t.kind(), t.text_range(), t.text());
                    }
                }
            }
            rowan::WalkEvent::Leave(rowan::NodeOrToken::Node(_)) => indent -= 1,
            rowan::WalkEvent::Leave(_) => {}
        }
    }
    out
}
