; Lua. On the same node the first pattern wins: the specific ones come first.

(hash_bang_line) @comment
(comment) @comment

; Literals.
(escape_sequence) @string.escape
(string) @string
(number) @number
[
  (true)
  (false)
] @boolean
(nil) @null
(vararg_expression) @constant.builtin

; Functions: declared, called (the standard library's own first).
(function_declaration
  name: (identifier) @function)
(function_declaration
  name: (dot_index_expression
    field: (identifier) @function))
(function_declaration
  name: (method_index_expression
    method: (identifier) @function.method))
(function_call
  name: (identifier) @function.builtin
  (#any-of? @function.builtin
    "assert" "collectgarbage" "dofile" "error" "getmetatable" "ipairs" "load" "loadfile"
    "next" "pairs" "pcall" "print" "rawequal" "rawget" "rawlen" "rawset" "require"
    "select" "setmetatable" "tonumber" "tostring" "type" "unpack" "xpcall"))
(function_call
  name: (identifier) @function.call)
(function_call
  name: (dot_index_expression
    field: (identifier) @function.call))
(function_call
  name: (method_index_expression
    method: (identifier) @function.method))

; Names.
(parameters
  name: (identifier) @variable.parameter)
((identifier) @variable.builtin
  (#eq? @variable.builtin "self"))
(field
  name: (identifier) @property)
(dot_index_expression
  field: (identifier) @property)
(attribute
  (identifier) @attribute)
(label_statement) @label
(goto_statement
  (identifier) @label)
((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))

; Keywords.
(break_statement) @keyword
[
  "do"
  "else"
  "elseif"
  "end"
  "for"
  "function"
  "global"
  "goto"
  "if"
  "in"
  "local"
  "repeat"
  "return"
  "then"
  "until"
  "while"
] @keyword
[
  "and"
  "or"
  "not"
] @keyword.operator

[
  "+"
  "-"
  "*"
  "/"
  "//"
  "%"
  "^"
  "#"
  "=="
  "~="
  "<"
  "<="
  ">"
  ">="
  "="
  "&"
  "|"
  "~"
  "<<"
  ">>"
  ".."
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
  ":"
  "::"
  ","
  "."
] @punctuation.delimiter
