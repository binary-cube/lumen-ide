; All the text outside `<?php ... ?>` is HTML, parsed as one document.
((text) @injection.content
 (#set! injection.language "html")
 (#set! injection.combined))

; Heredoc / nowdoc: the label names the language (<<<JSON, <<<HTML, <<<SQL...).
(heredoc
  (heredoc_body) @injection.content
  (heredoc_end) @injection.language)

(nowdoc
  (nowdoc_body) @injection.content
  (heredoc_end) @injection.language)
