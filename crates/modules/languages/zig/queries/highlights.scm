; Zig. On the same node the first pattern wins: the specific ones come first.

(comment) @comment

; Literals.
(escape_sequence) @string.escape
[
  (string)
  (multiline_string)
  (character)
] @string
[
  (integer)
  (float)
] @number
(boolean) @boolean
"null" @null
[
  "undefined"
  "unreachable"
] @constant.builtin

; Builtins (`@import`, `@as`…) and types.
(builtin_identifier) @function.builtin
[
  (builtin_type)
  "anyframe"
] @type.builtin

; Functions: declared, called.
(function_declaration
  name: (identifier) @function)
(call_expression
  function: (identifier) @function.call)
(call_expression
  function: (field_expression
    member: (identifier) @function.method))

; Names.
(parameter
  name: (identifier) @variable.parameter)
(container_field
  name: (identifier) @property)
(field_initializer
  .
  (identifier) @property)
(field_expression
  member: (identifier) @property)
(block_label
  (identifier) @label)
(break_label
  (identifier) @label)
((identifier) @variable.builtin
  (#eq? @variable.builtin "_"))
((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))
((identifier) @type
  (#match? @type "^[A-Z][a-zA-Z0-9_]*$"))
(variable_declaration
  (identifier) @type
  "="
  [
    (struct_declaration)
    (enum_declaration)
    (union_declaration)
    (opaque_declaration)
    (error_set_declaration)
  ])

; Keywords.
[
  "addrspace"
  "align"
  "allowzero"
  "asm"
  "async"
  "await"
  "break"
  "callconv"
  "catch"
  "comptime"
  "const"
  "continue"
  "defer"
  "else"
  "enum"
  "errdefer"
  "error"
  "export"
  "extern"
  "fn"
  "for"
  "if"
  "inline"
  "linksection"
  "noalias"
  "noinline"
  "nosuspend"
  "opaque"
  "orelse"
  "packed"
  "pub"
  "resume"
  "return"
  "struct"
  "suspend"
  "switch"
  "test"
  "threadlocal"
  "try"
  "union"
  "usingnamespace"
  "var"
  "volatile"
  "while"
] @keyword
[
  "and"
  "or"
] @keyword.operator

[
  "="
  "*="
  "*%="
  "*|="
  "/="
  "%="
  "+="
  "+%="
  "+|="
  "-="
  "-%="
  "-|="
  "<<="
  "<<|="
  ">>="
  "&="
  "^="
  "|="
  "!"
  "~"
  "-"
  "-%"
  "&"
  "=="
  "!="
  ">"
  ">="
  "<="
  "<"
  "^"
  "|"
  "<<"
  ">>"
  "<<|"
  "+"
  "++"
  "+%"
  "-%"
  "+|"
  "-|"
  "*"
  "/"
  "%"
  "**"
  "*%"
  "*|"
  "||"
  ".*"
  ".?"
  "?"
  ".."
  "=>"
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
  "."
  ","
  ":"
] @punctuation.delimiter
