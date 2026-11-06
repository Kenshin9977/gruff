//! `SyntaxKind`: every token and node kind of the GSC concrete syntax tree.
//!
//! Token kinds come first, then node kinds. The `T![...]` macro names punctuation tokens by their
//! spelling, as in rust-analyzer. `gsc.ungram` spells tokens the same way;
//! the AST generator maps them through [`SyntaxKind::from_grammar_token`].

macro_rules! define_kinds {
    (
        trivia { $($triv:ident),* $(,)? }
        literals { $($lit:ident),* $(,)? }
        punct { $($p_text:literal => $p_kind:ident),* $(,)? }
        directives { $($d_text:literal => $d_kind:ident),* $(,)? }
        keywords { $($k_text:literal => $k_kind:ident),* $(,)? }
        nodes { $($node:ident),* $(,)? }
    ) => {
        /// Token and node kinds. The discriminant is stable within a build only.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        #[allow(non_camel_case_types)]
        pub enum SyntaxKind {
            $($triv,)*
            $($lit,)*
            $($p_kind,)*
            $($d_kind,)*
            $($k_kind,)*
            /// A token the lexer could not classify (an unknown character).
            ERROR_TOKEN,
            /// End of input. Never stored in a tree.
            EOF,
            /// A node the parser opened and then abandoned. Never stored in a tree.
            TOMBSTONE,
            $($node,)*
            #[doc(hidden)]
            __LAST,
        }

        impl SyntaxKind {
            const ALL: &'static [SyntaxKind] = &[
                $(SyntaxKind::$triv,)*
                $(SyntaxKind::$lit,)*
                $(SyntaxKind::$p_kind,)*
                $(SyntaxKind::$d_kind,)*
                $(SyntaxKind::$k_kind,)*
                SyntaxKind::ERROR_TOKEN,
                SyntaxKind::EOF,
                SyntaxKind::TOMBSTONE,
                $(SyntaxKind::$node,)*
            ];

            /// Every keyword spelling with its kind (the superset of all dialects).
            pub const KEYWORDS: &'static [(&'static str, SyntaxKind)] = &[
                $(($k_text, SyntaxKind::$k_kind),)*
            ];

            /// Every directive spelling (`#using`...) with its kind.
            pub const DIRECTIVES: &'static [(&'static str, SyntaxKind)] = &[
                $(($d_text, SyntaxKind::$d_kind),)*
            ];

            /// Index in [`Self::KEYWORDS`] of a lower-case keyword spelling.
            pub(crate) fn keyword_index(word: &str) -> Option<usize> {
                const fn idx(kind: SyntaxKind) -> usize {
                    kind as usize - SyntaxKind::IF_KW as usize
                }
                Some(match word {
                    $($k_text => idx(SyntaxKind::$k_kind),)*
                    _ => return None,
                })
            }

            /// Is this one of the trivia kinds (whitespace, comments, inactive preprocessor text)?
            pub fn is_trivia(self) -> bool {
                matches!(self, $(SyntaxKind::$triv)|*)
            }

            /// Is this a word keyword token?
            pub fn is_keyword(self) -> bool {
                matches!(self, $(SyntaxKind::$k_kind)|*)
            }

            /// Is this a directive token (`#using`, `#define`...)?
            pub fn is_directive(self) -> bool {
                matches!(self, $(SyntaxKind::$d_kind)|*)
            }

            /// Is this a punctuation or operator token?
            pub fn is_punct(self) -> bool {
                matches!(self, $(SyntaxKind::$p_kind)|*)
            }

            /// Is this a node kind (as opposed to a token kind)?
            pub fn is_node(self) -> bool {
                (self as u16) > (SyntaxKind::TOMBSTONE as u16) && self != SyntaxKind::__LAST
            }

            /// The fixed spelling of a punctuation, directive or keyword token.
            pub fn text(self) -> Option<&'static str> {
                match self {
                    $(SyntaxKind::$p_kind => Some($p_text),)*
                    $(SyntaxKind::$d_kind => Some($d_text),)*
                    $(SyntaxKind::$k_kind => Some($k_text),)*
                    _ => None,
                }
            }

            /// The kind of a token written `'spelling'` in `gsc.ungram`.
            pub fn from_grammar_token(spelling: &str) -> Option<SyntaxKind> {
                Some(match spelling {
                    $($p_text => SyntaxKind::$p_kind,)*
                    $($d_text => SyntaxKind::$d_kind,)*
                    $($k_text => SyntaxKind::$k_kind,)*
                    "ident" => SyntaxKind::IDENT,
                    "path" => SyntaxKind::PATH,
                    "int" => SyntaxKind::INT_NUMBER,
                    "float" => SyntaxKind::FLOAT_NUMBER,
                    "string" => SyntaxKind::STRING,
                    "istring" => SyntaxKind::ISTRING,
                    "hash_string" => SyntaxKind::HASH_STRING,
                    "prefixed_string" => SyntaxKind::PREFIXED_STRING,
                    "hash_ident" => SyntaxKind::HASH_IDENT,
                    "macro_body" => SyntaxKind::MACRO_BODY,
                    "encoded_name" => SyntaxKind::ENCODED_NAME,
                    _ => return None,
                })
            }

            /// The Rust variant name, used by the AST generator.
            pub fn name(self) -> &'static str {
                match self {
                    $(SyntaxKind::$triv => stringify!($triv),)*
                    $(SyntaxKind::$lit => stringify!($lit),)*
                    $(SyntaxKind::$p_kind => stringify!($p_kind),)*
                    $(SyntaxKind::$d_kind => stringify!($d_kind),)*
                    $(SyntaxKind::$k_kind => stringify!($k_kind),)*
                    SyntaxKind::ERROR_TOKEN => "ERROR_TOKEN",
                    SyntaxKind::EOF => "EOF",
                    SyntaxKind::TOMBSTONE => "TOMBSTONE",
                    $(SyntaxKind::$node => stringify!($node),)*
                    SyntaxKind::__LAST => "__LAST",
                }
            }
        }

    };
}

