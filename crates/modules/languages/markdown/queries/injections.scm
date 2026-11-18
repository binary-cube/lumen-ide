; A fenced block in the language its info string names (` ```php `).
(fenced_code_block
  (info_string
    (language) @injection.language)
  (code_fence_content) @injection.content)

((html_block) @injection.content
  (#set! injection.language "html"))

; The front matter: YAML between `---`, TOML between `+++`.
((minus_metadata) @injection.content
  (#set! injection.language "yaml"))
((plus_metadata) @injection.content
  (#set! injection.language "toml"))

; Each block's text: emphasis, code spans, links.
((inline) @injection.content
  (#set! injection.language "markdown_inline"))
