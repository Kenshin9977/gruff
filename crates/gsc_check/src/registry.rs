//! The rule table: codes, readable names, tiers and documentation.
//!
//! Codes are a category letter plus digits, as in Ruff: `E` syntax, `D` dialect, `B` builtins,
//! `F` fields, `I` imports, `L` loops and control flow, `M` macros, `N` names and declarations,
//! `S` suppressions, `T` threads and events, `U` local variables, `V` values. See
//! `docs/rules/README.md`.

/// Whether a rule can offer fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixAvailability {
    /// The rule never offers a fix.
    None,
    /// Some diagnostics carry a fix (see each fix's [`crate::Applicability`]).
    Sometimes,
}

/// One rule.
#[derive(Debug)]
pub struct Rule {
    /// `B001`.
    pub code: &'static str,
    /// `unknown-builtin`.
    pub name: &'static str,
    /// One line for `--statistics`, SARIF and the rule list.
    pub summary: &'static str,
    /// Preview rules are off unless `--preview` (or `preview = true`) is given.
    pub preview: bool,
    /// Rules that report nothing yet (waiting for data).
    pub pending: bool,
    pub fix: FixAvailability,
    /// The Markdown page under `docs/rules/`.
    pub doc: &'static str,
}

macro_rules! rule {
    ($code:literal, $name:literal, $summary:literal $(, $flag:ident)*) => {
        Rule {
            code: $code,
            name: $name,
            summary: $summary,
            preview: rule!(@has preview $($flag)*),
            pending: rule!(@has pending $($flag)*),
            fix: if rule!(@has fix $($flag)*) {
                FixAvailability::Sometimes
            } else {
                FixAvailability::None
            },
            doc: include_str!(concat!("../../../docs/rules/", $code, ".md")),
        }
    };
    (@has $want:ident) => { false };
    (@has preview preview $($rest:ident)*) => { true };
    (@has pending pending $($rest:ident)*) => { true };
    (@has fix fix $($rest:ident)*) => { true };
    (@has $want:ident $other:ident $($rest:ident)*) => { rule!(@has $want $($rest)*) };
}

