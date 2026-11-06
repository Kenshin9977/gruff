//! Grammar functions: items, statements and a Pratt core for expressions.
//!
//! The parser accepts the superset of every dialect. Words that are keywords only in some games
//! are recognised contextually where the identifier reading would be a syntax error, and are
//! remapped to their keyword kind so that the dialect check can report them.

use crate::parser::{MarkClosed, Parser, MAX_DEPTH};
use crate::SyntaxKind::{self, *};
use crate::T;

// ===== Token sets ===============================================================================

/// Tokens that only start a declaration; a statement list stops at them (missing `}`).
const ITEM_ONLY: &[SyntaxKind] = &[
    FUNCTION_KW,
    CLASS_KW,
    USING_KW,
    INCLUDE_KW,
    INSERT_KW,
    INLINE_KW,
    NAMESPACE_KW,
    PRECACHE_KW,
    USING_ANIMTREE_KW,
    AUTOEXEC_KW,
    PRIVATE_KW,
    CODECALL_KW,
    EVENT_HANDLER_KW,
];

/// Where file-scope recovery resumes.
const ITEM_RECOVERY: &[SyntaxKind] = &[
    FUNCTION_KW,
    CLASS_KW,
    USING_KW,
    INCLUDE_KW,
    INSERT_KW,
    INLINE_KW,
    NAMESPACE_KW,
    PRECACHE_KW,
    USING_ANIMTREE_KW,
    DEFINE_KW,
    UNDEF_KW,
    AUTOEXEC_KW,
    PRIVATE_KW,
    CODECALL_KW,
    DEV_OPEN,
    DEV_CLOSE,
];

/// Does a declaration (which ends an unclosed statement list) start here? A declaration keyword
/// counts only in declaration shape, so `class = f();` stays a statement in games without classes.
fn at_item_only(p: &Parser) -> bool {
    let k = p.current();
    if !ITEM_ONLY.contains(&k) {
        return false;
    }
    let next = p.nth(1);
    match k {
        CLASS_KW => is_name_like(next),
        FUNCTION_KW | AUTOEXEC_KW | PRIVATE_KW | CODECALL_KW => {
            is_name_like(next)
                || matches!(next, FUNCTION_KW | AUTOEXEC_KW | PRIVATE_KW | CODECALL_KW)
        }
        EVENT_HANDLER_KW => next == T!['['],
        _ => true,
    }
}

/// Tokens that may follow a name: a keyword before one of them is used as an identifier.
const NAME_FOLLOW: &[SyntaxKind] = &[
    EQ, PLUSEQ, MINUSEQ, STAREQ, SLASHEQ, PERCENTEQ, AMPEQ, PIPEEQ, CARETEQ, SHLEQ, SHREQ, DOT,
    SEMICOLON, R_PAREN, R_BRACK, COMMA, COLON2, PLUS2, MINUS2, EQ2, NEQ, EQ3, NEQ2, LT, LTEQ, GT,
    GTEQ, PLUS, MINUS, STAR, SLASH, PERCENT, AMP2, PIPE2, QUESTION, L_BRACK, L_PAREN,
];

/// A keyword used as a plain name, e.g. `class = f();` in a game whose profile lists `class`.
fn at_keyword_as_name(p: &Parser) -> bool {
    let k = p.current();
    let next = p.nth(1);
    if !k.is_keyword() || k.is_name_like_keyword() || k.is_literal() {
        return false;
    }
    match k {
        // A value keyword stays the value (`x = thisthread;`) unless called or qualified.
        THISTHREAD_KW => matches!(next, L_PAREN | COLON2),
        // Declaration keywords are contextual: `function thread f();` calls on a variable
        // named `function` (jup), which cannot start a declaration.
        FUNCTION_KW | CLASS_KW => NAME_FOLLOW.contains(&next) || CALL_MODIFIERS.contains(&next),
        _ => NAME_FOLLOW.contains(&next),
    }
}

const ASSIGN_OPS: &[SyntaxKind] = &[
    EQ, PLUSEQ, MINUSEQ, STAREQ, SLASHEQ, PERCENTEQ, AMPEQ, PIPEEQ, CARETEQ, SHLEQ, SHREQ,
];

const CALL_MODIFIERS: &[SyntaxKind] = &[
    THREAD_KW,
    CHILDTHREAD_KW,
    CALL_KW,
    THREADENDON_KW,
    BUILTIN_KW,
];

const PREFIX_BP: u8 = 27;
const IS_BP: u8 = 17;

fn infix_bp(kind: SyntaxKind) -> Option<(u8, u8, SyntaxKind)> {
    let (l, r) = match kind {
        k if ASSIGN_OPS.contains(&k) => return Some((2, 1, ASSIGN_EXPR)),
        QUESTION => return Some((4, 3, TERNARY_EXPR)),
        QUESTION2 => (6, 5),
        PIPE2 => (7, 8),
        AMP2 => (9, 10),
        PIPE => (11, 12),
        CARET => (13, 14),
        AMP => (15, 16),
        EQ2 | NEQ | EQ3 | NEQ2 => (17, 18),
        LT | LTEQ | GT | GTEQ => (19, 20),
        SHL | SHR => (21, 22),
        PLUS | MINUS => (23, 24),
        STAR | SLASH | PERCENT => (25, 26),
        _ => return None,
    };
    Some((l, r, BIN_EXPR))
}

/// Can this token stand where a name stands (identifier or name-like keyword)?
fn is_name_like(kind: SyntaxKind) -> bool {
    kind == IDENT || kind.is_name_like_keyword()
}

/// Can this token name a field after `.` or a function after `::`? Any word goes.
fn is_any_word(kind: SyntaxKind) -> bool {
    kind == IDENT || kind.is_keyword()
}

// ===== File =====================================================================================

