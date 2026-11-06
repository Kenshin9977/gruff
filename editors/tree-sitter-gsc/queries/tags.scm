; SPDX-License-Identifier: MIT
; Code navigation tags (GitHub-style `tree-sitter tags`).

(function_definition name: (_) @name) @definition.function
(class_definition name: (identifier) @name) @definition.class
(class_definition
  body: (class_body (function_definition name: (_) @name) @definition.method))
(preproc_define name: (identifier) @name) @definition.macro
(namespace_directive name: (identifier) @name) @definition.module

(call_expression function: (identifier) @name) @reference.call
(call_expression function: (qualified_identifier name: (_) @name)) @reference.call
(function_reference (identifier) @name) @reference.call
(function_reference (qualified_identifier name: (_) @name)) @reference.call
(new_expression class: (identifier) @name) @reference.class
(using_directive path: (_) @name) @reference.module
