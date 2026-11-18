; What C and C++ share (only nodes both grammars have), after each one's own patterns.

(comment) @comment

; The preprocessor.
[
  "#define"
  "#elif"
  "#elifdef"
  "#elifndef"
  "#else"
  "#endif"
  "#if"
  "#ifdef"
  "#ifndef"
  "#include"
  (preproc_directive)
] @keyword.directive
"defined" @keyword.directive
(preproc_def
  name: (identifier) @constant)
(preproc_function_def
  name: (identifier) @function)
(preproc_params
  (identifier) @variable.parameter)
(system_lib_string) @string

; Literals.
(escape_sequence) @string.escape
[
  (string_literal)
  (char_literal)
] @string
(number_literal) @number
[
  (true)
  (false)
] @boolean
(null) @null

; Functions: called, declared.
(call_expression
  function: (identifier) @function.call)
(call_expression
  function: (field_expression
    field: (field_identifier) @function.method))
(function_declarator
  declarator: (identifier) @function)
(function_declarator
  declarator: (parenthesized_declarator
    (pointer_declarator
      declarator: (identifier) @function)))

; Types.
[
  (primitive_type)
  (sized_type_specifier)
] @type.builtin
(type_identifier) @type

; Parameters.
(parameter_declaration
  declarator: (identifier) @variable.parameter)
(parameter_declaration
  declarator: (pointer_declarator
    declarator: (identifier) @variable.parameter))
(parameter_declaration
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (identifier) @variable.parameter)))
(parameter_declaration
  declarator: (array_declarator
    declarator: (identifier) @variable.parameter))

; Fields, labels, constants (enumerators, ALL_CAPS names: mostly macros).
(field_identifier) @property
(statement_identifier) @label
(enumerator
  name: (identifier) @constant)
((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))

; Attributes (`[[nodiscard]]`, `__attribute__((…))`).
(attribute_declaration) @attribute
(attribute_specifier) @attribute

[
  "break"
  "case"
  "const"
  "constexpr"
  "continue"
  "default"
  "do"
  "else"
  "enum"
  "extern"
  "for"
  "goto"
  "if"
  "inline"
  "register"
  "restrict"
  "return"
  "sizeof"
  "static"
  "struct"
  "switch"
  "thread_local"
  "typedef"
  "union"
  "volatile"
  "while"
  "_Atomic"
  "_Generic"
  "_Noreturn"
  "noreturn"
  "alignas"
  "alignof"
  "offsetof"
  "asm"
  "__attribute__"
  "__declspec"
  "__extension__"
  (type_qualifier)
  (storage_class_specifier)
] @keyword

[
  "="
  "+="
  "-="
  "*="
  "/="
  "%="
  "&="
  "|="
  "^="
  "<<="
  ">>="
  "+"
  "-"
  "*"
  "/"
  "%"
  "++"
  "--"
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "&&"
  "||"
  "!"
  "&"
  "|"
  "^"
  "~"
  "<<"
  ">>"
  "->"
  "?"
] @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ";"
  ","
  "."
  ":"
  "..."
] @punctuation.delimiter
