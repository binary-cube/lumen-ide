; Everything outside Twig's directives is HTML (with CSS / JS / JSON in it), parsed as one
; document, like the HTML in PHP.
((content) @injection.content
 (#set! injection.language "html")
 (#set! injection.combined))
