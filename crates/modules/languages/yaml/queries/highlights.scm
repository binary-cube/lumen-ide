; As in tree-sitter: on the same node the first pattern wins.
(block_mapping_pair key: (flow_node [(plain_scalar (string_scalar)) (double_quote_scalar) (single_quote_scalar)] @property))
(flow_pair key: (flow_node [(plain_scalar (string_scalar)) (double_quote_scalar) (single_quote_scalar)] @property))
(comment) @comment
(escape_sequence) @string.escape
[(string_scalar) (double_quote_scalar) (single_quote_scalar) (block_scalar)] @string
[(integer_scalar) (float_scalar)] @number
(boolean_scalar) @boolean
(null_scalar) @null
[(anchor_name) (alias_name)] @type
(tag) @type