define_kinds! {
    trivia {
        WHITESPACE,
        COMMENT,
        BLOCK_COMMENT,
        DOC_COMMENT,
        BOM,
        PP_IF,
        PP_IFDEF,
        PP_IFNDEF,
        PP_ELIF,
        PP_ELSE,
        PP_ENDIF,
        DISABLED_TEXT,
    }
    literals {
        IDENT,
        PATH,
        INT_NUMBER,
        FLOAT_NUMBER,
        STRING,
        ISTRING,
        HASH_STRING,
        PREFIXED_STRING,
        HASH_IDENT,
        MACRO_BODY,
        ENCODED_NAME,
    }
    punct {
        ";" => SEMICOLON,
        "," => COMMA,
        "(" => L_PAREN,
        ")" => R_PAREN,
        "{" => L_CURLY,
        "}" => R_CURLY,
        "[" => L_BRACK,
        "]" => R_BRACK,
        "." => DOT,
        "..." => DOT3,
        ":" => COLON,
        "::" => COLON2,
        "?" => QUESTION,
        "?." => QUESTION_DOT,
        "??" => QUESTION2,
        "->" => ARROW,
        "$" => DOLLAR,
        "/#" => DEV_OPEN,
        "#/" => DEV_CLOSE,
        "=" => EQ,
        "==" => EQ2,
        "===" => EQ3,
        "!=" => NEQ,
        "!==" => NEQ2,
        "<" => LT,
        "<=" => LTEQ,
        ">" => GT,
        ">=" => GTEQ,
        "<<" => SHL,
        ">>" => SHR,
        "+" => PLUS,
        "-" => MINUS,
        "*" => STAR,
        "/" => SLASH,
        "%" => PERCENT,
        "&" => AMP,
        "|" => PIPE,
        "^" => CARET,
        "~" => TILDE,
        "!" => BANG,
        "&&" => AMP2,
        "||" => PIPE2,
        "++" => PLUS2,
        "--" => MINUS2,
        "+=" => PLUSEQ,
        "-=" => MINUSEQ,
        "*=" => STAREQ,
        "/=" => SLASHEQ,
        "%=" => PERCENTEQ,
        "&=" => AMPEQ,
        "|=" => PIPEEQ,
        "^=" => CARETEQ,
        "<<=" => SHLEQ,
        ">>=" => SHREQ,
    }
    directives {
        "#using" => USING_KW,
        "#include" => INCLUDE_KW,
        "#insert" => INSERT_KW,
        "#inline" => INLINE_KW,
        "#namespace" => NAMESPACE_KW,
        "#precache" => PRECACHE_KW,
        "#using_animtree" => USING_ANIMTREE_KW,
        "#animtree" => ANIMTREE_KW,
        "#define" => DEFINE_KW,
        "#undef" => UNDEF_KW,
    }
    keywords {
        "if" => IF_KW,
        "else" => ELSE_KW,
        "while" => WHILE_KW,
        "for" => FOR_KW,
        "foreach" => FOREACH_KW,
        "in" => IN_KW,
        "do" => DO_KW,
        "switch" => SWITCH_KW,
        "case" => CASE_KW,
        "default" => DEFAULT_KW,
        "break" => BREAK_KW,
        "continue" => CONTINUE_KW,
        "return" => RETURN_KW,
        "wait" => WAIT_KW,
        "waitrealtime" => WAITREALTIME_KW,
        "waitframe" => WAITFRAME_KW,
        "waittillframeend" => WAITTILLFRAMEEND_KW,
        "thread" => THREAD_KW,
        "childthread" => CHILDTHREAD_KW,
        "call" => CALL_KW,
        "threadendon" => THREADENDON_KW,
        "builtin" => BUILTIN_KW,
        "function" => FUNCTION_KW,
        "autoexec" => AUTOEXEC_KW,
        "private" => PRIVATE_KW,
        "codecall" => CODECALL_KW,
        "event_handler" => EVENT_HANDLER_KW,
        "class" => CLASS_KW,
        "var" => VAR_KW,
        "const" => CONST_KW,
        "constructor" => CONSTRUCTOR_KW,
        "destructor" => DESTRUCTOR_KW,
        "new" => NEW_KW,
        "true" => TRUE_KW,
        "false" => FALSE_KW,
        "undefined" => UNDEFINED_KW,
        "thisthread" => THISTHREAD_KW,
        "breakpoint" => BREAKPOINT_KW,
        "prof_begin" => PROF_BEGIN_KW,
        "prof_end" => PROF_END_KW,
        "profilestart" => PROFILESTART_KW,
        "profilestop" => PROFILESTOP_KW,
        "is" => IS_KW,
        "not" => NOT_KW,
        // Name-like keywords: they stand where a name stands (callee, value, field).
        "self" => SELF_KW,
        "level" => LEVEL_KW,
        "game" => GAME_KW,
        "anim" => ANIM_KW,
        "world" => WORLD_KW,
        "classes" => CLASSES_KW,
        "isdefined" => ISDEFINED_KW,
        "istrue" => ISTRUE_KW,
        "notify" => NOTIFY_KW,
        "endon" => ENDON_KW,
        "waittill" => WAITTILL_KW,
        "waittillmatch" => WAITTILLMATCH_KW,
        "assert" => ASSERT_KW,
        "assertex" => ASSERTEX_KW,
        "assertmsg" => ASSERTMSG_KW,
        "size" => SIZE_KW,
        "vararg" => VARARG_KW,
    }
    nodes {
        SOURCE_FILE,
        ERROR,
        // directives and preprocessor
        USING_DIRECTIVE,
        INCLUDE_DIRECTIVE,
        INSERT_DIRECTIVE,
        INLINE_DIRECTIVE,
        NAMESPACE_DIRECTIVE,
        PRECACHE_DIRECTIVE,
        USING_ANIMTREE_DIRECTIVE,
        PREPROC_DEFINE,
        MACRO_PARAMS,
        PREPROC_UNDEF,
        MACRO_CALL_ITEM,
        // declarations
        FUNCTION_DEF,
        EVENT_HANDLER_MODIFIER,
        PARAM_LIST,
        PARAM,
        DEFAULT_VALUE,
        VARARG_PARAM,
        CLASS_DEF,
        CLASS_BASE,
        CLASS_VAR,
        CONSTRUCTOR,
        DESTRUCTOR,
        FILE_CONSTANT,
        DEV_BLOCK_ITEM,
        // statements
        BLOCK,
        EXPR_STMT,
        IF_STMT,
        ELSE_BRANCH,
        WHILE_STMT,
        DO_WHILE_STMT,
        FOR_STMT,
        FOR_INIT,
        FOR_CONDITION,
        FOR_STEP,
        FOREACH_STMT,
        FOREACH_BINDINGS,
        SWITCH_STMT,
        SWITCH_CLAUSE,
        BREAK_STMT,
        CONTINUE_STMT,
        RETURN_STMT,
        WAIT_STMT,
        WAIT_REAL_TIME_STMT,
        WAIT_FRAME_STMT,
        WAITTILL_FRAME_END_STMT,
        CONST_STMT,
        PROFILER_STMT,
        BREAKPOINT_STMT,
        DEV_BLOCK_STMT,
        MACRO_STMT,
        GOTO_STMT,
        LABEL_STMT,
        EMPTY_STMT,
        // expressions
        NAME,
        NAME_REF,
        LITERAL,
        QUALIFIED_REF,
        QUALIFIER,
        FUNCTION_REF,
        CALL_EXPR,
        POINTER_CALL_EXPR,
        METHOD_CALL_EXPR,
        ARG_LIST,
        FIELD_EXPR,
        INDEX_EXPR,
        PREFIX_EXPR,
        POSTFIX_EXPR,
        BIN_EXPR,
        ASSIGN_EXPR,
        TERNARY_EXPR,
        IS_EXPR,
        PAREN_EXPR,
        VECTOR_EXPR,
        ARRAY_EXPR,
        ARRAY_ELEMENT,
        STRUCT_EXPR,
        STRUCT_FIELD,
        NEW_EXPR,
        ANIM_REF,
        ANIMTREE_EXPR,
        DOLLAR_NAME,
        THIS_THREAD_EXPR,
        CAST_EXPR,
    }
}