pub(crate) fn source_file(p: &mut Parser) {
    let m = p.open();
    loop {
        while !p.at(EOF) {
            item(p);
        }
        if p.at_real_eof() {
            break;
        }
        // The fuel ran out: report, skip one token and go on.
        p.unstick();
        p.advance_with_error("E0299", "the parser made no progress here");
    }
    p.close(m, SOURCE_FILE);
}

fn item(p: &mut Parser) {
    match p.current() {
        USING_KW => path_directive(p, USING_DIRECTIVE),
        INCLUDE_KW => path_directive(p, INCLUDE_DIRECTIVE),
        INSERT_KW => path_directive(p, INSERT_DIRECTIVE),
        INLINE_KW => path_directive(p, INLINE_DIRECTIVE),
        NAMESPACE_KW => {
            let m = p.open();
            p.bump();
            name(p);
            p.expect(T![;]);
            p.close(m, NAMESPACE_DIRECTIVE);
        }
        PRECACHE_KW => call_directive(p, PRECACHE_DIRECTIVE),
        USING_ANIMTREE_KW => call_directive(p, USING_ANIMTREE_DIRECTIVE),
        DEFINE_KW => define(p),
        UNDEF_KW => undef(p),
        DEV_OPEN => dev_block_item(p),
        FUNCTION_KW | AUTOEXEC_KW | PRIVATE_KW | CODECALL_KW | EVENT_HANDLER_KW => function_def(p),
        CLASS_KW => class_def(p),
        IDENT => ident_item(p),
        DEV_CLOSE if p.dev_depth > 0 => {
            // Handled by the enclosing dev block.
            p.advance_with_error("E0202", "unexpected `#/`");
        }
        _ => {
            let found = p.found();
            p.skip_until(
                ITEM_RECOVERY,
                "E0202",
                format!("expected a declaration, found {found}"),
            );
        }
    }
}

fn path_directive(p: &mut Parser, kind: SyntaxKind) {
    let m = p.open();
    p.bump();
    if p.at(PATH) || p.at(STRING) || (is_any_word(p.current()) && !p.newline_before(0)) {
        p.bump();
    } else {
        p.error_expected("a script path");
    }
    p.expect(T![;]);
    p.close(m, kind);
}

fn call_directive(p: &mut Parser, kind: SyntaxKind) {
    let m = p.open();
    p.bump();
    if p.at(T!['(']) {
        arg_list(p);
    } else {
        p.error_expected("`(`");
    }
    p.expect(T![;]);
    p.close(m, kind);
}

fn define(p: &mut Parser) {
    let m = p.open();
    p.bump();
    if p.at(IDENT) && !p.newline_before(0) {
        let joint = p.joint(0);
        name(p);
        if joint && p.at(T!['(']) && !p.newline_before(0) {
            let params = p.open();
            p.bump();
            while !p.at(EOF) && !p.at(T![')']) && !p.newline_before(0) {
                if p.at(IDENT) || p.at(T![,]) || p.at(T![...]) {
                    p.bump();
                } else {
                    p.advance_with_error("E0201", "expected a macro parameter");
                }
            }
            if !p.newline_before(0) {
                p.expect(T![')']);
            }
            p.close(params, MACRO_PARAMS);
        }
        if p.at(MACRO_BODY) {
            p.bump();
        }
    } else {
        p.error_expected("a macro name");
    }
    p.close(m, PREPROC_DEFINE);
}

fn undef(p: &mut Parser) {
    let m = p.open();
    p.bump();
    if is_any_word(p.current()) && !p.newline_before(0) {
        name(p);
    } else {
        p.error_expected("a macro name");
    }
    p.close(m, PREPROC_UNDEF);
}

fn dev_block_item(p: &mut Parser) {
    let m = p.open();
    p.bump();
    p.dev_depth += 1;
    while !p.at(EOF) && !p.at(DEV_CLOSE) {
        item(p);
    }
    p.dev_depth -= 1;
    // Shipped files may leave a file-scope dev block open until the end of the file.
    if !p.at_real_eof() {
        p.expect(DEV_CLOSE);
    }
    p.close(m, DEV_BLOCK_ITEM);
}

/// Index of the token after the `)` matching the `(` at absolute index `open`.
fn after_parens(p: &Parser, open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    loop {
        match p.kind_at(i) {
            L_PAREN => depth += 1,
            R_PAREN => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            L_CURLY | R_CURLY | SEMICOLON | EOF => return i,
            _ => {}
        }
        i += 1;
    }
}

/// A file-scope item that starts with an identifier.
fn ident_item(p: &mut Parser) {
    // Contextual keywords of games whose profile lacks them.
    if p.at_contextual(0, "function") && is_name_like(p.nth(1)) {
        return function_def(p);
    }
    if (p.at_contextual(0, "autoexec")
        || p.at_contextual(0, "private")
        || p.at_contextual(0, "codecall"))
        && (is_name_like(p.nth(1)) || p.nth(1) == FUNCTION_KW)
        && p.nth(2) != T![=]
    {
        return function_def(p);
    }
    if p.at_contextual(0, "class") && p.nth(1) == IDENT && matches!(p.nth(2), L_CURLY | COLON) {
        return class_def(p);
    }
    match p.nth(1) {
        L_PAREN => {
            let after = after_parens(p, p.position() + 1);
            if p.kind_at(after) == L_CURLY {
                function_def(p);
            } else {
                // A macro invoked at file scope, such as a registration macro.
                let m = p.open();
                let callee = p.open();
                p.bump();
                p.close(callee, NAME_REF);
                arg_list(p);
                p.eat(T![;]);
                p.close(m, MACRO_CALL_ITEM);
            }
        }
        EQ => {
            let m = p.open();
            name(p);
            p.bump();
            expr(p);
            p.expect(T![;]);
            p.close(m, FILE_CONSTANT);
        }
        _ => {
            let found = p.found();
            p.skip_until(
                ITEM_RECOVERY,
                "E0202",
                format!("expected a declaration, found {found}"),
            );
        }
    }
}

