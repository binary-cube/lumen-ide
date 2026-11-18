# A typical ~/.zshrc.
export PATH="$HOME/.local/bin:$PATH"
export EDITOR=vim
HISTSIZE=10000
setopt HIST_IGNORE_DUPS SHARE_HISTORY
autoload -Uz compinit && compinit
bindkey -e
alias ll='ls -la'
alias gs="git status"
zstyle ':completion:*' menu select

if [[ -f ~/.zsh_aliases ]]; then
    source ~/.zsh_aliases
fi

mkcd() {
    mkdir -p "$1" && cd "$1"
}

for plugin in git docker; do
    [[ -r "$HOME/.zsh/$plugin.zsh" ]] && source "$HOME/.zsh/$plugin.zsh"
done

PROMPT='%n@%m %~ %# '
