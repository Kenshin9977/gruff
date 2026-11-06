//! Dialect gating: report syntax the selected game lacks. The tree is never changed.
//!
//! Each rule names the profile feature it reads (the `gate:` comments of `gsc.ungram`). A
//! feature the profile does not know stays silent. The walk runs over the green tree, so it
//! allocates nothing for the (usual) case where there is nothing to report.

use rowan::{GreenNodeData, GreenTokenData, NodeOrToken, TextRange, TextSize};

use crate::{Dialect, Gate, SyntaxError, SyntaxKind, SyntaxNode};
use SyntaxKind::*;

fn kind_of(raw: rowan::SyntaxKind) -> SyntaxKind {
    <crate::GscLanguage as rowan::Language>::kind_from_raw(raw)
}

/// A green node with its absolute offset and its parent's kind.
#[derive(Clone, Copy)]
struct Node<'g> {
    green: &'g GreenNodeData,
    offset: TextSize,
    parent: Option<SyntaxKind>,
    /// Is this the first child node of its parent?
    first_child: bool,
}

/// A token directly under a node.
#[derive(Clone, Copy)]
struct Tok<'g> {
    green: &'g GreenTokenData,
    offset: TextSize,
}

impl<'g> Tok<'g> {
    fn kind(self) -> SyntaxKind {
        kind_of(self.green.kind())
    }
    fn text(self) -> &'g str {
        self.green.text()
    }
    fn range(self) -> TextRange {
        TextRange::at(self.offset, self.green.text_len())
    }
}

impl<'g> Node<'g> {
    fn kind(self) -> SyntaxKind {
        kind_of(self.green.kind())
    }

    fn elements(
        self,
    ) -> impl Iterator<Item = (NodeOrToken<&'g GreenNodeData, &'g GreenTokenData>, TextSize)> {
        let mut offset = self.offset;
        self.green.children().map(move |c| {
            let at = offset;
            offset += c.text_len();
            (c, at)
        })
    }

    fn tokens(self) -> impl Iterator<Item = Tok<'g>> {
        self.elements().filter_map(|(c, offset)| match c {
            NodeOrToken::Token(t) => Some(Tok { green: t, offset }),
            NodeOrToken::Node(_) => None,
        })
    }

    fn children(self) -> impl Iterator<Item = Node<'g>> {
        let parent = self.kind();
        let mut first = true;
        self.elements().filter_map(move |(c, offset)| match c {
            NodeOrToken::Node(n) => {
                let first_child = std::mem::take(&mut first);
                Some(Node {
                    green: n,
                    offset,
                    parent: Some(parent),
                    first_child,
                })
            }
            NodeOrToken::Token(_) => None,
        })
    }

    fn token(self, kind: SyntaxKind) -> Option<Tok<'g>> {
        self.tokens().find(|t| t.kind() == kind)
    }

    fn first_child(self) -> Option<Node<'g>> {
        self.children().next()
    }

    /// The first token anywhere under the node (trivia excluded).
    fn first_token(self) -> Option<Tok<'g>> {
        for (c, offset) in self.elements() {
            match c {
                NodeOrToken::Token(t) => {
                    let t = Tok { green: t, offset };
                    if !t.kind().is_trivia() {
                        return Some(t);
                    }
                }
                NodeOrToken::Node(g) => {
                    let child = Node {
                        green: g,
                        offset,
                        parent: None,
                        first_child: false,
                    };
                    if let Some(t) = child.first_token() {
                        return Some(t);
                    }
                }
            }
        }
        None
    }

    /// The first line of the node, trimmed of trivia: enough to point at the construct. Built
    /// through the red tree, which is fine: it only runs when reporting.
    fn head_range(self) -> TextRange {
        let root = SyntaxNode::new_root(self.green.to_owned());
        let mut range: Option<TextRange> = None;
        for t in root
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
        {
            if t.kind().is_trivia() {
                if range.is_some() && t.text().contains(['\n', '\r']) {
                    break;
                }
                continue;
            }
            range = Some(range.map_or(t.text_range(), |r| r.cover(t.text_range())));
        }
        let r = range.unwrap_or_else(|| root.text_range());
        r + self.offset
    }
}

/// Where a diagnostic points: a node (its first line, computed only when reporting) or a range.
#[derive(Clone, Copy)]
enum At<'g> {
    Node(Node<'g>),
    Range(TextRange),
}

impl At<'_> {
    fn range(self) -> TextRange {
        match self {
            At::Node(n) => n.head_range(),
            At::Range(r) => r,
        }
    }
}

impl From<TextRange> for At<'_> {
    fn from(r: TextRange) -> Self {
        At::Range(r)
    }
}