fn name(p: &mut Parser) {
    if p.at(BANG) && p.joint(0) && p.nth(1) == IDENT {
        // Decompiler output: a hash that resolved to a string starting with `!`.
        let m = p.open();
        p.bump();
        p.bump();
        p.close(m, NAME);
    } else if is_name_like(p.current()) || p.at(PATH) {
        let m = p.open();
        p.bump();
        p.close(m, NAME);
    } else if p.current().is_keyword() && !p.newline_before(0) {
        // A keyword used as a name, e.g. a method called like a keyword of another game.
        let m = p.open();
        p.bump_remap(IDENT);
        p.close(m, NAME);
    } else {
        p.error_expected("a name");
    }
}

fn function_def(p: &mut Parser) {
    let m = p.open();
    if p.at(FUNCTION_KW) {
        p.bump();
    } else if p.at_contextual(0, "function") {
        p.bump_remap(FUNCTION_KW);
    }
    loop {
        match p.current() {
            AUTOEXEC_KW | PRIVATE_KW | CODECALL_KW => p.bump(),
            EVENT_HANDLER_KW => event_handler(p),
            FUNCTION_KW => p.bump(),
            IDENT if is_name_like(p.nth(1)) || p.nth(1) == FUNCTION_KW => {
                if p.at_contextual(0, "autoexec") {
                    p.bump_remap(AUTOEXEC_KW)
                } else if p.at_contextual(0, "private") {
                    p.bump_remap(PRIVATE_KW)
                } else if p.at_contextual(0, "codecall") {
                    p.bump_remap(CODECALL_KW)
                } else if p.at_contextual(0, "function") {
                    p.bump_remap(FUNCTION_KW)
                } else {
                    break;
                }
            }
            IDENT if p.at_contextual(0, "event_handler") && p.nth(1) == T!['['] => event_handler(p),
            _ => break,
        }
    }
    name(p);
    if p.at(T!['(']) {
        param_list(p);
    } else {
        p.error_expected("`(`");
    }
    if p.at(T!['{']) {
        block(p);
    } else {
        p.error_expected("a function body");
    }
    p.close(m, FUNCTION_DEF);
}

fn event_handler(p: &mut Parser) {
    let m = p.open();
    if p.at(EVENT_HANDLER_KW) {
        p.bump();
    } else {
        p.bump_remap(EVENT_HANDLER_KW);
    }
    if p.expect(T!['[']) {
        name(p);
        p.expect(T![']']);
    }
    p.close(m, EVENT_HANDLER_MODIFIER);
}

fn param_list(p: &mut Parser) {
    let m = p.open();
    p.bump();
    while !p.at(EOF) && !p.at(T![')']) && !p.at(T!['{']) {
        param(p);
        if !p.at(T![')']) && !p.eat(T![,]) {
            p.error_expected("`,` or `)`");
            if !is_name_like(p.current()) && !p.at_any(&[AMP, STAR, DOT3]) {
                break;
            }
        }
    }
    p.expect(T![')']);
    p.close(m, PARAM_LIST);
}

fn param(p: &mut Parser) {
    if p.at(T![...]) {
        let m = p.open();
        p.bump();
        p.close(m, VARARG_PARAM);
        return;
    }
    let m = p.open();
    if p.at(AMP) || p.at(STAR) {
        p.bump();
    }
    if is_name_like(p.current()) || p.current().is_keyword() || p.at(PATH) {
        name(p);
    } else {
        p.error_expected("a parameter name");
        if !p.at_any(&[R_PAREN, COMMA, L_CURLY, EOF]) {
            p.bump();
        }
    }
    if p.at(T![=]) {
        let d = p.open();
        p.bump();
        expr(p);
        p.close(d, DEFAULT_VALUE);
    }
    p.close(m, PARAM);
}

fn class_def(p: &mut Parser) {
    let m = p.open();
    if p.at(CLASS_KW) {
        p.bump();
    } else {
        p.bump_remap(CLASS_KW);
    }
    name(p);
    if p.at(T![:]) {
        let b = p.open();
        p.bump();
        name_ref(p);
        while p.eat(T![,]) {
            name_ref(p);
        }
        p.close(b, CLASS_BASE);
    }
    if p.expect(T!['{']) {
        while !p.at(EOF) && !p.at(T!['}']) {
            class_member(p);
        }
        p.expect(T!['}']);
    }
    p.close(m, CLASS_DEF);
}

fn ctor_dtor(p: &mut Parser) {
    let ctor = p.at(CONSTRUCTOR_KW) || p.at_contextual(0, "constructor");
    let m = p.open();
    if p.at(IDENT) {
        p.bump_remap(if ctor { CONSTRUCTOR_KW } else { DESTRUCTOR_KW });
    } else {
        p.bump();
    }
    if p.at(T!['(']) {
        param_list(p);
    } else {
        p.error_expected("`(`");
    }
    if p.at(T!['{']) {
        block(p);
    } else {
        p.error_expected("a body");
    }
    p.close(m, if ctor { CONSTRUCTOR } else { DESTRUCTOR });
}

fn class_member(p: &mut Parser) {
    match p.current() {
        VAR_KW => {
            let m = p.open();
            p.bump();
            name(p);
            if p.at(T![=]) {
                p.bump();
                expr(p);
            }
            p.expect(T![;]);
            p.close(m, CLASS_VAR);
        }
        CONSTRUCTOR_KW | DESTRUCTOR_KW => ctor_dtor(p),
        IDENT
            if (p.at_contextual(0, "constructor") || p.at_contextual(0, "destructor"))
                && p.nth(1) == T!['('] =>
        {
            ctor_dtor(p)
        }
        IDENT if p.at_contextual(0, "var") && is_name_like(p.nth(1)) => {
            let m = p.open();
            p.bump_remap(VAR_KW);
            name(p);
            if p.at(T![=]) {
                p.bump();
                expr(p);
            }
            p.expect(T![;]);
            p.close(m, CLASS_VAR);
        }
        FUNCTION_KW | AUTOEXEC_KW | PRIVATE_KW | CODECALL_KW | EVENT_HANDLER_KW | DEFINE_KW
        | UNDEF_KW | DEV_OPEN | IDENT => item(p),
        _ => {
            let found = p.found();
            let mut recovery = ITEM_RECOVERY.to_vec();
            recovery.extend([VAR_KW, CONSTRUCTOR_KW, DESTRUCTOR_KW, R_CURLY]);
            p.skip_until(
                &recovery,
                "E0202",
                format!("expected a class member, found {found}"),
            );
        }
    }
}

