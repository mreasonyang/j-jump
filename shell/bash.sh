if [ -z "${BASH_VERSION-}" ]; then
  printf '%s\n' 'J2: this is the bash integration; generate init for your current shell' >&2
  (exit 2)
elif [ "${__jj_installed-}" != '__JJ_CMD__' ] && { type '__JJ_CMD__' >/dev/null 2>&1 || type '__JJ_CMD__i' >/dev/null 2>&1; }; then
  printf '%s\n' 'J2: command conflict; use jjump init bash --cmd NAME' >&2
  (exit 2)
else
__jj_installed='__JJ_CMD__'
function __jj_resolve {
  local frame rc
  frame=$(command jjump query --setup-if-needed "$@"; rc=$?; printf '\001'; exit "$rc")
  rc=$?
  [ "$rc" -eq 0 ] || return "$rc"
  case "$frame" in *$'\001') frame=${frame%$'\001'};; *) return 6;; esac
  case "$frame" in *$'\n') frame=${frame%$'\n'};; *) return 6;; esac
  case "$frame" in /*) ;; *) return 6;; esac
  case "$frame" in *$'\n'*|*$'\r'*) return 6;; esac
  __jj_path=$frame
}
function __JJ_CMD__ {
  if [ "$#" -eq 1 ] && { [ "$1" = --help ] || [ "$1" = -h ]; }; then
    command jjump shell-help __JJ_CMD__
    return $?
  fi
  if [ "$#" -eq 1 ] && [ "$1" = '-' ]; then
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
  __jj_restore_completion
  if [ "${__jj_last_pwd-}" != "$PWD" ]; then
    if [ -n "${__jj_record_pid-}" ] && kill -0 "$__jj_record_pid" 2>/dev/null; then
      local __jj_attempt
      for __jj_attempt in 1 2 3 4; do
        command sleep 0.005 2>/dev/null || break
        kill -0 "$__jj_record_pid" 2>/dev/null || break
      done
      if kill -0 "$__jj_record_pid" 2>/dev/null; then return "$rc"; fi
    fi
    __jj_record_pid=$(command env -u TYPESAFE_API_KEY jjump observe -- "$PWD" 2>/dev/null)
    [ -z "$__jj_record_pid" ] || __jj_last_pwd=$PWD
  fi
  return "$rc"
}
# Device-status reply applies a Readline macro, including on Bash 3.2.
# The macro inserts shell-quoted text only; it never accepts/executes the line.
function __jj_restore_completion {
  if [ "${__jj_completion_pending-}" = 1 ]; then
    bind -r '\e[0n'
    if [ -n "${__jj_saved_reply_command-}" ]; then
      bind -x "$__jj_saved_reply_command"
    else
      [ -z "${__jj_saved_reply_binding-}" ] || bind "$__jj_saved_reply_binding"
    fi
    unset __jj_completion_pending __jj_saved_reply_binding __jj_saved_reply_command
  fi
}
function __jj_complete {
  local __jj_path q item quoted macro line_length=${#COMP_LINE}
  # Bash 4.3+ reports characters in the current locale; Bash 3.2/4.2 uses bytes.
  # Capture the character length before switching to byte-safe path quoting.
  local LC_ALL=C
  if (( BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 3) )); then
    line_length=${#COMP_LINE}
  fi
  COMPREPLY=()
  [ "$COMP_POINT" -eq "$line_length" ] || return 0
  case "$COMP_LINE" in *' ') ;; *)
    while IFS= read -r item; do COMPREPLY+=("$item"); done < <(compgen -d -- "${COMP_WORDS[COMP_CWORD]}")
    return 0;;
  esac
  q=${COMP_WORDS[*]:1}
  __jj_resolve --complete "$q" < /dev/tty || return 0
  __jj_restore_completion
  __jj_saved_reply_binding=$( { bind -s; bind -p; } | command grep -F -e '"\e[0n":' -e '"\M-[0n":' | command head -n 1)
  __jj_saved_reply_command=$(bind -X 2>/dev/null | command grep -F -e '"\e[0n":' -e '"\M-[0n":' | command head -n 1)
  printf -v quoted '%q' "$__jj_path"
  macro="${COMP_WORDS[0]} -- $quoted "
  macro=${macro//\\/\\\\}
  macro=${macro//\"/\\\"}
  bind '"\e[0n": "\C-u'"$macro"'"'
  __jj_completion_pending=1
  printf '\033[5n' > /dev/tty
}

case $- in *i*)
  if declare -p PROMPT_COMMAND 2>/dev/null | command grep -q 'declare -a'; then
    case " ${PROMPT_COMMAND[*]} " in *' __jj_prompt '*) ;; *) PROMPT_COMMAND=(__jj_prompt "${PROMPT_COMMAND[@]}");; esac
  else
    case ";${PROMPT_COMMAND-};" in *';__jj_prompt;'*) ;; *) PROMPT_COMMAND="__jj_prompt${PROMPT_COMMAND:+;${PROMPT_COMMAND}}";; esac
  fi
  complete -o filenames -F __jj_complete '__JJ_CMD__' '__JJ_CMD__i'
;; esac

fi