impl<'g> From<Node<'g>> for At<'g> {
    fn from(n: Node<'g>) -> Self {
        At::Node(n)
    }
}

struct Checker<'a> {
    dialect: &'a Dialect,
    out: Vec<SyntaxError>,
    seen_declaration: bool,
}

impl Checker<'_> {
    fn report<'g>(&mut self, gate: Gate, at: impl Into<At<'g>>, what: &str, feature: &str) {
        self.report_as("D001", gate, at, what, feature);
    }

    /// [`Self::report`] under a dedicated code.
    fn report_as<'g>(
        &mut self,
        code: &'static str,
        gate: Gate,
        at: impl Into<At<'g>>,
        what: &str,
        feature: &str,
    ) {
        if gate == Gate::Off {
            let range = at.into().range();
            self.out.push(SyntaxError::new(
                code,
                format!(
                    "{what} is not available in {} (feature `{feature}`)",
                    self.dialect.id
                ),
                range,
            ));
        }
    }

    fn flag<'g>(&mut self, at: impl Into<At<'g>>, what: &str, feature: &str) {
        let gate = self.dialect.flag(feature);
        self.report(gate, at, what, feature);
    }

    fn node(&mut self, n: Node) {
        let d = self.dialect;
        let range = At::Node(n);
        match n.kind() {
            CLASS_DEF => self.flag(range, "`class`", "classes"),
            NEW_EXPR => self.flag(range, "`new`", "classes"),
            METHOD_CALL_EXPR => match n.token(ARROW) {
                Some(t) => self.flag(t.range(), "`->`", "classes"),
                None => self.flag(range, "`->`", "classes"),
            },
            FUNCTION_DEF => {
                self.seen_declaration = true;
                match (d.flag("function_keyword"), n.token(FUNCTION_KW)) {
                    (Gate::On, None) => self.out.push(SyntaxError::new(
                        "D002",
                        format!(
                            "function declarations need the `function` keyword in {}",
                            d.id
                        ),
                        range.range(),
                    )),
                    (Gate::Off, Some(t)) => self.report(
                        Gate::Off,
                        t.range(),
                        "the `function` keyword",
                        "function_keyword",
                    ),
                    _ => {}
                }
                for t in n.tokens() {
                    match t.kind() {
                        AUTOEXEC_KW => self.flag(t.range(), "`autoexec`", "autoexec_modifier"),
                        PRIVATE_KW => self.flag(t.range(), "`private`", "private_modifier"),
                        CODECALL_KW => self.flag(t.range(), "`codecall`", "codecall_modifier"),
                        _ => {}
                    }
                }
            }
            CLASS_VAR | CONSTRUCTOR | DESTRUCTOR => self.seen_declaration = true,
            EVENT_HANDLER_MODIFIER => self.flag(range, "`event_handler`", "event_handler_modifier"),
            PARAM => {
                match n.first_token().map(|t| t.kind()) {
                    Some(AMP) => {
                        self.flag(range, "a `&` reference parameter", "reference_parameters")
                    }
                    Some(STAR) => self.flag(range, "a `*` pointer parameter", "pointer_parameters"),
                    _ => {}
                }
                if let Some(dv) = n.children().find(|c| c.kind() == DEFAULT_VALUE) {
                    self.flag(dv, "a default parameter value", "default_parameters");
                }
            }
            VARARG_PARAM => self.flag(range, "`...`", "varargs"),
            FILE_CONSTANT => {
                self.seen_declaration = true;
                self.flag(range, "a file-scope constant", "file_scope_constants")
            }
            MACRO_CALL_ITEM => {
                self.flag(range, "a file-scope macro call", "file_scope_macro_calls")
            }
            DEV_BLOCK_ITEM | DEV_BLOCK_STMT => {
                self.flag(range, "a `/# #/` dev block", "dev_blocks")
            }
            USING_DIRECTIVE => {
                self.import_order(range, "`#using`");
                let g = d.is("import_directive", "using");
                self.report(g, range, "`#using`", "import_directive");
            }
            INCLUDE_DIRECTIVE => {
                self.import_order(range, "`#include`");
                let g = d.is("import_directive", "include");
                self.report(g, range, "`#include`", "import_directive");
            }
            NAMESPACE_DIRECTIVE => self.flag(range, "`#namespace`", "namespace_directive"),
            INSERT_DIRECTIVE => {
                let g = d.is("header_directive", "insert");
                self.report(g, range, "`#insert`", "header_directive");
            }
            INLINE_DIRECTIVE => {
                let g = d.is("header_directive", "inline");
                self.report(g, range, "`#inline`", "header_directive");
            }
            PRECACHE_DIRECTIVE => self.flag(range, "`#precache`", "precache_directive"),
            USING_ANIMTREE_DIRECTIVE => {
                self.flag(range, "`#using_animtree`", "using_animtree_directive")
            }
            PREPROC_DEFINE => {
                self.flag(range, "`#define`", "preprocessor_define");
                if n.children().any(|c| c.kind() == MACRO_PARAMS) {
                    self.flag(
                        range,
                        "a function-like macro",
                        "preprocessor_function_macros",
                    );
                }
            }
            PREPROC_UNDEF => self.flag(range, "`#undef`", "preprocessor_ifdef_undef"),
            DO_WHILE_STMT => self.flag(range, "`do ... while`", "do_while"),
            FOREACH_STMT => self.flag(range, "`foreach`", "foreach"),
            FOREACH_BINDINGS => {
                if n.tokens().any(|t| t.kind() == COMMA) {
                    self.flag(range, "a `foreach` key binding", "foreach_key_value");
                }
            }
            CALL_EXPR | POINTER_CALL_EXPR => {
                for t in n.tokens() {
                    match t.kind() {
                        CHILDTHREAD_KW => self.flag(t.range(), "`childthread`", "childthread"),
                        CALL_KW => self.flag(t.range(), "`call`", "call_pointer_sync"),
                        THREADENDON_KW => self.flag(t.range(), "`threadendon`", "threadendon_call"),
                        BUILTIN_KW => {
                            let g = d.flag("builtin_call");
                            self.report_as(
                                "D006",
                                g,
                                t.range(),
                                "a `builtin` call",
                                "builtin_call",
                            );
                        }
                        _ => {}
                    }
                }
                if n.kind() == POINTER_CALL_EXPR {
                    self.flag(range, "a `[[ ]]` pointer call", "pointer_call");
                }
            }
            WAIT_REAL_TIME_STMT => self.flag(range, "`waitrealtime`", "waitrealtime"),
            WAIT_FRAME_STMT => self.flag(range, "the `waitframe` statement", "waitframe"),
            WAITTILL_FRAME_END_STMT => self.flag(range, "`waittillframeend`", "waittillframeend"),
            CONST_STMT => self.flag(range, "`const`", "const_locals"),
            PROFILER_STMT => {
                if let Some(t) = n.first_token() {
                    let word = t.text().to_ascii_lowercase();
                    let g = d.has("profiler_keywords", &word);
                    self.report(g, t.range(), &format!("`{word}`"), "profiler_keywords");
                }
            }
            BREAKPOINT_STMT => self.flag(range, "`breakpoint`", "breakpoint_keyword"),
            // Unknown keeps the phase-1 behaviour: report, since decompilers write these.
            GOTO_STMT | LABEL_STMT => {
                if d.flag("goto_labels") != Gate::On {
                    self.out.push(SyntaxError::new(
                        "D004",
                        "`goto` and labels are decompiler output, not GSC (feature `goto_labels`)",
                        range.range(),
                    ))
                }
            }
            CAST_EXPR => {
                let g = d.flag("casts");
                self.report_as("D005", g, range, "a `(type)` cast", "casts");
            }
            ASSIGN_EXPR => {
                if n.first_child().is_some_and(|c| c.kind() == ARRAY_EXPR) {
                    self.flag(range, "a tuple assignment", "tuple_assignment");
                }
            }
            QUALIFIED_REF => self.qualified(n),
            FUNCTION_REF => {
                let g = d.is("function_pointer_style", "ampersand");
                self.report(g, range, "a `&` function pointer", "function_pointer_style");
            }
            BIN_EXPR => {
                for t in n.tokens() {
                    match t.kind() {
                        EQ3 | NEQ2 => {
                            self.flag(t.range(), &format!("`{}`", t.text()), "strict_equality")
                        }
                        QUESTION2 => self.flag(t.range(), "`??`", "null_coalescing"),
                        _ => {}
                    }
                }
            }
            FIELD_EXPR => {
                if let Some(t) = n.token(QUESTION_DOT) {
                    self.flag(t.range(), "`?.`", "optional_chaining");
                }
            }
            IS_EXPR => self.flag(range, "an `is` test", "is_expression"),
            ARRAY_EXPR => {
                let tuple_lhs = n.parent == Some(ASSIGN_EXPR) && n.first_child;
                if n.children().next().is_some() && !tuple_lhs {
                    self.flag(range, "a non-empty array literal", "array_literal");
                }
            }
            STRUCT_EXPR => self.flag(range, "a struct literal", "struct_literal"),
            ANIM_REF => {
                self.flag(range, "a `%` animation reference", "anim_reference");
                if n.token(COLON2).is_some() {
                    self.flag(
                        range,
                        "a tree-qualified animation",
                        "anim_reference_qualified",
                    );
                }
            }
            ANIMTREE_EXPR => self.flag(range, "`#animtree`", "animtree_expression"),
            DOLLAR_NAME => self.flag(range, "a `$` name", "dollar_identifiers"),
            THIS_THREAD_EXPR => self.flag(range, "`thisthread`", "thisthread"),
            // `istrue` is lexed as a keyword only where the profile lists the word; elsewhere it
            // is an ordinary name (a script may define `istrue()`), so only the keyword is gated.
            NAME_REF => {
                if let Some(t) = n.token(ISTRUE_KW) {
                    self.flag(t.range(), "the `istrue` operator", "istrue_operator");
                }
            }
            _ => {}
        }
    }

    fn import_order(&mut self, at: At, what: &str) {
        if self.seen_declaration && self.dialect.flag("imports_before_declarations") == Gate::On {
            self.out.push(SyntaxError::new(
                "D003",
                format!(
                    "{what} must come before the first declaration in {}",
                    self.dialect.id
                ),
                at.range(),
            ));
        }
    }

    fn qualified(&mut self, n: Node) {
        let d = self.dialect;
        let range = At::Node(n);
        match n.children().find(|c| c.kind() == QUALIFIER) {
            Some(q) => {
                let Some(t) = q.first_token() else { return };
                if t.kind() == PATH {
                    self.flag(range, "a path-qualified name", "path_qualified_calls");
                } else if t.text().eq_ignore_ascii_case("sys") {
                    self.flag(
                        range,
                        "the `sys::` qualifier",
                        "builtin_namespace_qualifier",
                    );
                } else if d.flag("path_qualified_calls") != Gate::On {
                    // In include games a one-word qualifier is a one-component script path.
                    self.flag(
                        range,
                        "a namespace-qualified name",
                        "namespace_qualified_calls",
                    );
                }
            }
            None => {
                if n.parent != Some(CALL_EXPR) {
                    let g = d.is("function_pointer_style", "colon");
                    self.report(
                        g,
                        range,
                        "a `::` function pointer",
                        "function_pointer_style",
                    );
                }
            }
        }
    }

    fn token(&mut self, t: Tok) {
        let range = t.range();
        match t.kind() {
            ISTRING => self.flag(range, "a `&\"...\"` localized string", "istring_literal"),
            HASH_STRING => self.flag(range, "a `#\"...\"` hashed string", "hash_string_literal"),
            PREFIXED_STRING => {
                let prefix: String = t.text().chars().take(1).collect();
                let g = self.dialect.has("extra_string_prefixes", &prefix);
                self.report(
                    g,
                    range,
                    &format!("a `{prefix}\"...\"` string"),
                    "extra_string_prefixes",
                );
            }
            DOC_COMMENT => {
                let g = self.dialect.is("doc_comment_style", "at-block");
                self.report(g, range, "a `/@ @/` doc comment", "doc_comment_style");
            }
            PP_IF | PP_ELIF | PP_ELSE | PP_ENDIF => {
                self.flag(range, "a `#if` conditional", "preprocessor_conditionals")
            }
            PP_IFDEF | PP_IFNDEF => {
                self.flag(range, "`#ifdef`/`#ifndef`", "preprocessor_ifdef_undef")
            }
            _ => {}
        }
    }
}

/// Report every construct of `root` that `dialect` lacks. Empty for the superset dialect.
pub fn check_dialect(root: &SyntaxNode, dialect: &Dialect) -> Vec<SyntaxError> {
    if !dialect.is_gating() {
        return Vec::new();
    }
    let mut c = Checker {
        dialect,
        out: Vec::new(),
        seen_declaration: false,
    };
    let green = root.green();
    let mut stack = vec![Node {
        green: &green,
        offset: root.text_range().start(),
        parent: root.parent().map(|p| p.kind()),
        first_child: false,
    }];
    // Pre-order: a node is checked before its children, children in source order.
    while let Some(n) = stack.pop() {
        c.node(n);
        let mut kids = Vec::new();
        for (e, offset) in n.elements() {
            match e {
                NodeOrToken::Token(t) => c.token(Tok { green: t, offset }),
                NodeOrToken::Node(_) => {}
            }
        }
        kids.extend(n.children());
        stack.extend(kids.into_iter().rev());
    }
    c.out.sort_by_key(|e| e.range.start());
    c.out
}