// ===== Statements ===============================================================================

fn block(p: &mut Parser) {
    let m = p.open();
    p.bump();
    stmt_list(p, &[R_CURLY]);
    p.expect(T!['}']);
    p.close(m, BLOCK);
}

/// Statements until one of `end` (not consumed), a declaration-only token or the end of input.
fn stmt_list(p: &mut Parser, end: &[SyntaxKind]) {
    while !p.at(EOF) && !p.at_any(end) && !at_item_only(p) {
        if p.at(DEV_CLOSE) {
            if p.dev_depth > 0 {
                break;
            }
            p.advance_with_error("E0202", "`#/` without a matching `/#`");
            continue;
        }
        if p.at(T!['}']) {
            break;
        }
        stmt(p);
    }
}

fn stmt(p: &mut Parser) {
    if p.depth >= MAX_DEPTH {
        p.advance_with_error("E0203", "statements nest too deeply");
        return;
    }
    p.depth += 1;
    stmt_inner(p);
    p.depth -= 1;
}

fn stmt_inner(p: &mut Parser) {
    match p.current() {
        L_CURLY => block(p),
        IF_KW => if_stmt(p),
        WHILE_KW => {
            let m = p.open();
            p.bump();
            condition(p);
            body(p);
            p.close(m, WHILE_STMT);
        }
        DO_KW => {
            let m = p.open();
            p.bump();
            body(p);
            if p.expect(WHILE_KW) {
                condition(p);
            }
            p.expect(T![;]);
            p.close(m, DO_WHILE_STMT);
        }
        FOR_KW => for_stmt(p),
        FOREACH_KW => foreach_stmt(p),
        SWITCH_KW => switch_stmt(p),
        BREAK_KW => simple_stmt(p, BREAK_STMT),
        CONTINUE_KW => simple_stmt(p, CONTINUE_STMT),
        WAITTILLFRAMEEND_KW => {
            let m = p.open();
            p.bump();
            // Decompilers write `waittillframeend();`.
            if p.at(T!['(']) && p.nth(1) == T![')'] {
                p.bump();
                p.bump();
            }
            p.expect(T![;]);
            p.close(m, WAITTILL_FRAME_END_STMT);
        }
        IDENT if p.at_contextual(0, "goto") && p.nth(1) == IDENT && p.nth(2) == T![;] => {
            let m = p.open();
            p.bump();
            name_ref(p);
            p.bump();
            p.close(m, GOTO_STMT);
        }
        IDENT
            if (p.at_contextual(0, "jumpiffalse") || p.at_contextual(0, "jumpiftrue"))
                && p.nth(1) == T!['('] =>
        {
            // acts output for an unstructured conditional jump: `jumpiffalse( c ) LOC_1;`.
            let m = p.open();
            p.bump();
            condition(p);
            name_ref(p);
            p.expect(T![;]);
            p.close(m, GOTO_STMT);
        }
        IDENT if p.nth(1) == T![:] => {
            let m = p.open();
            name(p);
            p.bump();
            p.close(m, LABEL_STMT);
        }
        BREAKPOINT_KW => simple_stmt(p, BREAKPOINT_STMT),
        RETURN_KW => {
            let m = p.open();
            p.bump();
            if !p.at(T![;]) && !p.at(T!['}']) {
                expr(p);
            }
            p.expect(T![;]);
            p.close(m, RETURN_STMT);
        }
        WAIT_KW => expr_keyword_stmt(p, WAIT_STMT),
        WAITREALTIME_KW => expr_keyword_stmt(p, WAIT_REAL_TIME_STMT),
        WAITFRAME_KW if p.nth(1) == T![;] || (p.nth(1) == T!['('] && p.nth(2) == T![')']) => {
            let m = p.open();
            p.bump();
            if p.eat(T!['(']) {
                p.expect(T![')']);
            }
            p.expect(T![;]);
            p.close(m, WAIT_FRAME_STMT);
        }
        CONST_KW => const_stmt(p),
        IDENT if p.at_contextual(0, "const") && is_name_like(p.nth(1)) && p.nth(2) == T![=] => {
            const_stmt(p)
        }
        PROF_BEGIN_KW | PROF_END_KW | PROFILESTART_KW | PROFILESTOP_KW => {
            let m = p.open();
            p.bump();
            if p.at(T!['(']) {
                arg_list(p);
            }
            p.expect(T![;]);
            p.close(m, PROFILER_STMT);
        }
        DEV_OPEN => {
            let m = p.open();
            p.bump();
            p.dev_depth += 1;
            stmt_list(p, &[DEV_CLOSE]);
            p.dev_depth -= 1;
            p.expect(DEV_CLOSE);
            p.close(m, DEV_BLOCK_STMT);
        }
        DEFINE_KW => define(p),
        UNDEF_KW => undef(p),
        SEMICOLON => {
            let m = p.open();
            p.bump();
            p.close(m, EMPTY_STMT);
        }
        CASE_KW | DEFAULT_KW if p.nth(1) != T!['('] => {
            let found = p.found();
            p.advance_with_error("E0202", format!("{found} outside a `switch`"));
        }
        ELSE_KW => p.advance_with_error("E0202", "`else` without `if`"),
        _ if at_expr_start(p) => {
            let m = p.open();
            let start = p.position();
            expr(p);
            if p.eat(T![;]) {
                p.close(m, EXPR_STMT);
            } else if p.newline_before(0) && is_macro_invocation(p, start) {
                // `WAIT_SERVER_FRAME` or `MACRO( x )` on its own line: the macro body
                // supplies the `;`.
                p.close(m, MACRO_STMT);
            } else {
                p.expect(T![;]);
                p.close(m, EXPR_STMT);
            }
        }
        _ => {
            let found = p.found();
            p.advance_with_error("E0202", format!("expected a statement, found {found}"));
        }
    }
}

