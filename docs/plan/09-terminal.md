# 09 — Terminal integrat (modulul CLI)

## 1. Cerințe

- Mai multe taburi de terminal, cu redenumire și închidere; opțional, split.
- Shell implicit al sistemului (zsh/bash/fish pe Unix, PowerShell/cmd pe Windows) sau profile configurate.
- Culori ANSI (16, 256 și truecolor), mouse reporting, alternate screen (vim, htop), bracketed paste.
- Randat pe GPU, prin același pipeline și același atlas de glife ca editorul.
- Link-uri clicabile: căile de fișier cu linie (`app/Models/User.php:42`) se deschid în editor, iar URL-urile în browser.
- Directorul de start: rădăcina proiectului.

## 2. Arhitectură

```
terminal-session (domeniu)
  TerminalSession { id, title, profile, state: Running | Exited(code) }
  ShellProfile    { name, program, args, env, cwd }
  port:   trait TerminalSession { write, resize, snapshot(&mut GridSnapshot), scroll,
                                  title, exit_status, take_dirty }
  encode_key / encode_paste: tastele și textul lipit → secvențe xterm (pur, testat)

pty (infrastructură)
  PtySession : TerminalSession  ← crate `alacritty_terminal`: PTY-ul (ConPTY pe Windows,
                                  openpty pe Unix), bucla de citire / scriere și emulatorul VT
                                  (grid, scrollback, ecran alternativ)

modules/tools/terminal
  TerminalPanel (UI): taburi, view-ul de grid → Scene (quads pentru fundal, glyph-uri pentru text)
  comenzi: terminal.new, terminal.close, terminal.next, terminal.rename, terminal.split
```

- Citirea din PTY se face pe runtime-ul async. Bytes ajung în emulator, iar emulatorul emite `TerminalDirty`, care cere un frame nou (limitat la rata de refresh).
- **Throttling:** la output masiv (`cat` pe un fișier mare), se procesează tot, dar se randează maximum 1 frame per vsync.
- Paleta ANSI vine din temă (`[terminal]`).

> Decizie (faza 5): `portable-pty` nu mai e necesar. `alacritty_terminal` are propriul strat
> PTY cross-platform și bucla de evenimente, deci o singură dependență.

## 3. Etape

1. Un singur terminal, cu taburi și emulare completă.
2. Link detection și integrare cu editorul (click pe `fișier:linie`).
3. Split, profile, „Run configurations” (ex: `php artisan test` cu un click).
4. Shell integration (OSC 133 / OSC 7): cwd-ul curent, marcaje de comenzi și navigare între prompt-uri.