/// `T![;]`, `T![::]`, `T!['(']`: the kind of a punctuation token.
#[macro_export]
macro_rules! T {
    [;] => { $crate::SyntaxKind::SEMICOLON };
    [,] => { $crate::SyntaxKind::COMMA };
    ['('] => { $crate::SyntaxKind::L_PAREN };
    [')'] => { $crate::SyntaxKind::R_PAREN };
    ['{'] => { $crate::SyntaxKind::L_CURLY };
    ['}'] => { $crate::SyntaxKind::R_CURLY };
    ['['] => { $crate::SyntaxKind::L_BRACK };
    [']'] => { $crate::SyntaxKind::R_BRACK };
    [.] => { $crate::SyntaxKind::DOT };
    [...] => { $crate::SyntaxKind::DOT3 };
    [:] => { $crate::SyntaxKind::COLON };
    [::] => { $crate::SyntaxKind::COLON2 };
    [?] => { $crate::SyntaxKind::QUESTION };
    [?.] => { $crate::SyntaxKind::QUESTION_DOT };
    [??] => { $crate::SyntaxKind::QUESTION2 };
    [->] => { $crate::SyntaxKind::ARROW };
    [/#] => { $crate::SyntaxKind::DEV_OPEN };
    [#/] => { $crate::SyntaxKind::DEV_CLOSE };
    [=] => { $crate::SyntaxKind::EQ };
    [==] => { $crate::SyntaxKind::EQ2 };
    [===] => { $crate::SyntaxKind::EQ3 };
    [!=] => { $crate::SyntaxKind::NEQ };
    [!==] => { $crate::SyntaxKind::NEQ2 };
    [<] => { $crate::SyntaxKind::LT };
    [<=] => { $crate::SyntaxKind::LTEQ };
    [>] => { $crate::SyntaxKind::GT };
    [>=] => { $crate::SyntaxKind::GTEQ };
    [<<] => { $crate::SyntaxKind::SHL };
    [>>] => { $crate::SyntaxKind::SHR };
    [+] => { $crate::SyntaxKind::PLUS };
    [-] => { $crate::SyntaxKind::MINUS };
    [*] => { $crate::SyntaxKind::STAR };
    [/] => { $crate::SyntaxKind::SLASH };
    [%] => { $crate::SyntaxKind::PERCENT };
    [&] => { $crate::SyntaxKind::AMP };
    [|] => { $crate::SyntaxKind::PIPE };
    [^] => { $crate::SyntaxKind::CARET };
    [~] => { $crate::SyntaxKind::TILDE };
    [!] => { $crate::SyntaxKind::BANG };
    [&&] => { $crate::SyntaxKind::AMP2 };
    [||] => { $crate::SyntaxKind::PIPE2 };
    [++] => { $crate::SyntaxKind::PLUS2 };
    [--] => { $crate::SyntaxKind::MINUS2 };
}

impl SyntaxKind {
    /// Keywords that stand where a name stands: global objects, call keywords (`notify`,
    /// `waittill`, `isdefined`...), `size` and `vararg`.
    pub fn is_name_like_keyword(self) -> bool {
        use SyntaxKind::*;
        matches!(
            self,
            SELF_KW
                | LEVEL_KW
                | GAME_KW
                | ANIM_KW
                | WORLD_KW
                | CLASSES_KW
                | ISDEFINED_KW
                | ISTRUE_KW
                | NOTIFY_KW
                | ENDON_KW
                | WAITTILL_KW
                | WAITTILLMATCH_KW
                | ASSERT_KW
                | ASSERTEX_KW
                | ASSERTMSG_KW
                | SIZE_KW
                | VARARG_KW
        )
    }

    /// Kinds of literal tokens.
    pub fn is_literal(self) -> bool {
        use SyntaxKind::*;
        matches!(
            self,
            INT_NUMBER
                | FLOAT_NUMBER
                | STRING
                | ISTRING
                | HASH_STRING
                | PREFIXED_STRING
                | HASH_IDENT
                | TRUE_KW
                | FALSE_KW
                | UNDEFINED_KW
        )
    }

    /// Every kind, in discriminant order.
    pub fn all() -> &'static [SyntaxKind] {
        Self::ALL
    }

    pub(crate) fn from_raw(raw: u16) -> SyntaxKind {
        Self::ALL
            .get(raw as usize)
            .copied()
            .unwrap_or(SyntaxKind::ERROR)
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        rowan::SyntaxKind(kind as u16)
    }
}

/// The rowan language tag of the GSC tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GscLanguage {}

impl rowan::Language for GscLanguage {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from_raw(raw.0)
    }
    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// A node of the untyped GSC tree.
pub type SyntaxNode = rowan::SyntaxNode<GscLanguage>;
/// A token of the untyped GSC tree.
pub type SyntaxToken = rowan::SyntaxToken<GscLanguage>;
/// A node or a token.
pub type SyntaxElement = rowan::SyntaxElement<GscLanguage>;
/// Iterator over the children of a node.
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<GscLanguage>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_match_the_table() {
        for (i, k) in SyntaxKind::all().iter().enumerate() {
            assert_eq!(*k as usize, i, "{k:?}");
            assert_eq!(SyntaxKind::from_raw(i as u16), *k);
        }
    }

    #[test]
    fn keyword_index_matches_the_table() {
        for (i, (text, _)) in SyntaxKind::KEYWORDS.iter().enumerate() {
            assert_eq!(SyntaxKind::keyword_index(text), Some(i), "{text}");
        }
        assert_eq!(SyntaxKind::keyword_index("goto"), None);
    }

    #[test]
    fn grammar_token_names() {
        assert_eq!(
            SyntaxKind::from_grammar_token("::"),
            Some(SyntaxKind::COLON2)
        );
        assert_eq!(
            SyntaxKind::from_grammar_token("#using"),
            Some(SyntaxKind::USING_KW)
        );
        assert_eq!(
            SyntaxKind::from_grammar_token("thread"),
            Some(SyntaxKind::THREAD_KW)
        );
        assert_eq!(
            SyntaxKind::from_grammar_token("ident"),
            Some(SyntaxKind::IDENT)
        );
        assert_eq!(T![;], SyntaxKind::SEMICOLON);
        assert_eq!(T![/#], SyntaxKind::DEV_OPEN);
    }
}
