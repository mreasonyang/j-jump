if [ -z "${ZSH_VERSION-}" ]; then
  printf '%s\n' 'J2: this is the zsh integration; generate init for your current shell' >&2
  (exit 2)
elif [ "${__jj_installed-}" != '__JJ_CMD__' ] && { type '__JJ_CMD__' >/dev/null 2>&1 || type '__JJ_CMD__i' >/dev/null 2>&1; }; then
  printf '%s\n' 'J2: command conflict; use jjump init zsh --cmd NAME' >&2
  (exit 2)
else
typeset -g __jj_installed='__JJ_CMD__'
function __jj_resolve {
  emulate -L zsh
  local frame rc
  frame=$(command jjump query --setup-if-needed "$@"; rc=$?; printf '\001'; exit "$rc")
  rc=$?
  ((rc == 0)) || return $rc
  [[ $frame == *$'\001' ]] || return 6
  frame=${frame%$'\001'}
  [[ $frame == *$'\n' ]] || return 6
  frame=${frame%$'\n'}
  [[ $frame == /* && $frame != *$'\n'* && $frame != *$'\r'* ]] || return 6
  __jj_path=$frame
}
function __JJ_CMD__ {
  if [ "$#" -eq 1 ] && { [ "$1" = --help ] || [ "$1" = -h ]; }; then
    command jjump shell-help __JJ_CMD__
    return $?
  fi
  if (( $# == 1 )) && [[ $1 == '-' ]]; then
    command jjump setup-if-needed >&2 || return $?
    builtin cd -- -; return $?
  fi
  local __jj_path
  __jj_resolve "$@" || return $?
  builtin cd -- "$__jj_path"
}
function __JJ_CMD__i {
  if [ "$#" -eq 1 ] && { [ "$1" = --help ] || [ "$1" = -h ]; }; then
    command jjump shell-help __JJ_CMD__i --interactive
    return $?
  fi
  local __jj_path
  __jj_resolve --interactive "$@" || return $?
  builtin cd -- "$__jj_path"
}
function __jj_prompt {
  local rc=$?
  if [[ ${__jj_last_pwd-} != $PWD ]]; then
    if [ -n "${__jj_record_pid-}" ] && kill -0 "$__jj_record_pid" 2>/dev/null; then
      local __jj_attempt
      for __jj_attempt in 1 2 3 4; do
        command sleep 0.005 2>/dev/null || break
        kill -0 "$__jj_record_pid" 2>/dev/null || break
      done
      if kill -0 "$__jj_record_pid" 2>/dev/null; then return "$rc"; fi
    fi
    __jj_record_pid=$(command env -u TYPESAFE_API_KEY jjump observe -- "$PWD" 2>/dev/null)
    [[ -z $__jj_record_pid ]] || typeset -g __jj_last_pwd=$PWD
  fi
  return $rc
}
function __jj_complete_widget {
  emulate -L zsh
  local -a words
  words=(${(z)LBUFFER})
  if [[ ${words[1]-} == '__JJ_CMD__' && $LBUFFER == *' ' ]]; then
    local __jj_path
    local query=${(j: :)words[2,-1]}
    __jj_resolve --complete "$query" < /dev/tty || { zle redisplay; return 0; }
    LBUFFER='__JJ_CMD__ -- '${(q)__jj_path}' '
    RBUFFER=''
    zle redisplay
  else
    zle "${__jj_previous_tab:-expand-or-complete}"
  fi
}
if [[ -o interactive ]]; then
  typeset -ga precmd_functions
  (( ${precmd_functions[(Ie)__jj_prompt]} )) || precmd_functions+=(__jj_prompt)
  if [[ -z ${__jj_previous_tab-} ]]; then
    # zsh does not apply a modifier like ${...##* } to a command substitution
    # nested inside ${...}: `typeset -g x=${$(bindkey '^I')##* }` stores the
    # whole `"^I" expand-or-complete' string, and every later Tab then fails
    # with `No such widget'. Capture the binding first, then strip the key.
    typeset -g __jj_previous_tab
    __jj_previous_tab=$(bindkey '^I' 2>/dev/null)
    __jj_previous_tab=${__jj_previous_tab##* }
    # Only a widget that actually exists may be handed to zle: our own widget,
    # an empty capture, an unbound key, a macro body or any other leftover text
    # falls back to the stock completion widget instead of raising a diagnostic.
    if [[ $__jj_previous_tab == __jj_complete_widget ]] ||
      (( ! ${${(f)"$(zle -la 2>/dev/null)"}[(Ie)${__jj_previous_tab}]} )); then
      __jj_previous_tab=expand-or-complete
    fi
  fi
  zle -N __jj_complete_widget
  bindkey '^I' __jj_complete_widget
fi

fi