/// Do the tokens from `start` to the current position look like a macro invocation: an
/// upper-case name, alone or followed by one argument list?
fn is_macro_invocation(p: &Parser, start: usize) -> bool {
    let end = p.position();
    // gsc-tool writes unstructured jumps as `__asm_jmp( loc_5D2 )` with no `;`.
    if p.dialect.decompiled() && p.text_at(start).starts_with("__asm") {
        return true;
    }
    if p.kind_at(start) != IDENT || !p.text_at(start).bytes().any(|b| b.is_ascii_uppercase()) {
        return false;
    }
    if p.text_at(start).bytes().any(|b| b.is_ascii_lowercase()) {
        return false;
    }
    if end == start + 1 {
        return true;
    }
    p.kind_at(start + 1) == L_PAREN && after_parens(p, start + 1) == end
}

fn simple_stmt(p: &mut Parser, kind: SyntaxKind) {
    let m = p.open();
    p.bump();
    p.expect(T![;]);
    p.close(m, kind);
}

fn expr_keyword_stmt(p: &mut Parser, kind: SyntaxKind) {
    let m = p.open();
    p.bump();
    expr(p);
    p.expect(T![;]);
    p.close(m, kind);
}

fn const_stmt(p: &mut Parser) {
    let m = p.open();
    if p.at(CONST_KW) {
        p.bump();
    } else {
        p.bump_remap(CONST_KW);
    }
    name(p);
    p.expect(T![=]);
    expr(p);
    p.expect(T![;]);
    p.close(m, CONST_STMT);
}

fn condition(p: &mut Parser) {
    if p.expect(T!['(']) {
        expr(p);
        p.expect(T![')']);
    }
}

/// The body of `if`, `while`, `for`, `foreach` or `do`.
fn body(p: &mut Parser) {
    if p.at(EOF) || p.at(T!['}']) || at_item_only(p) || p.at(DEV_CLOSE) {
        p.error_expected("a statement");
    } else {
        stmt(p);
    }
}

/// `if`/`else if` chains are parsed iteratively, so a long chain does not deepen the stack.
fn if_stmt(p: &mut Parser) {
    let mut pending = Vec::new();
    loop {
        let m = p.open();
        p.bump();
        condition(p);
        body(p);
        if p.at(ELSE_KW) {
            let e = p.open();
            p.bump();
            // Each link of the chain nests two nodes; it counts against the depth budget
            // so that the tree (whose drop is recursive) stays shallow.
            if p.at(IF_KW) && p.depth < MAX_DEPTH {
                p.depth += 1;
                pending.push((m, e));
                continue;
            }
            body(p);
            p.close(e, ELSE_BRANCH);
        }
        p.close(m, IF_STMT);
        break;
    }
    while let Some((m, e)) = pending.pop() {
        p.close(e, ELSE_BRANCH);
        p.close(m, IF_STMT);
        p.depth -= 1;
    }
}

fn for_stmt(p: &mut Parser) {
    let m = p.open();
    p.bump();
    if p.expect(T!['(']) {
        for (kind, end) in [
            (FOR_INIT, T![;]),
            (FOR_CONDITION, T![;]),
            (FOR_STEP, T![')']),
        ] {
            if !p.at(end) {
                let c = p.open();
                expr(p);
                p.close(c, kind);
            }
            if !p.expect(end) && end == T![;] && !p.at(T![;]) {
                break;
            }
        }
    }
    body(p);
    p.close(m, FOR_STMT);
}

fn foreach_stmt(p: &mut Parser) {
    let m = p.open();
    p.bump();
    if p.expect(T!['(']) {
        let b = p.open();
        foreach_binding(p);
        if p.eat(T![,]) {
            foreach_binding(p);
        }
        p.close(b, FOREACH_BINDINGS);
        if p.at(IN_KW) {
            p.bump();
        } else if p.at_contextual(0, "in") {
            p.bump_remap(IN_KW);
        } else {
            p.error_expected("`in`");
        }
        expr(p);
        p.expect(T![')']);
    }
    body(p);
    p.close(m, FOREACH_STMT);
}

/// A loop variable: a name, or (in decompiler output) any assignable expression.
fn foreach_binding(p: &mut Parser) {
    let simple = matches!(p.nth(1), COMMA | IN_KW) || p.at_contextual(1, "in");
    if simple || !(is_name_like(p.current()) || p.current().is_keyword()) {
        name(p);
    } else {
        expr_bp(p, PREFIX_BP);
    }
}

fn at_clause_start(p: &Parser) -> bool {
    p.at(CASE_KW) || (p.at(DEFAULT_KW) && p.nth(1) == T![:])
}

fn switch_stmt(p: &mut Parser) {
    let m = p.open();
    p.bump();
    condition(p);
    if p.expect(T!['{']) {
        while !p.at(EOF) && !p.at(T!['}']) && !at_item_only(p) {
            if at_clause_start(p) {
                let c = p.open();
                if p.at(CASE_KW) {
                    p.bump();
                    expr(p);
                } else {
                    p.bump();
                }
                p.expect(T![:]);
                while !p.at(EOF) && !p.at(T!['}']) && !at_clause_start(p) && !at_item_only(p) {
                    if p.at(DEV_CLOSE) && p.dev_depth > 0 {
                        break;
                    }
                    stmt(p);
                }
                p.close(c, SWITCH_CLAUSE);
            } else if p.at(DEV_CLOSE) && p.dev_depth > 0 {
                break;
            } else {
                let e = p.open();
                p.error_expected("`case` or `default`");
                stmt(p);
                p.close(e, ERROR);
            }
        }
        p.expect(T!['}']);
    }
    p.close(m, SWITCH_STMT);
}

