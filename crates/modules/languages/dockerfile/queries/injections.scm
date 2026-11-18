; Each shell command is its own Bash document (not combined: the end of one `RUN` must not
; run into the start of the next). The Bash grammar is the Bash module's: nothing here
; when it's off.
((shell_command) @injection.content
  (#set! injection.language "bash"))

; A here-document's body (`RUN <<EOF … EOF`), its closing line left out.
((run_instruction
  (heredoc_block
    (heredoc_content) @injection.content))
  (#set! injection.language "bash"))
