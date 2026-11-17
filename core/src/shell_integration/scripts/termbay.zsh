# termbay shell integration for zsh (protocol 1).
# Part of termbay, MIT License. https://github.com/termbay/termbay
#
# Loaded by the .zshenv shim next to this file, in interactive shells that
# termbay started with `[autocomplete] enabled = true`. It never writes any
# file and never runs the command line. It emits:
#
#   - OSC 133 A, C and D (semantic prompt marks) and OSC 7 (working folder);
#   - termbay's private OSC 6973, always with the session nonce:
#       ready;<protocol>;zsh;<version>;<history file>   once, at the first prompt
#       prompt;<exit code, or empty>                     when zle starts reading a
#                                                        new command line (below)
#       exec                                             before every command
#       complete;<request>;<insert|command>;<line>;<cursor>;<word start>;
#                [<text>;<insert>;<description>]...      answer to a request
#     Fields are escaped: "\" as "\\", ";" as "\x3b", bytes below 0x20 and
#     0x7f as "\xHH". Positions count characters.
#
# termbay asks for completions only when the user presses the trigger key
# at the prompt. It types private-use characters no keyboard sends:
# U+10FFF0..U+10FFF9 add a digit to the request number and U+10FFFD runs
# the completion widget. The widget asks zsh's own completion system for
# the matches (capturing `compadd`), answers with the private OSC and
# leaves the command line and the screen untouched.
#
# Third-party code:
#   The core of _termbay_compadd (the zparseopts option list, passing calls
#   with -O, -A or -D on to the builtin, and capturing the matches and
#   descriptions with `builtin compadd -A ... -D ...`) and the way it is
#   hooked as `functions[compadd]` are adapted from `-ftb-compadd` in
#   fzf-tab (https://github.com/Aloxaf/fzf-tab), under the MIT License
#   below. The rest of this file is termbay's own. fzf-tab credits the idea
#   to zsh-capture-completion (https://github.com/Valodim/zsh-capture-completion,
#   MIT License); no code of it is used here.
#
#   The field escaping ("\\", "\x3b", "\xHH") follows the convention of the
#   VS Code shell integration (OSC 633, MIT License); the code is our own.
#   OSC 7 and OSC 133 (FinalTerm semantic prompts) are public protocols.
#   Nothing here comes from Warp, iTerm2 or kitty.
#
#   MIT License
#
#   Copyright (c) 2019-2026 Aloxaf
#
#   Permission is hereby granted, free of charge, to any person obtaining a copy
#   of this software and associated documentation files (the "Software"), to deal
#   in the Software without restriction, including without limitation the rights
#   to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
#   copies of the Software, and to permit persons to whom the Software is
#   furnished to do so, subject to the following conditions:
#
#   The above copyright notice and this permission notice shall be included in all
#   copies or substantial portions of the Software.
#
#   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
#   IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
#   FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
#   AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
#   LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
#   OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
#   SOFTWARE.

[[ -n "${_termbay_nonce-}" ]] || return 0

typeset -g _termbay_req=''
typeset -gi _termbay_ran=0 _termbay_ready=0 _termbay_prompt_due=0 _termbay_hooked=0
typeset -g _termbay_code=''

# When is the shell "at the prompt"? Not in precmd: a command typed ahead
# while the previous one ran (`cat` + Enter) is still in the terminal's
# input queue then, and zle runs it right away. So the prompt mark is sent
# from zle-line-init, only when no input is pending ($PENDING is 0), or
# else from the first zle-line-pre-redraw with nothing pending that does
# not follow an accept-line (typed-ahead letters without Enter). If a
# plugin replaced zsh's hook dispatcher, precmd sends the mark as before.

# UTF-8 bytes of the trigger characters, so the bindings work in any locale.
typeset -ga _termbay_digit_keys=(
  $'\xf4\x8f\xbf\xb0' $'\xf4\x8f\xbf\xb1' $'\xf4\x8f\xbf\xb2' $'\xf4\x8f\xbf\xb3'
  $'\xf4\x8f\xbf\xb4' $'\xf4\x8f\xbf\xb5' $'\xf4\x8f\xbf\xb6' $'\xf4\x8f\xbf\xb7'
  $'\xf4\x8f\xbf\xb8' $'\xf4\x8f\xbf\xb9'
)
typeset -g _termbay_run_key=$'\xf4\x8f\xbf\xbd'

