; The text inside the blocks, in the theme's names.
(code_span) @string
[
  (emphasis_delimiter)
  (code_span_delimiter)
] @punctuation

(strong_emphasis) @keyword
(emphasis) @variable
(strikethrough) @comment

[
  (link_text)
  (image_description)
] @function
[
  (link_destination)
  (uri_autolink)
  (email_autolink)
] @string
(link_label) @property
(link_title) @string

(image ["!" "[" "]" "(" ")"] @punctuation)
(inline_link ["[" "]" "(" ")"] @punctuation)
(shortcut_link ["[" "]"] @punctuation)

[
  (backslash_escape)
  (hard_line_break)
  (entity_reference)
  (numeric_character_reference)
] @string.escape