// ===== Expressions ==============================================================================

fn at_expr_start(p: &Parser) -> bool {
    let k = p.current();
    is_name_like(k)
        || k.is_literal()
        || matches!(
            k,
            PATH | L_PAREN
                | L_BRACK
                | L_CURLY
                | BANG
                | TILDE
                | MINUS
                | PLUS
                | PLUS2
                | MINUS2
                | AMP
                | COLON2
                | PERCENT
                | DOLLAR
                | NEW_KW
                | ANIMTREE_KW
                | THISTHREAD_KW
                | DEFAULT_KW
        )
        || CALL_MODIFIERS.contains(&k)
        || at_keyword_as_name(p)
}

pub(crate) fn expr(p: &mut Parser) -> Option<MarkClosed> {
    expr_bp(p, 0)
}

fn expr_bp(p: &mut Parser, min_bp: u8) -> Option<MarkClosed> {
    if p.depth >= MAX_DEPTH {
        p.error("E0203", "expression nests too deeply");
        if !p.at(EOF) {
            p.advance_with_error("E0203", "expression nests too deeply");
        }
        return None;
    }
    p.depth += 1;
    let r = expr_bp_inner(p, min_bp);
    p.depth -= 1;
    r
}

fn expr_bp_inner(p: &mut Parser, min_bp: u8) -> Option<MarkClosed> {
    let start = p.position();
    let mut lhs = unary(p)?;
    loop {
        let k = p.current();
        // Postfix operators and calls with a receiver bind tightest.
        match k {
            DOT | QUESTION_DOT => {
                let m = p.open_before(lhs);
                p.bump();
                if p.at(T!['(']) {
                    // `obj.( expr )`: a field named by an expression (acts output).
                    paren_expr(p);
                } else {
                    field_name(p);
                }
                lhs = p.close(m, FIELD_EXPR);
                continue;
            }
            L_BRACK if !at_pointer_open(p) => {
                let m = p.open_before(lhs);
                p.bump();
                expr(p);
                p.expect(T![']']);
                lhs = p.close(m, INDEX_EXPR);
                continue;
            }
            PLUS2 | MINUS2 => {
                let m = p.open_before(lhs);
                p.bump();
                lhs = p.close(m, POSTFIX_EXPR);
                continue;
            }
            _ => {}
        }
        // A macro statement on its own line is not the receiver of a call on the next line.
        if at_call_start(p) && !(p.newline_before(0) && is_macro_invocation(p, start)) {
            let m = p.open_before(lhs);
            lhs = call_tail(p, m);
            continue;
        }
        if k == IS_KW || (p.at_contextual(0, "is") && is_is_operand(p)) {
            if IS_BP < min_bp {
                break;
            }
            let m = p.open_before(lhs);
            p.bump_remap(IS_KW);
            if p.at(NOT_KW) {
                p.bump();
            } else if p.at_contextual(0, "not") {
                p.bump_remap(NOT_KW);
            }
            if p.at_any(&[UNDEFINED_KW, TRUE_KW, FALSE_KW, FUNCTION_KW])
                || is_name_like(p.current())
            {
                p.bump();
            } else {
                p.error_expected("`undefined`, `true`, `false`, `function` or a type name");
            }
            lhs = p.close(m, IS_EXPR);
            continue;
        }
        let Some((l_bp, r_bp, kind)) = infix_bp(k) else {
            break;
        };
        if l_bp < min_bp {
            break;
        }
        let m = p.open_before(lhs);
        p.bump();
        if kind == TERNARY_EXPR {
            expr_bp(p, 0);
            p.expect(T![:]);
        }
        if expr_bp(p, r_bp).is_none() && !p.at(EOF) {
            // The error is reported; keep the operator node.
        }
        lhs = p.close(m, kind);
    }
    Some(lhs)
}

fn is_is_operand(p: &Parser) -> bool {
    matches!(p.nth(1), UNDEFINED_KW | TRUE_KW | FALSE_KW | FUNCTION_KW)
        || (p.at_contextual(1, "not"))
}

/// `[[` (two adjacent brackets) not followed by a third: the start of a pointer call.
fn at_pointer_open(p: &Parser) -> bool {
    if !(p.at(T!['[']) && p.nth(1) == T!['[']) {
        return false;
    }
    // `[ [ target ] ]` followed by `(` or `->`; spacing between the brackets is free.
    let start = p.position() + 1;
    let mut depth = 0usize;
    let mut i = start;
    while i < start + 4096 {
        match p.kind_at(i) {
            L_BRACK => depth += 1,
            R_BRACK => {
                depth -= 1;
                if depth == 0 {
                    return p.kind_at(i + 1) == R_BRACK
                        && matches!(p.kind_at(i + 2), L_PAREN | ARROW);
                }
            }
            EOF | SEMICOLON | L_CURLY | R_CURLY => return false,
            _ => {}
        }
        i += 1;
    }
    true
}

/// Does a call (with a receiver, when after an expression) start here?
fn at_call_start(p: &Parser) -> bool {
    let k = p.current();
    if CALL_MODIFIERS.contains(&k) && p.nth(1) != T!['('] {
        return true;
    }
    if at_pointer_open(p) {
        return true;
    }
    if at_contextual_modifier(p) {
        return true;
    }
    (is_name_like(k) || k == PATH) && matches!(p.nth(1), L_PAREN | COLON2)
        || (k.is_keyword() && p.nth(1) == COLON2)
}

/// `childthread`, `call` or `threadendon` in a game whose profile lacks the keyword.
fn at_contextual_modifier(p: &Parser) -> bool {
    p.at(IDENT)
        && (p.at_contextual(0, "childthread")
            || p.at_contextual(0, "call")
            || p.at_contextual(0, "threadendon")
            || p.at_contextual(0, "builtin"))
        && (is_name_like(p.nth(1)) || matches!(p.nth(1), PATH | COLON2 | L_BRACK))
}

