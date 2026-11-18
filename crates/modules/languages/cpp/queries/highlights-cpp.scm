; C++'s own patterns, before what it shares with C (highlights-common.scm).
; On the same node the first pattern wins.

; Calls and declarations through a scope or a template.
(call_expression
  function: (qualified_identifier
    name: (identifier) @function.call))
(call_expression
  function: (qualified_identifier
    name: (qualified_identifier
      name: (identifier) @function.call)))
(call_expression
  function: (template_function
    name: (identifier) @function.call))
(call_expression
  function: (qualified_identifier
    name: (template_function
      name: (identifier) @function.call)))
(call_expression
  function: (field_expression
    field: (template_method
      name: (field_identifier) @function.method)))
(function_declarator
  declarator: (qualified_identifier
    name: (identifier) @function))
(function_declarator
  declarator: (qualified_identifier
    name: (qualified_identifier
      name: (identifier) @function)))
(function_declarator
  declarator: (field_identifier) @function.method)
(function_declarator
  declarator: (destructor_name) @function.method)
(function_declarator
  declarator: (operator_name) @function.method)
(template_function
  name: (identifier) @function)
(template_method
  name: (field_identifier) @function.method)

; Types, namespaces, template parameters.
(namespace_identifier) @type
(template_type
  name: (type_identifier) @type)
(auto) @type.builtin
(this) @variable.builtin

; Parameters by reference.
(parameter_declaration
  declarator: (reference_declarator
    (identifier) @variable.parameter))
(optional_parameter_declaration
  declarator: (identifier) @variable.parameter)

(raw_string_literal) @string
(user_defined_literal
  (literal_suffix) @number)

[
  "catch"
  "class"
  "co_await"
  "co_return"
  "co_yield"
  "concept"
  "consteval"
  "constinit"
  "decltype"
  "delete"
  "explicit"
  "final"
  "friend"
  "mutable"
  "namespace"
  "new"
  "noexcept"
  "operator"
  "override"
  "private"
  "protected"
  "public"
  "requires"
  "static_assert"
  "template"
  "throw"
  "try"
  "typename"
  "using"
  "virtual"
] @keyword

[
  "and"
  "and_eq"
  "bitand"
  "bitor"
  "compl"
  "not"
  "not_eq"
  "or"
  "or_eq"
  "xor"
  "xor_eq"
] @keyword.operator

[
  "::"
  "<=>"
  ".*"
  "->*"
] @operator
