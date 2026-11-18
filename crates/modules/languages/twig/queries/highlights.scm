; Adapted from tree-sitter-twig (gbprod, MIT).

(comment) @comment

(filter_identifier) @function.call
(function_identifier) @function.call
(test) @function.builtin
(method) @function.method

(variable) @variable
(parameter) @variable.parameter
(attribute) @attribute

(string) @string
(interpolated_string) @string
(number) @number
(boolean) @boolean
(null) @constant.builtin
(operator) @operator

(keyword) @keyword
(tag) @keyword
(conditional) @keyword.conditional
(repeat) @keyword.repeat

[
  "{{" "}}" "{{-" "-}}" "{{~" "~}}"
  "{%" "%}" "{%-" "-%}" "{%~" "~%}"
] @tag.delimiter

[ "," "." ] @punctuation.delimiter
[ "?" ":" "=" "|" ] @operator

(interpolated_string [ "#{" "}" ] @punctuation.special)
[ "(" ")" "[" "]" ] @punctuation.bracket
(hash [ "{" "}" ] @punctuation.bracket)