# Escapes one field into $REPLY.
_termbay_enc() {
  emulate -L zsh
  local s=$1 c
  local -i i
  s=${s//\\/\\\\}
  s=${s//;/\\x3b}
  if [[ $s == *[[:cntrl:]]* || $s == *$'\0'* ]]; then
    for i in {0..31} 127; do
      c=${(#)i}
      [[ $s == *$c* ]] && s=${s//$c/\\x${(l:2::0:)$(( [##16] i ))}}
    done
  fi
  REPLY=$s
}

# Prints the private OSC with the given fields.
_termbay_osc() {
  emulate -L zsh
  local out=$_termbay_nonce f
  for f in "$@"; do
    _termbay_enc "$f"
    out+=";$REPLY"
  done
  builtin print -rn -- $'\e]6973;'"$out"$'\a'
}

# OSC 7 with the percent-encoded working folder.
_termbay_osc7() {
  emulate -L zsh
  setopt localoptions no_multibyte
  local p=$PWD out='' c hex
  local -i i
  for (( i = 1; i <= ${#p}; i++ )); do
    c=${p[i]}
    if [[ $c == [A-Za-z0-9/._~-] ]]; then
      out+=$c
    else
      builtin printf -v hex '%%%02X' $(( #c & 255 ))
      out+=$hex
    fi
  done
  builtin print -rn -- $'\e]7;file://'"${HOST}${out}"$'\a'
}

_termbay_digit() {
  (( ${#_termbay_req} < 20 )) && _termbay_req+=${WIDGET##*_}
}

# Binds the trigger in every keymap. Runs before every prompt, so keymaps
# reset by a plugin or by `bindkey -e`/`-v` get the bindings back.
_termbay_line_init() {
  (( _termbay_prompt_due )) || return 0
  (( PENDING )) && return 0
  _termbay_prompt_due=0
  _termbay_osc prompt "$_termbay_code"
}

_termbay_pre_redraw() {
  (( _termbay_prompt_due )) || return 0
  (( PENDING )) && return 0
  [[ $LASTWIDGET == *accept* ]] && return 0
  _termbay_prompt_due=0
  _termbay_osc prompt "$_termbay_code"
}

# Hooks zle-line-init and zle-line-pre-redraw once, keeping the user's own
# hook widgets (add-zle-hook-widget, zsh 5.3 or newer).
_termbay_hook() {
  (( _termbay_hooked )) && return 0
  _termbay_hooked=1
  autoload -Uz add-zle-hook-widget 2>/dev/null || return 0
  add-zle-hook-widget line-init _termbay_line_init 2>/dev/null
  add-zle-hook-widget line-pre-redraw _termbay_pre_redraw 2>/dev/null
}

# Whether both of our zle hooks will run: zsh's dispatcher is still in
# place and lists them.
_termbay_hooks_ok() {
  emulate -L zsh
  zmodload -i zsh/zleparameter 2>/dev/null || return 1
  [[ ${widgets[zle-line-init]-} == user:azhw:zle-line-init ]] || return 1
  [[ ${widgets[zle-line-pre-redraw]-} == user:azhw:zle-line-pre-redraw ]] || return 1
  local -a list
  zstyle -a zle-line-init widgets list
  [[ -n ${(M)list:#<->:_termbay_line_init} ]] || return 1
  zstyle -a zle-line-pre-redraw widgets list
  [[ -n ${(M)list:#<->:_termbay_pre_redraw} ]] || return 1
}

_termbay_bind() {
  emulate -L zsh
  local km
  local -i d
  for d in {0..9}; do
    zle -N _termbay_digit_$d _termbay_digit
  done
  zle -N _termbay_complete
  zle -C _termbay_compwidget complete-word _termbay_compfunc
  for km in emacs viins vicmd; do
    for d in {0..9}; do
      bindkey -M $km "${_termbay_digit_keys[d+1]}" _termbay_digit_$d
    done
    bindkey -M $km "$_termbay_run_key" _termbay_complete
  done
}

_termbay_precmd() {
  local -i ret=$?
  emulate -L zsh
  if (( _termbay_ran )); then
    builtin print -rn -- $'\e]133;D;'"$ret"$'\a'
  fi
  _termbay_bind 2>/dev/null
  if (( ! _termbay_ready )); then
    _termbay_ready=1
    _termbay_osc ready 1 zsh "$ZSH_VERSION" "${HISTFILE-}"
  fi
  _termbay_osc7
  if (( _termbay_ran )); then
    _termbay_code=$ret
  else
    _termbay_code=''
  fi
  _termbay_hook
  if _termbay_hooks_ok; then
    _termbay_prompt_due=1
  else
    _termbay_osc prompt "$_termbay_code"
  fi
  _termbay_ran=0
  builtin print -rn -- $'\e]133;A\a'
}

_termbay_preexec() {
  emulate -L zsh
  _termbay_ran=1
  _termbay_prompt_due=0
  _termbay_osc exec
  builtin print -rn -- $'\e]133;C\a'
}

# Stand-in for `compadd` while the widget runs: records the matches zsh
# would offer instead of adding them. Option parsing and capture adapted
# from fzf-tab's `-ftb-compadd` (MIT, see the notice at the top).
_termbay_compadd() {
  local -A _tb_o
  # Every option of compadd, so option values are not taken for words.
  if ! builtin zparseopts -A _tb_o P: p: d: X: O: A: D: f i: S: s: I: x: r: R: W: F: M: E: \
      q e Q n U C J: V: a l k o:: 1 2 2>/dev/null; then
    builtin compadd "$@"
    return
  fi
  # Calls that only fill arrays (used by completion functions themselves)
  # or only show a message are passed on.
  if (( ${+_tb_o[-O]} || ${+_tb_o[-A]} || ${+_tb_o[-D]} )); then
    builtin compadd "$@"
    return
  fi
  local -a _tb_hits _tb_descs
  if (( ${+_tb_o[-d]} )); then
    _tb_descs=( "${(@P)_tb_o[-d]}" )
  fi
  builtin compadd -A _tb_hits -D _tb_descs "$@"
  local -i ret=$?
  (( ${#_tb_hits} )) || return ret

  local pre="${IPREFIX}${_tb_o[-i]-}${_tb_o[-p]-}${_tb_o[-P]-}"
  local suf="${_tb_o[-S]-}"
  [[ $suf == [[:space:]]# ]] && suf=''
  local fdir="${_tb_o[-W]-}"
  [[ -n $fdir && $fdir != */ ]] && fdir+=/
  local word ins desc end
  local -i i
  for (( i = 1; i <= ${#_tb_hits}; i++ )); do
    (( ${#_termbay_items} >= 1500 )) && break
    word=${_tb_hits[i]}
    if (( ${+_tb_o[-Q]} )); then
      ins="$pre$word"
    else
      ins="$pre${(q)word}"
    fi
    end=$suf
    if [[ -z $end ]] && (( ${+_tb_o[-f]} )); then
      if (( ${+_tb_o[-Q]} )); then
        [[ -d "$fdir${(Q)word}" ]] && end=/
      else
        [[ -d "$fdir$word" ]] && end=/
      fi
    fi
    [[ $end == / && $ins == */ ]] && end=''
    desc=${_tb_descs[i]-}
    if [[ $desc == *' -- '* ]]; then
      desc=${desc#* -- }
    elif [[ $desc == "$word" ]]; then
      desc=''
    fi
    # With -Q the words (and prefixes) come quoted; show them unquoted.
    if (( ${+_tb_o[-Q]} )); then
      _termbay_items+=( "${(Q)pre}${(Q)word}$end" "$ins$end" "$desc" )
    else
      _termbay_items+=( "$pre$word$end" "$ins$end" "$desc" )
    fi
  done
  # Tell zsh this completer found matches, as it would without us.
  builtin compadd -U -Q -S '' -- ''
  return 0
}

# Completion widget: runs zsh's completion system with `compadd` captured,
# then makes sure nothing is inserted or listed.
#
# The context gets the name of this widget, so a matcher-list can apply to
# this call only: exact matches first and, when there are none, matches
# that ignore case ("pro" finds "Projects"). The style is removed right
# after, so the user's normal Tab never sees it.
_termbay_compfunc() {
  local _termbay_saved="${functions[compadd]-}"
  local -i _termbay_had=${+functions[compadd]}
  local curcontext=_termbay_compwidget:::
  functions[compadd]=${functions[_termbay_compadd]}
  zstyle ':completion:_termbay_compwidget:*' matcher-list '' 'm:{[:lower:][:upper:]}={[:upper:][:lower:]}'
  {
    _main_complete
  } always {
    zstyle -d ':completion:_termbay_compwidget:*' matcher-list
    if (( _termbay_had )); then
      functions[compadd]=$_termbay_saved
    else
      unfunction compadd 2>/dev/null
    fi
  }
  # A silent placeholder match keeps zle from beeping when nothing matched.
  builtin compadd -U -Q -S '' -- ''
  compstate[insert]=''
  compstate[list]=''
}

# Runs on U+10FFFD. Completion must run with the user's options, so no
# `emulate` here.
_termbay_complete() {
  local _termbay_rq=$_termbay_req
  _termbay_req=''
  local _termbay_mode=insert
  [[ $KEYMAP == vicmd ]] && _termbay_mode=command
  local -a _termbay_items
  if [[ $_termbay_mode == insert ]]; then
    if (( ${+functions[compdef]} )); then
      zle _termbay_compwidget 2>/dev/null
    else
      _termbay_fallback
    fi
  fi
  _termbay_reply
}

# Without the completion system (no `compinit` in the user's .zshrc), zsh's
# own globbing lists files and folders, and the first word of a command
# also gets commands, aliases, builtins and functions. Nothing is run and
# the user's Tab is not changed. Case is ignored; the core puts exact
# prefixes first.
_termbay_fallback() {
  emulate -L zsh
  setopt extended_glob null_glob no_nomatch
  local -a words
  words=( ${(z)LBUFFER} )
  local cur='' prev=''
  if [[ -n $LBUFFER && $LBUFFER != *[[:space:]] ]]; then
    cur=${words[-1]}
    (( ${#words} > 1 )) && prev=${words[-2]}
  else
    (( ${#words} )) && prev=${words[-1]}
  fi
  local word=${(Q)cur}
  # `~` and `~/...` are the only expansions done (no variables, no commands).
  local shown_dir='' real_dir='' base=$word
  if [[ $word == */* ]]; then
    shown_dir=${word%/*}/
    base=${word##*/}
  elif [[ $word == '~' ]]; then
    shown_dir='~/'
    base=''
  fi
  real_dir=$shown_dir
  [[ $real_dir == '~/'* ]] && real_dir=$HOME/${real_dir#'~/'}
  [[ $real_dir == '~'* ]] && return 0
  local ins_dir=''
  if [[ $shown_dir == '~/'* ]]; then
    ins_dir='~/'
    [[ -n ${shown_dir#'~/'} ]] && ins_dir+=${(q)shown_dir#'~/'}
  elif [[ -n $shown_dir ]]; then
    ins_dir=${(q)shown_dir}
  fi

  local -i n=0
  local name end
  local pat="${(b)real_dir}(#i)${(b)base}*"
  [[ $base == .* ]] && setopt glob_dots
  local -a found
  found=( ${~pat}(N) )
  for name in "${found[@]}"; do
    (( n++ >= 500 )) && break
    end=''
    [[ -d $name ]] && end=/
    name=${name:t}
    _termbay_items+=( "$shown_dir$name$end" "$ins_dir${(q)name}$end" '' )
  done

  # Command position: first word, or after a separator.
  if [[ $word != */* && $word != '~'* ]] &&
     [[ -z $prev || $prev == (';'|'|'|'||'|'&&'|'&'|'|&'|'('|'{'|sudo|command|exec|nohup|time) ]]; then
    local -aU cmds
    local cpat="(#i)${(b)base}*"
    cmds=( ${(M)${(k)aliases}:#${~cpat}} ${(M)${(k)builtins}:#${~cpat}}
           ${(M)${(k)functions}:#${~cpat}} ${(M)${(k)commands}:#${~cpat}} )
    cmds=( ${cmds:#_*} )
    for name in ${(o)cmds}; do
      (( n++ >= 500 )) && break
      _termbay_items+=( "$name" "${(q)name}" '' )
    done
  fi
}

# Sends the answer: line, cursor, start of the current word and items.
_termbay_reply() {
  emulate -L zsh
  local -a words
  words=( ${(z)LBUFFER} )
  local cur=''
  if [[ -n $LBUFFER && $LBUFFER != *[[:space:]] ]]; then
    cur=${words[-1]}
  fi
  local -i from=$(( CURSOR - ${#cur} ))
  (( from < 0 )) && from=0
  local out="$_termbay_nonce;complete;$_termbay_rq;$_termbay_mode"
  _termbay_enc "$BUFFER"
  out+=";$REPLY;$CURSOR;$from"
  local item a b c
  local -i i
  for (( i = 1; i + 2 <= ${#_termbay_items}; i += 3 )); do
    _termbay_enc "${_termbay_items[i]}"; a=$REPLY
    _termbay_enc "${_termbay_items[i+1]}"; b=$REPLY
    _termbay_enc "${_termbay_items[i+2]}"; c=$REPLY
    item=";$a;$b;$c"
    (( ${#out} + ${#item} > 65536 )) && break
    out+=$item
  done
  builtin print -rn -- $'\e]6973;'"$out"$'\a'
}

typeset -ga precmd_functions preexec_functions
precmd_functions+=(_termbay_precmd)
preexec_functions+=(_termbay_preexec)
