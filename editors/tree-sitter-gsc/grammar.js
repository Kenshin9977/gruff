// tree-sitter grammar for GSC (Call of Duty game scripts): the superset of every dialect from
// CoD1 to BO7, source and decompiler output. Shape follows grammar/gsc.ungram; dialect gating is
// left to the language server. See README.md.
// SPDX-License-Identifier: MIT

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const PREC = {
  assign: 1,
  ternary: 2,
  coalesce: 3,
  or: 4,
  and: 5,
  bitor: 6,
  bitxor: 7,
  bitand: 8,
  equality: 9,
  relational: 10,
  shift: 11,
  additive: 12,
  multiplicative: 13,
  unary: 14,
  call: 16,
  postfix: 17,
  member: 18,
};

/**
 * A keyword token. GSC compilers fold the case of keywords (`If`, `Function` in shipped files).
 * @param {string} word
 */
function kwToken(word) {
  const pattern = word
    .split('')
    .map(c => (/[a-z]/.test(c) ? `[${c}${c.toUpperCase()}]` : c))
    .join('');
  return token(new RegExp(pattern));
}

/**
 * A keyword, as an anonymous node named after its lower-case spelling (for queries).
 * @param {string} word
 */
function kw(word) {
  return alias(kwToken(word), word);
}

const MODIFIERS = ['thread', 'childthread', 'call', 'threadendon', 'builtin'].map(kw);

const WORD = /[A-Za-z_][A-Za-z0-9_]*/;