/// The rest of a call after its receiver (if any): modifier, callee and arguments. `m` is open.
fn call_tail(p: &mut Parser, m: crate::parser::MarkOpened) -> MarkClosed {
    if CALL_MODIFIERS.contains(&p.current()) {
        p.bump();
    } else if at_contextual_modifier(p) {
        if p.at_contextual(0, "childthread") {
            p.bump_remap(CHILDTHREAD_KW);
        } else if p.at_contextual(0, "call") {
            p.bump_remap(CALL_KW);
        } else if p.at_contextual(0, "threadendon") {
            p.bump_remap(THREADENDON_KW);
        } else if p.at_contextual(0, "builtin") {
            p.bump_remap(BUILTIN_KW);
        }
    }
    if at_pointer_open(p) {
        p.bump();
        p.bump();
        expr(p);
        if p.expect(T![']']) {
            p.expect(T![']']);
        }
        if p.at(T![->]) {
            p.bump();
            if is_any_word(p.current()) {
                name_ref(p);
            } else {
                p.error_expected("a method name");
            }
            args_or_error(p);
            return p.close(m, METHOD_CALL_EXPR);
        }
        args_or_error(p);
        return p.close(m, POINTER_CALL_EXPR);
    }
    if is_name_like(p.current()) || p.at(PATH) || p.at(COLON2) || p.nth(1) == COLON2 {
        callee_ref(p);
        args_or_error(p);
    } else {
        p.error_expected("a function to call");
    }
    p.close(m, CALL_EXPR)
}

fn args_or_error(p: &mut Parser) {
    if p.at(T!['(']) {
        arg_list(p);
    } else {
        p.error_expected("`(`");
    }
}

/// `name`, `ns::name`, `path::name` or `::name`.
fn callee_ref(p: &mut Parser) -> MarkClosed {
    if p.at(COLON2) {
        let m = p.open();
        p.bump();
        qualified_name(p);
        return p.close(m, QUALIFIED_REF);
    }
    if p.nth(1) == COLON2 {
        let m = p.open();
        let q = p.open();
        if p.current().is_keyword() {
            p.bump_remap(IDENT);
        } else {
            p.bump();
        }
        p.close(q, QUALIFIER);
        p.bump();
        qualified_name(p);
        return p.close(m, QUALIFIED_REF);
    }
    name_ref(p)
}

fn qualified_name(p: &mut Parser) {
    if p.at(BANG) && p.joint(0) && p.nth(1) == IDENT {
        // Decompiler output: a hash that resolved to a string starting with `!`.
        let m = p.open();
        p.bump();
        p.bump();
        p.close(m, NAME_REF);
    } else if is_any_word(p.current()) || p.at(PATH) {
        name_ref(p);
    } else {
        p.error_expected("a function name");
    }
}

fn name_ref(p: &mut Parser) -> MarkClosed {
    let m = p.open();
    if is_any_word(p.current()) || p.at(PATH) {
        p.bump();
    } else {
        p.error_expected("a name");
    }
    p.close(m, NAME_REF)
}

fn field_name(p: &mut Parser) {
    if is_any_word(p.current()) || p.at_any(&[HASH_IDENT, PATH, HASH_STRING]) {
        let m = p.open();
        p.bump();
        p.close(m, NAME_REF);
    } else {
        p.error_expected("a field name");
    }
}

fn unary(p: &mut Parser) -> Option<MarkClosed> {
    let k = p.current();
    let m = match k {
        INT_NUMBER | FLOAT_NUMBER | STRING | ISTRING | HASH_STRING | PREFIXED_STRING
        | HASH_IDENT | TRUE_KW | FALSE_KW | UNDEFINED_KW => {
            let m = p.open();
            p.bump();
            p.close(m, LITERAL)
        }
        BANG | TILDE | MINUS | PLUS | PLUS2 | MINUS2 => {
            let m = p.open();
            p.bump();
            expr_bp(p, PREFIX_BP);
            p.close(m, PREFIX_EXPR)
        }
        AMP => {
            let m = p.open();
            p.bump();
            if is_name_like(p.current()) || p.at(PATH) || p.at(COLON2) || p.nth(1) == COLON2 {
                callee_ref(p);
            } else if p.current().is_keyword() {
                name_ref(p);
            } else {
                p.error_expected("a function name");
            }
            p.close(m, FUNCTION_REF)
        }
        COLON2 => {
            let r = callee_ref(p);
            if p.at(T!['(']) {
                let m = p.open_before(r);
                arg_list(p);
                p.close(m, CALL_EXPR)
            } else {
                r
            }
        }
        _ if (CALL_MODIFIERS.contains(&k) && p.nth(1) != T!['('])
            || at_pointer_open(p)
            || at_contextual_modifier(p) =>
        {
            let m = p.open();
            call_tail(p, m)
        }
        IDENT if p.at_contextual(0, "new") && p.nth(1) == IDENT && p.nth(2) == T!['('] => {
            let m = p.open();
            p.bump_remap(NEW_KW);
            name_ref(p);
            arg_list(p);
            p.close(m, NEW_EXPR)
        }
        IDENT | PATH => name_or_call(p),
        DEFAULT_KW if p.nth(1) == T!['('] => {
            // An upper-case macro named like the keyword, e.g. `DEFAULT( x, 0 )`.
            let m = p.open();
            let r = p.open();
            p.bump_remap(IDENT);
            p.close(r, NAME_REF);
            arg_list(p);
            p.close(m, CALL_EXPR)
        }
        k if k.is_name_like_keyword() => name_or_call(p),
        _ if at_keyword_as_name(p) => {
            if p.nth(1) == COLON2 {
                // `class::f()` where `class` names a namespace.
                name_or_call(p)
            } else {
                let r = p.open();
                p.bump_remap(IDENT);
                let r = p.close(r, NAME_REF);
                if p.at(T!['(']) {
                    let c = p.open_before(r);
                    arg_list(p);
                    p.close(c, CALL_EXPR)
                } else {
                    r
                }
            }
        }
        L_BRACK => array_expr(p),
        L_PAREN if at_cast(p) => cast_expr(p),
        L_PAREN => paren_expr(p),
        L_CURLY => struct_expr(p),
        PERCENT => {
            let m = p.open();
            p.bump();
            anim_name(p);
            if p.at(COLON2) {
                p.bump();
                anim_name(p);
            }
            p.close(m, ANIM_REF)
        }
        DOLLAR => {
            let m = p.open();
            p.bump();
            name(p);
            p.close(m, DOLLAR_NAME)
        }
        NEW_KW => {
            let m = p.open();
            p.bump();
            name_ref(p);
            if p.at(T!['(']) {
                arg_list(p);
            }
            p.close(m, NEW_EXPR)
        }
        ANIMTREE_KW => {
            let m = p.open();
            p.bump();
            p.close(m, ANIMTREE_EXPR)
        }
        THISTHREAD_KW => {
            let m = p.open();
            p.bump();
            p.close(m, THIS_THREAD_EXPR)
        }
        _ => {
            p.error_expected("an expression");
            if p.at(ERROR_TOKEN) {
                p.advance_with_error("E0202", "unexpected token");
            }
            return None;
        }
    };
    Some(m)
}

