; HTTP request files. On the same node the first pattern wins: the specific ones come
; first. The captures are among those every bundled theme colors (a URL `@function`, a path
; `@type`): told apart from text, discreetly.

(comment) @comment
(request_separator) @comment

; What a comment asks (`# @name login`, `# @no-redirect`): its directive, its value.
(comment
  "@" @keyword
  name: (_) @keyword)
(comment
  "=" @operator)

; The request line.
(method) @keyword
(request
  url: (_) @function)
(http_version) @constant

; Headers.
(header
  name: (_) @property)
(header
  ":" @punctuation.delimiter)
(header
  value: (_) @string)

; Variables: declared (`@host = …`), and `{{name}}` wherever they're used.
(variable_declaration
  name: (identifier) @property)
(variable_declaration
  "=" @operator)
(variable_declaration
  value: (_) @string)
(variable
  name: (_) @variable)
[
  "{{"
  "}}"
] @punctuation.special

; A body from a file (`< ./body.json`), a response written to one.
(external_body
  path: (_) @type)
(res_redirect
  path: (_) @type)

; A response written in the file (a REST Client's example).
(status_code) @number
(status_text) @string
