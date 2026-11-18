; Attributes that are a template's directives (Vue's `v-*`, `:x`, `@x`, `#x`; htmx's
; `hx-*`; Alpine's `x-*`): colored apart from plain attributes (as keywords). Before HTML's
; own highlights: at the same place the first pattern wins.
((attribute_name) @keyword.directive
 (#match? @keyword.directive "^(v-|hx-|x-|[:@#])"))
