; Bash (and the shells it reads well enough: sh, dash, ksh, zsh). On the same node the first
; pattern wins: the specific ones come first.

(comment) @comment

; Strings and what's inside them.
(ansi_c_string) @string
(raw_string) @string
(translated_string) @string
(heredoc_body) @string
[
  (heredoc_start)
  (heredoc_end)
] @label
(string) @string
(regex) @string.regex
(extglob_pattern) @string.regex

; Expansions: `$x`, `${x:-y}`, `$(…)`, `$((…))`.
(special_variable_name) @variable.builtin
(simple_expansion
  "$" @punctuation.special)
(expansion
  [
    "${"
    "}"
  ] @punctuation.special)
(command_substitution
  [
    "$("
    ")"
    "`"
  ] @punctuation.special)
(arithmetic_expansion
  [
    "$(("
    "))"
  ] @punctuation.special)
(process_substitution
  [
    "<("
    ">("
    ")"
  ] @punctuation.special)

; Functions: declared, called (the shell's builtins first).
(function_definition
  name: (word) @function)
(command_name
  (word) @function.builtin
  (#any-of? @function.builtin
    "alias" "bg" "bind" "break" "builtin" "caller" "cd" "command" "compgen" "complete"
    "compopt" "continue" "dirs" "disown" "echo" "enable" "eval" "exec" "exit" "false" "fc"
    "fg" "getopts" "hash" "help" "history" "jobs" "kill" "let" "logout" "mapfile" "popd"
    "printf" "pushd" "pwd" "read" "readarray" "return" "set" "shift" "shopt" "source" "suspend"
    "test" "times" "trap" "true" "type" "ulimit" "umask" "unalias" "wait" "."))
(command_name) @function.call

; A command's options (`-v`, `--force`).
((command
  argument: (word) @attribute)
  (#match? @attribute "^--?[A-Za-z0-9]"))

; Names.
(variable_assignment
  name: (variable_name) @variable)
(variable_name) @variable
(file_descriptor) @number
(number) @number
(test_operator) @operator

; Keywords.
[
  "case"
  "do"
  "done"
  "elif"
  "else"
  "esac"
  "fi"
  "for"
  "function"
  "if"
  "in"
  "select"
  "then"
  "until"
  "while"
] @keyword
[
  "declare"
  "export"
  "local"
  "readonly"
  "typeset"
  "unset"
  "unsetenv"
] @keyword

; Operators.
[
  "&&"
  "||"
  "|"
  "|&"
  "&"
  ";"
  ";;"
  ";&"
  ";;&"
  ">"
  ">>"
  "<"
  "<<"
  "<<-"
  "<<<"
  "&>"
  "&>>"
  ">&"
  "<&"
  ">|"
  "="
  "=="
  "!="
  "=~"
  "+="
  "!"
] @operator
[
  "("
  ")"
  "(("
  "))"
  "["
  "]"
  "[["
  "]]"
  "{"
  "}"
] @punctuation
