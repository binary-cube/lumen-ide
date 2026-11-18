; As in tree-sitter: on the same node the first pattern wins.
(pair key: (string) @property)
(escape_sequence) @string.escape
(string) @string
(number) @number
[(true) (false)] @boolean
(null) @null
(comment) @comment
["{" "}" "[" "]"] @punctuation.bracket
[":" ","] @punctuation.delimiter
