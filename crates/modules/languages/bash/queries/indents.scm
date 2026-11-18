; The nodes whose inner lines go one level in (a line starting with `fi`, `done`, `esac`,
; `}` or `)` stays at the node's own level).
[
  (compound_statement)
  (do_group)
  (if_statement)
  (elif_clause)
  (else_clause)
  (case_statement)
  (case_item)
  (subshell)
  (array)
] @indent
