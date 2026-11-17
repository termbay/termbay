# termbay shell integration: zsh startup shim (protocol 1).
# Part of termbay, MIT License. https://github.com/termbay/termbay
#
# termbay starts zsh with ZDOTDIR pointing to the folder of this file, only
# when `[autocomplete] enabled = true`. This file:
#   1. moves the session nonce out of the environment, so no program
#      started from now on inherits it;
#   2. puts ZDOTDIR back the way it was (or unsets it), so the user's
#      .zprofile, .zshrc and .zlogin load from their usual place and
#      HISTFILE, .zcompdump and plugins see the original value;
#   3. loads the user's own .zshenv;
#   4. in interactive shells, loads termbay.zsh from this folder.
# It never writes any file.
#
# Starting zsh through a temporary ZDOTDIR that restores the user's own is
# a common technique (VS Code, among others). This file is termbay's own
# code; it is not based on the kitty or Ghostty zsh integration (GPL-3.0).

typeset -g _termbay_nonce="${TERMBAY_NONCE-}"
unset TERMBAY_NONCE
typeset -g _termbay_dir="${ZDOTDIR-}"

if [[ -n "${TERMBAY_USER_ZDOTDIR+set}" ]]; then
  ZDOTDIR="$TERMBAY_USER_ZDOTDIR"
  unset TERMBAY_USER_ZDOTDIR
else
  unset ZDOTDIR
fi

if [[ -f "${ZDOTDIR-$HOME}/.zshenv" && -r "${ZDOTDIR-$HOME}/.zshenv" ]]; then
  builtin source "${ZDOTDIR-$HOME}/.zshenv"
fi

if [[ -o interactive && -n "$_termbay_nonce" && -r "$_termbay_dir/termbay.zsh" ]]; then
  builtin source "$_termbay_dir/termbay.zsh"
else
  unset _termbay_nonce
fi
unset _termbay_dir
