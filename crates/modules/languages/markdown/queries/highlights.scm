; The blocks, in the theme's names (`keyword` for headings: the color a theme gives what
; stands out). As in tree-sitter: on the same node the first pattern wins.
(atx_heading (inline) @keyword)
(setext_heading (paragraph) @keyword)
[
  (atx_h1_marker)
  (atx_h2_marker)
  (atx_h3_marker)
  (atx_h4_marker)
  (atx_h5_marker)
  (atx_h6_marker)
  (setext_h1_underline)
  (setext_h2_underline)
] @keyword

(fenced_code_block_delimiter) @punctuation
(info_string) @type
(indented_code_block) @string

(link_reference_definition (link_label) @property)
(link_destination) @string
(link_title) @string

[
  (list_marker_plus)
  (list_marker_minus)
  (list_marker_star)
  (list_marker_dot)
  (list_marker_parenthesis)
  (thematic_break)
] @punctuation
[
  (task_list_marker_checked)
  (task_list_marker_unchecked)
] @constant

(block_quote) @comment
[
  (block_continuation)
  (block_quote_marker)
] @comment

(pipe_table_header (pipe_table_cell) @property)
[
  (pipe_table_delimiter_row)
  "|"
] @punctuation

(backslash_escape) @string.escape
[
  (entity_reference)
  (numeric_character_reference)
] @string.escape
