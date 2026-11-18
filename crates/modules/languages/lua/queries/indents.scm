; The nodes whose inner lines go one level in (a line starting with `end`, `}` or `)` stays
; at the node's own level).
[
  (function_declaration)
  (function_definition)
  (if_statement)
  (for_statement)
  (while_statement)
  (repeat_statement)
  (do_statement)
  (table_constructor)
  (arguments)
  (parameters)
] @indent