// The body of a quoted string: any character but a quote or a line break; `\` escapes one.
const STRING_BODY = repeat(choice(/[^"\\\r\n]+/, /\\[^\r\n]/));

/**
 * Comma-separated, optionally empty.
 * @param {RuleOrLiteral} rule
 */
function commaSep(rule) {
  return optional(seq(rule, repeat(seq(',', rule))));
}

module.exports = grammar({
  name: 'gsc',

  word: $ => $.identifier,

  externals: $ => [
    $.preproc_disabled,
    $.encoded_name,
    $._macro_name,
    $._decompiled_path,
    $._decompiled_name,
    $._directive_end,
    $._error_sentinel,
  ],

  extras: $ => [
    /[\s ﻿]/,
    // A line continuation (`#define` bodies).
    /\\\r?\n/,
    $.comment,
    $.preproc_condition,
    $.preproc_disabled,
  ],

  supertypes: $ => [
    $._statement,
    $._expression,
  ],

  conflicts: $ => [
    [$._function_name, $.macro_call_item],
    [$.block, $.struct],
    [$.label_statement, $._expression],
    [$.dev_block_item],
    [$.goto_statement, $._reserved_identifier],
    [$.foreach_statement, $._reserved_identifier],
    [$.call_modifier, $._reserved_identifier],
    [$.array, $._pointer_target],
  ],

  rules: {
    source_file: $ => repeat($._item),

    _item: $ => choice(
      $.using_directive,
      $.include_directive,
      $.insert_directive,
      $.inline_directive,
      $.namespace_directive,
      $.precache_directive,
      $.using_animtree_directive,
      $.preproc_define,
      $.preproc_undef,
      $.macro_call_item,
      $.function_definition,
      $.class_definition,
      $.file_constant,
      $.dev_block_item,
      ';',
    ),

    // ===== Directives =======================================================================

    using_directive: $ => seq('#using', field('path', $._directive_path), ';'),
    include_directive: $ => seq('#include', field('path', $._directive_path), ';'),
    insert_directive: $ => seq('#insert', field('path', $._directive_path), ';'),
    inline_directive: $ => seq('#inline', field('path', $._directive_path), ';'),

    _directive_path: $ => choice(alias($.directive_path, $.path), $.string),

    // `scripts\shared\util`, `scripts/shared/util`, `maps\mp\_load.gsh`.
    directive_path: _ => /[A-Za-z0-9_\\/.\-]+/,

    namespace_directive: $ => seq('#namespace', field('name', $.identifier), ';'),
    precache_directive: $ => seq('#precache', field('arguments', $.argument_list), ';'),
    using_animtree_directive: $ => seq(
      '#using_animtree',
      field('arguments', $.argument_list),
      ';',
    ),

    // ===== Preprocessor =====================================================================

    preproc_define: $ => seq(
      '#define',
      field('name', $.identifier),
      optional(field('parameters', $.preproc_params)),
      optional(field('value', $.preproc_arg)),
      // The line break ends the directive, so a body that is one word (`#define X true`) is not
      // read as the start of the next declaration.
      $._directive_end,
    ),

    preproc_params: $ => seq(
      token.immediate('('),
      commaSep(choice($.identifier, '...')),
      ')',
    ),

    // The rest of the line; a trailing backslash continues it.
    preproc_arg: _ => token(prec(-1, /[^\s\\]([^\\\r\n]|\\\r?\n|\\[^\r\n])*/)),

    preproc_undef: $ => seq('#undef', field('name', $.identifier)),

    // `#if X`, `#ifdef X`, `#ifndef X`, `#endif`: one line each, kept as extras.
    preproc_condition: _ => token(seq(
      '#',
      choice('if', 'ifdef', 'ifndef', 'endif'),
      optional(seq(/[^A-Za-z0-9_\r\n]/, /([^\\\r\n]|\\\r?\n|\\[^\r\n])*/)),
    )),

    // `REGISTER_SYSTEM( "name", &init, undefined )` at file scope; the `;` is usually absent.
    macro_call_item: $ => prec.dynamic(-1, prec.right(seq(
      field('macro', $.identifier),
      field('arguments', $.argument_list),
      optional(';'),
    ))),

    // ===== Declarations =====================================================================

    function_definition: $ => seq(
      optional(kw('function')),
      repeat(choice(
        alias(kw('autoexec'), $.modifier),
        alias(kw('private'), $.modifier),
        alias(kw('codecall'), $.modifier),
        $.event_handler,
      )),
      field('name', $._function_name),
      field('parameters', $.parameter_list),
      field('body', $.block),
    ),

    // Decompiler output may name a function or parameter with a path (a hash that resolved to
    // `scripts\mp\x` or `maps/creek/fx_name`) or a string starting with `!`.
    _function_name: $ => choice(
      $.identifier,
      alias($.decompiled_name, $.identifier),
      $.path,
      alias($._decompiled_name, $.path),
    ),

    decompiled_name: _ => token(seq('!', WORD)),

    event_handler: $ => seq('event_handler', '[', field('event', $.identifier), ']'),

    parameter_list: $ => seq('(', commaSep(choice($.parameter, $.vararg_parameter)), ')'),

    parameter: $ => seq(
      optional(choice('&', '*')),
      field('name', choice($.identifier, $.path, alias($._decompiled_name, $.path))),
      optional(seq('=', field('default', $._expression))),
    ),

    vararg_parameter: _ => '...',

    class_definition: $ => seq(
      kw('class'),
      field('name', $.identifier),
      optional(field('bases', $.class_bases)),
      field('body', $.class_body),
    ),

    class_bases: $ => seq(':', $.identifier, repeat(seq(',', $.identifier))),

    class_body: $ => seq('{', repeat($._class_member), '}'),

    _class_member: $ => choice(
      $.class_variable,
      $.constructor_definition,
      $.destructor_definition,
      $.function_definition,
      $.preproc_define,
      $.preproc_undef,
      $.dev_block_item,
      $.macro_call_item,
      ';',
    ),

    class_variable: $ => seq(
      kw('var'),
      field('name', $.identifier),
      optional(seq('=', field('value', $._expression))),
      ';',
    ),

    constructor_definition: $ => seq(
      kw('constructor'),
      field('parameters', $.parameter_list),
      field('body', $.block),
    ),

    destructor_definition: $ => seq(
      kw('destructor'),
      field('parameters', $.parameter_list),
      field('body', $.block),
    ),

    // IW `NAME = expr;` between functions.
    file_constant: $ => seq(field('name', $.identifier), '=', field('value', $._expression), ';'),

    // A file-scope dev block holds whole declarations; shipped files may leave it open.
    dev_block_item: $ => seq('/#', repeat($._item), optional('#/')),

    // ===== Statements =======================================================================

    _statement: $ => choice(
      $.block,
      $.expression_statement,
      $.macro_statement,
      $.if_statement,
      $.while_statement,
      $.do_statement,
      $.for_statement,
      $.foreach_statement,
      $.switch_statement,
      $.break_statement,
      $.continue_statement,
      $.return_statement,
      $.wait_statement,
      $.waitrealtime_statement,
      $.waittillframeend_statement,
      $.const_statement,
      $.profiler_statement,
      $.breakpoint_statement,
      $.dev_block,
      $.preproc_define,
      $.preproc_undef,
      $.goto_statement,
      $.label_statement,
      $.empty_statement,
    ),

    block: $ => seq('{', repeat($._statement), '}'),

    expression_statement: $ => seq($._expression, ';'),

    // An upper-case macro on its own line whose body supplies the `;` (`WAIT_SERVER_FRAME`,
    // `DEFAULT( x, 0 )`), or gsc-tool's `__asm_jmp( loc_1 )`. The scanner checks the shape.
    macro_statement: $ => prec.right(seq(
      field('macro', alias($._macro_name, $.identifier)),
      optional(field('arguments', $.argument_list)),
    )),

    if_statement: $ => prec.right(seq(
      kw('if'),
      field('condition', $.condition),
      field('consequence', $._statement),
      optional(field('alternative', $.else_clause)),
    )),

    else_clause: $ => seq(kw('else'), $._statement),

    condition: $ => seq('(', $._expression, ')'),

    while_statement: $ => seq(
      kw('while'),
      field('condition', $.condition),
      field('body', $._statement),
    ),

    do_statement: $ => seq(
      kw('do'),
      field('body', $._statement),
      kw('while'),
      field('condition', $.condition),
      ';',
    ),

    for_statement: $ => seq(
      kw('for'),
      '(',
      field('initializer', optional($._expression)),
      ';',
      field('condition', optional($._expression)),
      ';',
      field('update', optional($._expression)),
      ')',
      field('body', $._statement),
    ),

    foreach_statement: $ => seq(
      kw('foreach'),
      '(',
      field('binding', $._expression),
      optional(seq(',', field('binding', $._expression))),
      kw('in'),
      field('iterable', $._expression),
      ')',
      field('body', $._statement),
    ),

    switch_statement: $ => seq(
      kw('switch'),
      field('condition', $.condition),
      field('body', $.switch_body),
    ),

    switch_body: $ => seq('{', repeat(choice($.case_clause, $.default_clause, $.dev_block)), '}'),

    case_clause: $ => prec.right(seq(
      kw('case'),
      field('value', $._expression),
      ':',
      repeat($._statement),
    )),

    default_clause: $ => prec.right(seq(kw('default'), ':', repeat($._statement))),

    break_statement: _ => seq(kw('break'), ';'),
    continue_statement: _ => seq(kw('continue'), ';'),

    return_statement: $ => seq(kw('return'), optional($._expression), ';'),

    wait_statement: $ => seq(kw('wait'), $._expression, ';'),
    waitrealtime_statement: $ => seq(kw('waitrealtime'), $._expression, ';'),
    waittillframeend_statement: _ => seq(kw('waittillframeend'), optional(seq('(', ')')), ';'),

    const_statement: $ => seq(
      kw('const'),
      field('name', $.identifier),
      '=',
      field('value', $._expression),
      ';',
    ),

    profiler_statement: $ => seq(
      choice(kw('prof_begin'), kw('prof_end'), kw('profilestart'), kw('profilestop')),
      optional($.argument_list),
      ';',
    ),

    breakpoint_statement: _ => seq(kw('breakpoint'), ';'),

    dev_block: $ => seq('/#', repeat($._statement), '#/'),

    // `goto LOC_1;` and acts' conditional jumps `jumpiffalse( c ) LOC_1;`.
    goto_statement: $ => choice(
      seq(kw('goto'), field('label', $.identifier), ';'),
      seq(
        choice(kw('jumpiffalse'), kw('jumpiftrue')),
        field('condition', $.condition),
        field('label', $.identifier),
        ';',
      ),
    ),

    label_statement: $ => seq(field('label', $.identifier), ':'),

    empty_statement: _ => ';',

    // ===== Expressions ======================================================================

    _expression: $ => choice(
      $.identifier,
      $._reserved_identifier,
      $.path,
      $.qualified_identifier,
      $.integer,
      $.float,
      $.string,
      $.istring,
      $.hash_string,
      $.prefixed_string,
      $.hash_identifier,
      $.true,
      $.false,
      $.undefined,
      $.function_reference,
      $.call_expression,
      $.pointer_call_expression,
      $.method_call_expression,
      $.field_expression,
      $.subscript_expression,
      $.unary_expression,
      $.update_expression,
      $.binary_expression,
      $.assignment_expression,
      $.ternary_expression,
      $.is_expression,
      $.parenthesized_expression,
      $.vector,
      $.array,
      $.struct,
      $.new_expression,
      $.animation_reference,
      $.animtree,
      $.dollar_name,
      $.thisthread,
      $.cast_expression,
    ),

    // `ns::f`, `maps\mp\_utility::f`, `::f` (a function of this file).
    qualified_identifier: $ => prec(PREC.member, seq(
      optional(field('namespace', choice($.identifier, $.path, $._decompiled_path_name))),
      '::',
      field('name', $._function_name),
    )),

    _callee: $ => choice(
      $.identifier,
      $._reserved_identifier,
      $.qualified_identifier,
      $.path,
      $._decompiled_path_name,
    ),

    // Decompiler output: a name glued with `/` (`dlc5/zmhd/fx_name( x )`), before `(` or `::`.
    _decompiled_path_name: $ => alias($._decompiled_path, $.path),

    call_modifier: _ => choice(...MODIFIERS),

    // `f( a )`, `self thread f( a )`, `ns::f()`.
    call_expression: $ => prec(PREC.call, seq(
      optional(field('receiver', $._expression)),
      optional(field('modifier', $.call_modifier)),
      field('function', $._callee),
      field('arguments', $.argument_list),
    )),

    // `[[ f ]]( a )`, `self thread [[ level.cb ]]()`.
    pointer_call_expression: $ => prec(PREC.call, seq(
      optional(field('receiver', $._expression)),
      optional(field('modifier', $.call_modifier)),
      $._pointer_target,
      field('arguments', $.argument_list),
    )),

    // `[[ obj ]]->method( a )`.
    method_call_expression: $ => prec(PREC.call, seq(
      optional(field('receiver', $._expression)),
      optional(field('modifier', $.call_modifier)),
      $._pointer_target,
      '->',
      field('method', $.identifier),
      field('arguments', $.argument_list),
    )),

    // `[[ target ]]`, the brackets maybe spaced. A rule of its own so that `[ [ a ] ]` can still
    // be a nested array: the call reading needs the `(` or `->` after it.
    _pointer_target: $ => seq('[', '[', field('target', $._expression), ']', ']'),

    argument_list: $ => seq('(', commaSep($._expression), optional(','), ')'),

    function_reference: $ => prec(PREC.unary, seq('&', $._callee)),

    field_expression: $ => prec(PREC.member, seq(
      field('object', $._expression),
      choice('.', '?.'),
      field('field', choice(
        $.identifier,
        $.hash_identifier,
        $.hash_string,
        $.path,
        $.parenthesized_expression,
      )),
    )),

    subscript_expression: $ => prec(PREC.member, seq(
      field('object', $._expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    unary_expression: $ => prec(PREC.unary, seq(
      field('operator', choice('!', '~', '-', '+')),
      field('argument', $._expression),
    )),

    update_expression: $ => {
      const op = choice('++', '--');
      return choice(
        prec(PREC.unary, seq(field('operator', op), field('argument', $._expression))),
        prec(PREC.postfix, seq(field('argument', $._expression), field('operator', op))),
      );
    },

    binary_expression: $ => {
      /** @type {[number, RuleOrLiteral][]} */
      const table = [
        [PREC.coalesce, '??'],
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.bitor, '|'],
        [PREC.bitxor, '^'],
        [PREC.bitand, '&'],
        [PREC.equality, choice('==', '!=', '===', '!==')],
        [PREC.relational, choice('<', '<=', '>', '>=')],
        [PREC.shift, choice('<<', '>>')],
        [PREC.additive, choice('+', '-')],
        [PREC.multiplicative, choice('*', '/', '%')],
      ];
      return choice(...table.map(([p, op]) => {
        const rule = seq(
          field('left', $._expression),
          field('operator', op),
          field('right', $._expression),
        );
        // `??` groups to the right, as in the Rust parser.
        return p === PREC.coalesce ? prec.right(p, rule) : prec.left(p, rule);
      }));
    },

    assignment_expression: $ => prec.right(PREC.assign, seq(
      field('left', $._expression),
      field('operator', choice('=', '+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '<<=', '>>=')),
      field('right', $._expression),
    )),

    ternary_expression: $ => prec.right(PREC.ternary, seq(
      field('condition', $._expression),
      '?',
      field('consequence', $._expression),
      ':',
      field('alternative', $._expression),
    )),

    is_expression: $ => prec.left(PREC.equality, seq(
      field('left', $._expression),
      kw('is'),
      optional(kw('not')),
      field('type', choice($.undefined, $.true, $.false, kw('function'), $.identifier)),
    )),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    vector: $ => seq('(', $._expression, ',', $._expression, repeat(seq(',', $._expression)), ')'),

    array: $ => seq(
      '[',
      commaSep(choice($._expression, $.array_element)),
      optional(','),
      ']',
    ),

    array_element: $ => seq(field('key', $._expression), ':', field('value', $._expression)),

    struct: $ => seq('{', commaSep($.struct_field), optional(','), '}'),

    struct_field: $ => seq(field('key', $._expression), ':', field('value', $._expression)),

    new_expression: $ => prec.right(PREC.unary, seq(
      kw('new'),
      field('class', $.identifier),
      optional(field('arguments', $.argument_list)),
    )),

    // `%idle`, `%generic::idle`, `%2h_idle`, acts' `%\x83\x99...`.
    animation_reference: $ => prec.right(seq(
      '%',
      optional(seq(field('tree', $.animation_name), '::')),
      field('name', choice($.animation_name, $.encoded_name)),
    )),

    animation_name: _ => /[A-Za-z0-9_@]+/,

    animtree: _ => '#animtree',

    dollar_name: _ => seq('$', token.immediate(/[A-Za-z0-9_]+/)),

    thisthread: _ => kwToken('thisthread'),

    // `(int) x`, `(float) x`: CoD1 source.
    // `int` and `float` are function names elsewhere (`float( s )`).
    cast_expression: $ => prec.dynamic(1, prec(PREC.unary, seq(
      '(',
      field('type', alias(choice(kwToken('int'), kwToken('float')), $.type)),
      ')',
      field('value', $._expression),
    ))),

    // Words with a meaning in one construct that are ordinary names elsewhere
    // (`goto = nodes[ 0 ];`, `float( s )`).
    _reserved_identifier: $ => alias(
      choice(...['goto', 'jumpiffalse', 'jumpiftrue', 'int', 'float', 'new', 'call', 'builtin', 'foreach']
        .map(kwToken)),
      $.identifier,
    ),

    // ===== Tokens ===========================================================================

    identifier: _ => token(choice(
      WORD,
      // Decompiler spellings: `@name`, and names starting with a digit (`051a_name`).
      seq('@', /[A-Za-z0-9_]+/),
      /[0-9]+[A-Za-z_][A-Za-z0-9_]*/,
    )),

    // `maps\mp\_utility`, `scripts\shared\util_shared`.
    // Decompiler output also names functions `jup\fly\x\name.ln.85.all`.
    path: _ => token(seq(
      WORD,
      repeat1(seq('\\', /[A-Za-z0-9_]+/)),
      repeat(seq('.', /[A-Za-z0-9_]+/)),
    )),

    integer: _ => token(prec(1, choice(/0[xX][0-9a-fA-F]+/, /[0-9]+/))),

    float: _ => token(prec(1, choice(
      /[0-9]+\.[0-9]*([eE][+-]?[0-9]+)?/,
      /\.[0-9]+([eE][+-]?[0-9]+)?/,
      /[0-9]+[eE][+-]?[0-9]+/,
    ))),

    string: _ => token(seq('"', STRING_BODY, '"')),

    // `&"MENU_LABEL"`: a localized string.
    istring: _ => token(seq('&"', STRING_BODY, '"')),

    // `#"name"`: a hashed string.
    hash_string: _ => token(seq('#"', STRING_BODY, '"')),

    // `%"asset"`, `@"..."`, `t"..."`, `s"..."`, `r"..."`, `o"..."`.
    prefixed_string: _ => token(seq(/[%@tsro]"/, STRING_BODY, '"')),

    // `#name` (a hashed name in acts output), `#tm7/name`; a bare `#` is acts' zero hash.
    hash_identifier: _ => choice(
      token(seq('#', WORD, repeat(seq('/', /[A-Za-z0-9_]+/)))),
      '#',
    ),

    true: _ => 'true',
    false: _ => 'false',
    undefined: _ => 'undefined',

    comment: _ => token(choice(
      seq('//', /[^\r\n]*/),
      seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/'),
      // `/@ ... @/` doc comments (T7 and later).
      seq('/@', /[^@]*@+([^/@][^@]*@+)*/, '/'),
    )),
  },
});
