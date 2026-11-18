; TOML. On the same node the first pattern wins: the specific ones come first.

(comment) @comment

; Table headers: `[package]`, `[[bin]]`, `[target.'cfg(unix)'.dependencies]`.
(table
  [
    (bare_key)
    (quoted_key)
    (dotted_key)
  ] @tag)
(table_array_element
  [
    (bare_key)
    (quoted_key)
    (dotted_key)
  ] @tag)

; Keys: `name = …`, `a.b = …`.
(pair
  (bare_key) @property)
(pair
  (quoted_key) @property)
(pair
  (dotted_key) @property)

; Values.
(escape_sequence) @string.escape
(string) @string
[
  (integer)
  (float)
] @number
(boolean) @boolean
[
  (offset_date_time)
  (local_date_time)
  (local_date)
  (local_time)
] @constant

; Punctuation.
"=" @operator
[
  "."
  ","
] @punctuation
[
  "["
  "]"
  "[["
  "]]"
  "{"
  "}"
] @punctuation
