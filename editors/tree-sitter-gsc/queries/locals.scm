; SPDX-License-Identifier: MIT
; Scopes and local variables. GSC locals live for the whole function: blocks open no scope.

(function_definition) @local.scope
(constructor_definition) @local.scope
(destructor_definition) @local.scope

(parameter name: (identifier) @local.definition)
(assignment_expression left: (identifier) @local.definition)
(foreach_statement binding: (identifier) @local.definition)
(const_statement name: (identifier) @local.definition)

(identifier) @local.reference
