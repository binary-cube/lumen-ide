; Twig's blocks are "flat" directives (`{% if %}` and `{% endif %}` are siblings in the tree):
; they pair by their keyword, `X` with `endX` (block, if, for, macro, apply, embed,
; autoescape, verbatim, set in its block form…).
(statement_directive
  (_ [(tag) (conditional) (repeat)] @fold.key)) @fold.pair

(comment) @fold
