; SPDX-License-Identifier: MIT
; Highlights for GSC. Later patterns win in Neovim; the tree-sitter CLI takes the first match,
; so specific patterns come first.

; Functions --------------------------------------------------------------------------------------

(function_definition name: (identifier) @function)
(function_definition name: (path) @function)
(constructor_definition "constructor" @constructor)
(destructor_definition "destructor" @constructor)

(call_expression function: (identifier) @function.call)
(call_expression function: (path) @function.call)
(call_expression
  function: (qualified_identifier name: (identifier) @function.call))
(method_call_expression method: (identifier) @function.method.call)
(function_reference (identifier) @function)
(function_reference (qualified_identifier name: (identifier) @function))
(qualified_identifier name: (identifier) @function)
(qualified_identifier namespace: (_) @module)

(macro_call_item macro: (identifier) @function.macro)
(macro_statement macro: (identifier) @function.macro)
(preproc_define name: (identifier) @constant.macro)
(preproc_undef name: (identifier) @constant.macro)

; Engine objects and well-known names --------------------------------------------------------------

((identifier) @variable.builtin
  (#match? @variable.builtin "^([sS][eE][lL][fF]|[lL][eE][vV][eE][lL]|[gG][aA][mM][eE]|[aA][nN][iI][mM]|[wW][oO][rR][lL][dD]|[vV][aA][rR][aA][rR][gG])$"))

((call_expression function: (identifier) @function.builtin)
  (#match? @function.builtin "^([iI][sS][dD][eE][fF][iI][nN][eE][dD]|[nN][oO][tT][iI][fF][yY]|[eE][nN][dD][oO][nN]|[wW][aA][iI][tT][tT][iI][lL][lL]|[wW][aA][iI][tT][tT][iI][lL][lL][mM][aA][tT][cC][hH]|[wW][aA][iI][tT][tT][iI][lL][lL][aA][nN][yY]|[wW][aA][iI][tT][fF][rR][aA][mM][eE]|[aA][sS][sS][eE][rR][tT]|[aA][sS][sS][eE][rR][tT][mM][sS][gG])$"))

; Declarations -----------------------------------------------------------------------------------

(class_definition name: (identifier) @type)
(class_bases (identifier) @type)
(new_expression class: (identifier) @type)
(class_variable name: (identifier) @variable.member)
(parameter name: (_) @variable.parameter)
(vararg_parameter) @variable.parameter
(file_constant name: (identifier) @constant)
(const_statement name: (identifier) @constant)
(namespace_directive name: (identifier) @module)
(event_handler event: (identifier) @label)
(label_statement label: (identifier) @label)
(goto_statement label: (identifier) @label)

(field_expression field: (identifier) @property)
(field_expression field: (hash_identifier) @property)

(animation_reference) @string.special
(animtree) @string.special
(dollar_name) @variable.builtin

; Literals ---------------------------------------------------------------------------------------

(string) @string
(istring) @string.special
(hash_string) @string.special
(prefixed_string) @string.special
(hash_identifier) @string.special.symbol
(path) @string.special.path
(integer) @number
(float) @number.float
[(true) (false)] @boolean
(undefined) @constant.builtin
(thisthread) @variable.builtin

(comment) @comment
(preproc_disabled) @comment

; Keywords ---------------------------------------------------------------------------------------

[
  "#using" "#include" "#insert" "#inline" "#namespace" "#precache" "#using_animtree"
] @keyword.import

[
  "#define" "#undef"
] @keyword.directive.define

(preproc_condition) @keyword.directive
(preproc_arg) @embedded
["/#" "#/"] @keyword.directive

[
  "if" "else" "switch" "case" "default"
] @keyword.conditional

[
  "while" "do" "for" "foreach" "in" "break" "continue"
] @keyword.repeat

[
  "return"
] @keyword.return

[
  "function" "class" "var" "const" "event_handler"
] @keyword

(modifier) @keyword.modifier
(call_modifier) @keyword.coroutine

[
  "wait" "waitrealtime" "waittillframeend"
] @keyword.coroutine

[
  "new" "is" "not"
] @keyword.operator

[
  "goto" "jumpiffalse" "jumpiftrue" "breakpoint"
  "prof_begin" "prof_end" "profilestart" "profilestop"
] @keyword

(cast_expression type: _ @type.builtin)

; Operators and punctuation ----------------------------------------------------------------------

[
  "=" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
  "==" "!=" "===" "!==" "<" "<=" ">" ">="
  "+" "-" "*" "/" "%" "&&" "||" "!" "~" "&" "|" "^" "<<" ">>" "??"
  "++" "--" "->" "?" ":"
] @operator

["." "?." "::" "," ";"] @punctuation.delimiter
["(" ")" "[" "]" "{" "}"] @punctuation.bracket

(identifier) @variable