fn anim_name(p: &mut Parser) {
    // Animation names may start with a digit (`%2h_idle`); the lexer splits them.
    if p.at(ENCODED_NAME) {
        let m = p.open();
        p.bump();
        p.close(m, NAME);
    } else if is_any_word(p.current()) || p.at(INT_NUMBER) || p.at(FLOAT_NUMBER) {
        let m = p.open();
        p.bump();
        while is_any_word(p.current()) && p.joint_before(0) {
            p.bump();
        }
        p.close(m, NAME);
    } else {
        p.error_expected("an animation name");
    }
}

fn name_or_call(p: &mut Parser) -> MarkClosed {
    let r = callee_ref(p);
    if p.at(T!['(']) {
        let m = p.open_before(r);
        arg_list(p);
        p.close(m, CALL_EXPR)
    } else {
        r
    }
}

pub(crate) fn arg_list(p: &mut Parser) {
    let m = p.open();
    p.bump();
    while !p.at(EOF) && !p.at(T![')']) && !p.at(T![;]) && !p.at(T!['}']) {
        if p.at(T![,]) {
            p.error_expected("an argument");
            p.bump();
            continue;
        }
        if expr(p).is_none() && !p.at_any(&[R_PAREN, COMMA, SEMICOLON, R_CURLY, EOF]) {
            p.advance_with_error("E0202", "unexpected token in an argument list");
        }
        if !p.at(T![')']) && !p.eat(T![,]) {
            p.error_expected("`,` or `)`");
            break;
        }
    }
    p.expect(T![')']);
    p.close(m, ARG_LIST);
}

/// `(int) x` or `(float) x`: a C-style cast (CoD1 and CoD2 source).
fn at_cast(p: &Parser) -> bool {
    p.at(T!['('])
        && p.nth(2) == T![')']
        && (p.at_contextual(1, "int") || p.at_contextual(1, "float"))
        && {
            let k = p.nth(3);
            is_name_like(k) || k.is_literal() || matches!(k, L_PAREN | MINUS | BANG | TILDE | PATH)
        }
}

fn cast_expr(p: &mut Parser) -> MarkClosed {
    let m = p.open();
    p.bump();
    let t = p.open();
    p.bump();
    p.close(t, NAME_REF);
    p.bump();
    expr_bp(p, PREFIX_BP);
    p.close(m, CAST_EXPR)
}

fn paren_expr(p: &mut Parser) -> MarkClosed {
    let m = p.open();
    p.bump();
    let mut n = 0;
    loop {
        expr(p);
        n += 1;
        if !p.eat(T![,]) {
            break;
        }
    }
    p.expect(T![')']);
    if n == 1 {
        p.close(m, PAREN_EXPR)
    } else {
        if n != 3 {
            p.error_at_previous("E0204", format!("a vector has 3 components, found {n}"));
        }
        p.close(m, VECTOR_EXPR)
    }
}

fn array_expr(p: &mut Parser) -> MarkClosed {
    let m = p.open();
    p.bump();
    while !p.at(EOF) && !p.at(T![']']) && !p.at(T![;]) && !p.at(T!['}']) {
        let e = p.open();
        let value = expr(p);
        if p.at(T![:]) {
            p.bump();
            expr(p);
            p.close(e, ARRAY_ELEMENT);
        } else {
            p.abandon(e);
            if value.is_none() && !p.at_any(&[R_BRACK, COMMA, SEMICOLON, R_CURLY, EOF]) {
                p.advance_with_error("E0202", "unexpected token in an array");
            }
        }
        if !p.at(T![']']) && !p.eat(T![,]) {
            p.error_expected("`,` or `]`");
            break;
        }
    }
    p.expect(T![']']);
    p.close(m, ARRAY_EXPR)
}

fn struct_expr(p: &mut Parser) -> MarkClosed {
    let m = p.open();
    p.bump();
    while !p.at(EOF) && !p.at(T!['}']) && !p.at(T![;]) {
        let f = p.open();
        if expr(p).is_none() && !p.at_any(&[COLON, COMMA, R_CURLY, SEMICOLON, EOF]) {
            p.advance_with_error("E0202", "unexpected token in a struct literal");
        }
        p.expect(T![:]);
        expr(p);
        p.close(f, STRUCT_FIELD);
        if !p.at(T!['}']) && !p.eat(T![,]) {
            p.error_expected("`,` or `}`");
            break;
        }
    }
    p.expect(T!['}']);
    p.close(m, STRUCT_EXPR)
}
