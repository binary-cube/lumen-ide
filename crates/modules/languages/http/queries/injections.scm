; A request's body in its own language; its scripts (`< {% %}`, `> {% %}`) in JavaScript.
((json_body) @injection.content
 (#set! injection.language "json"))

((xml_body) @injection.content
 (#set! injection.language "html"))

((script) @injection.content
 (#offset! @injection.content 0 2 0 -2)
 (#set! injection.language "javascript"))
