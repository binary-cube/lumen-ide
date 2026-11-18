; .env files. On the same node the first pattern wins: the specific ones come first. The
; captures are among those every bundled theme colors (a URL `@function`, a path `@type`):
; told apart from text, discreetly.

(comment) @comment

"export" @keyword
"=" @operator

; Keys, and the variables interpolated in values: colored as the keys they name.
(assignment
  key: (identifier) @property)
(variable
  (identifier) @property)
(variable
  [
    "$"
    "{"
    "}"
  ] @punctuation.special)
(variable
  (operator) @operator)
(variable
  (default) @string)

; Values: booleans, numbers, URLs, paths, text.
(escape_sequence) @string.escape
(boolean) @boolean
(number) @number
((value) @boolean
  (#match? @boolean "^(?i:true|false|yes|no|on|off)$"))
((value) @function
  (#match? @function "^[A-Za-z][A-Za-z0-9+.-]*://"))
((value) @type
  (#match? @type "^(/|\\./|\\.\\./|~/|%kernel\\.project_dir%)"))
(value) @string
((string_content) @function
  (#match? @function "^[A-Za-z][A-Za-z0-9+.-]*://"))
((string_content) @type
  (#match? @type "^(/|\\./|\\.\\./|~/|%kernel\\.project_dir%)"))
(string) @string
