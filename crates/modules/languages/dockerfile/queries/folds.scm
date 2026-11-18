; Long instructions (continued over several lines, or with a here-document: the instruction
; folds, not its here-document again a line below).
[
  (run_instruction)
  (copy_instruction)
  (add_instruction)
  (env_instruction)
  (label_instruction)
  (arg_instruction)
  (cmd_instruction)
  (entrypoint_instruction)
  (healthcheck_instruction)
] @fold
(json_string_array) @fold.array
