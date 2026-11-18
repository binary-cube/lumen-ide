; The extractors' templates (`*.extractor`), read by the Twig grammar (already in the tree):
; its `{{ … }}` tags are ours. A section's tag (`#rows`, `^last`, `/columns`: signs the
; grammar doesn't know) is colored whole as a keyword; the template language says its own
; errors, with their place.

(output_directive (ERROR) (variable) @keyword)
(output_directive (ERROR) @keyword)
(comment) @comment
["{{" "}}"] @tag.delimiter
(filter_identifier) @function.call
(variable) @variable
(number) @number
(string) @string
[ "|" ":" ] @operator
"." @punctuation.delimiter