/// Every rule, sorted by code.
pub static RULES: &[Rule] = &[
    rule!(
        "B001",
        "unknown-builtin",
        "Call to a name that is neither a builtin nor a script function",
        fix
    ),
    rule!(
        "B002",
        "builtin-arg-count",
        "Builtin called with the wrong number of arguments"
    ),
    rule!(
        "B003",
        "dev-only-builtin",
        "Dev-only builtin called outside a `/# #/` dev block",
        preview
    ),
    rule!(
        "B004",
        "bare-call-other-namespace",
        "Bare call or `&f` that only a function of another namespace defines",
        fix
    ),
    rule!(
        "B005",
        "thread-builtin",
        "`thread` on a builtin, or `::f` of one in `file-first` and `builtin-strict` games"
    ),
    rule!(
        "D001",
        "unavailable-feature",
        "Syntax the selected game does not have"
    ),
    rule!(
        "D002",
        "missing-function-keyword",
        "Function declared without the `function` keyword"
    ),
    rule!(
        "D003",
        "import-after-declaration",
        "`#using` or `#insert` after the first declaration"
    ),
    rule!(
        "D004",
        "decompiler-control-flow",
        "`goto` or a label, which only decompilers write"
    ),
    rule!(
        "D005",
        "unavailable-cast",
        "A `(type)` cast the selected game does not have"
    ),
    rule!(
        "D006",
        "unavailable-builtin-call",
        "A `builtin` call the selected game does not have"
    ),
    rule!(
        "E0101",
        "unterminated-string",
        "String literal without its closing quote"
    ),
    rule!(
        "E0102",
        "unexpected-character",
        "Character that starts no token"
    ),
    rule!(
        "E0103",
        "unterminated-block-comment",
        "Block comment without its closing `*/`"
    ),
    rule!(
        "E0104",
        "unbalanced-conditional",
        "`#if` without `#endif`, or the reverse"
    ),
    rule!(
        "E0201",
        "expected-token",
        "A token the grammar requires is missing"
    ),
    rule!(
        "E0202",
        "unexpected-token",
        "A token that cannot appear here"
    ),
    rule!(
        "E0203",
        "nesting-too-deep",
        "Statements or expressions nest too deeply to parse"
    ),
    rule!(
        "E0204",
        "vector-arity",
        "Vector literal without exactly three components"
    ),
    rule!(
        "E0299",
        "parser-stuck",
        "The parser made no progress (a parser bug)"
    ),
    rule!(
        "E0301",
        "decompiler-name",
        "Decompiler name no tokenizer can read as one name"
    ),
    rule!(
        "F001",
        "unknown-field",
        "Field read that nothing sets, one typo away from one that is set",
        preview,
        fix
    ),
    rule!(
        "I001",
        "duplicate-import",
        "The same `#using` or `#include` twice",
        preview,
        fix
    ),
    rule!(
        "I002",
        "unused-import",
        "`#using` or `#include` whose file supplies nothing the script uses",
        preview,
        fix
    ),
    rule!(
        "I003",
        "unresolved-import",
        "`#using`, `#include` or `#insert` of a file the workspace does not have",
        preview
    ),
    rule!(
        "L001",
        "infinite-loop-without-wait",
        "Infinite loop whose body never waits",
        fix
    ),
    rule!(
        "L002",
        "unreachable-code",
        "Statements after a `return`, `break` or `continue`",
        preview
    ),
    rule!(
        "L003",
        "duplicate-case-label",
        "A `case` value or `default` a switch already has",
        preview
    ),
    rule!(
        "L004",
        "assignment-as-value",
        "An assignment used as a value, as in `if ( x = 1 )`",
        preview
    ),
    rule!(
        "L005",
        "non-constant-case-label",
        "A `case` label that is not a constant string or integer"
    ),
    rule!(
        "M001",
        "macro-arg-count",
        "Function-like macro called with the wrong number of arguments"
    ),
    rule!(
        "M002",
        "macro-redefined",
        "`#define` of a macro already defined",
        preview
    ),
    rule!(
        "M003",
        "duplicate-macro-parameter",
        "`#define` with a parameter name repeated"
    ),
    rule!(
        "N001",
        "unresolved-script-function",
        "`ns::f()`, `path::f()` or `::f()` naming no script function",
        preview
    ),
    rule!(
        "N002",
        "namespace-not-imported",
        "`ns::f()` with no `#using` of a file declaring `ns`",
        preview,
        fix
    ),
    rule!(
        "N003",
        "private-function-call",
        "Call to a `private` function of another namespace",
        preview
    ),
    rule!(
        "N004",
        "script-arg-count",
        "Script function called with more arguments than it declares",
        preview
    ),
    rule!(
        "N005",
        "duplicate-function",
        "Function defined twice in one file"
    ),
    rule!(
        "N006",
        "duplicate-parameter",
        "Parameter name repeated in one parameter list",
        preview
    ),
    rule!(
        "N007",
        "misplaced-vararg",
        "`...` that is not the last parameter"
    ),
    rule!(
        "N008",
        "inheritance-cycle",
        "Class that inherits from itself"
    ),
    rule!(
        "N009",
        "constructor-parameters",
        "Constructor or destructor with parameters"
    ),
    rule!(
        "S100",
        "unused-suppression",
        "Suppression comment that suppresses nothing",
        fix
    ),
    rule!(
        "T001",
        "unknown-event",
        "`waittill` or `endon` on an event no script and not the engine notifies",
        preview
    ),
    rule!(
        "T002",
        "misspelled-event",
        "`waittill` or `endon` on an event nothing notifies, one typo away from one that is",
        preview
    ),
    rule!(
        "T003",
        "waittill-arg-count",
        "`waittill` binds (or its BO4+ result is read for) values no notify of the event sends",
        preview
    ),
    rule!(
        "T004",
        "missing-endon",
        "Function threaded on a player that loops forever with no `endon`",
        preview
    ),
    rule!(
        "U001",
        "unused-variable",
        "Local variable assigned and never read",
        preview
    ),
    rule!(
        "U002",
        "uninitialized-variable",
        "Local variable read that nothing in its function assigns",
        preview
    ),
    rule!(
        "V001",
        "hash-equality",
        "`==` on a hash, a script error in games that do not compare hashes by value",
        preview
    ),
    rule!(
        "V002",
        "thread-call-value",
        "The value of a `thread` call used, in games where it is the callee's result",
        preview
    ),
    rule!(
        "V003",
        "read-only-assignment",
        "Assignment to `.size`, an engine global or a `const`"
    ),
    rule!(
        "V004",
        "division-by-zero",
        "Division or modulo by a literal zero",
        preview
    ),
    rule!(
        "V005",
        "useless-expression",
        "Expression statement with no effect, as in `a == b;`"
    ),
];

/// The rule with this exact code (any case) or name.
pub fn rule(code_or_name: &str) -> Option<&'static Rule> {
    RULES
        .iter()
        .find(|r| r.code.eq_ignore_ascii_case(code_or_name) || r.name == code_or_name)
}

/// The readable name of a code (`syntax-error` and `dialect` for codes `gsc_syntax` adds later).
pub fn name_of(code: &str) -> &'static str {
    match rule(code) {
        Some(r) => r.name,
        None if code.starts_with('E') => "syntax-error",
        None if code.starts_with('D') => "dialect",
        None => "unknown",
    }
}

/// Is `code` a preview rule?
pub fn is_preview(code: &str) -> bool {
    rule(code).is_some_and(|r| r.preview)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        for w in RULES.windows(2) {
            assert!(w[0].code < w[1].code, "{} then {}", w[0].code, w[1].code);
        }
        let mut names: Vec<_> = RULES.iter().map(|r| r.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), RULES.len());
    }

    #[test]
    fn every_rule_has_a_doc_page() {
        for r in RULES {
            assert!(
                r.doc.starts_with(&format!("# {} {}", r.code, r.name)),
                "docs/rules/{}.md must start with `# {} {}`",
                r.code,
                r.code,
                r.name
            );
        }
    }

    #[test]
    fn lookup() {
        assert_eq!(rule("b001").unwrap().name, "unknown-builtin");
        assert_eq!(rule("unknown-builtin").unwrap().code, "B001");
        assert_eq!(name_of("E0999"), "syntax-error");
    }
}
