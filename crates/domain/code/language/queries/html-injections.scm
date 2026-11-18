; For the same region the first pattern wins: the JSON scripts first, then the scripts and
; styles written in another language (`lang="ts"`, `lang="scss"`: Vue), then the rest.
((script_element
   (start_tag
     (attribute
       (attribute_name) @_attr
       (quoted_attribute_value (attribute_value) @_type)))
   (raw_text) @injection.content)
 (#eq? @_attr "type")
 (#match? @_type "json")
 (#set! injection.language "json"))

((script_element
   (start_tag
     (attribute
       (attribute_name) @_attr
       (quoted_attribute_value (attribute_value) @_lang)))
   (raw_text) @injection.content)
 (#eq? @_attr "lang")
 (#any-of? @_lang "ts" "typescript")
 (#set! injection.language "typescript"))

((script_element
   (start_tag
     (attribute
       (attribute_name) @_attr
       (quoted_attribute_value (attribute_value) @_lang)))
   (raw_text) @injection.content)
 (#eq? @_attr "lang")
 (#eq? @_lang "tsx")
 (#set! injection.language "tsx"))

((style_element
   (start_tag
     (attribute
       (attribute_name) @_attr
       (quoted_attribute_value (attribute_value) @_lang)))
   (raw_text) @injection.content)
 (#eq? @_attr "lang")
 (#eq? @_lang "scss")
 (#set! injection.language "scss"))

((script_element
  (raw_text) @injection.content)
 (#set! injection.language "javascript"))

((style_element
  (raw_text) @injection.content)
 (#set! injection.language "css"))
