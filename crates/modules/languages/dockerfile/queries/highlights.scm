; Dockerfile. On the same node the first pattern wins: the specific ones come first. The
; shell commands (`RUN`, `CMD`, `ENTRYPOINT` in shell form, here-documents) are colored as
; Bash (`injections.scm`).

(comment) @comment

; Instructions.
[
  "FROM"
  "AS"
  "RUN"
  "CMD"
  "LABEL"
  "EXPOSE"
  "ENV"
  "ADD"
  "COPY"
  "ENTRYPOINT"
  "VOLUME"
  "USER"
  "WORKDIR"
  "ARG"
  "ONBUILD"
  "STOPSIGNAL"
  "HEALTHCHECK"
  "SHELL"
  "MAINTAINER"
  "CROSS_BUILD"
  "NONE"
] @keyword

; The image: its name, tag, digest and the stage's name.
(image_spec
  name: (image_name) @type)
(image_tag) @constant
(image_digest) @constant
(image_alias) @label
(from_instruction
  (param) @attribute)

; Flags (`--from=build`, `--mount=type=cache,…`) and names set.
(param) @attribute
(mount_param) @attribute
(arg_pair
  name: (unquoted_string) @variable)
(env_pair
  name: (unquoted_string) @variable)
(label_pair
  key: (_) @property)
(expose_port) @number

; Variables: `$NAME`, `${NAME:-default}`.
(expansion
  [
    "$"
    "{"
    "}"
  ] @punctuation.special)
(expansion_operator) @operator
((variable) @constant
  (#match? @constant "^[A-Z][A-Z_0-9]*$"))
(variable) @variable

; Strings.
(escape_sequence) @string.escape
[
  (double_quoted_string)
  (single_quoted_string)
  (json_string)
] @string
[
  (heredoc_marker)
  (heredoc_end)
] @label
(line_continuation) @operator

[
  "["
  "]"
  ","
] @punctuation
"=" @operator
