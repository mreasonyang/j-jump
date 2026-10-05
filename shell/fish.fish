if not set -q __jj_installed; or test "$__jj_installed" != '__JJ_CMD__'
    if type -q __JJ_CMD__; or type -q __JJ_CMD__i; or abbr --query __JJ_CMD__; or abbr --query __JJ_CMD__i
        echo 'J2: command conflict; use jjump init fish --cmd NAME' >&2
        return 2
    end
end
set -g __jj_installed __JJ_CMD__
# Native-history adapter: all navigation uses builtin cd, never user wrappers.
# dirprev/dirnext and direction are fish's existing history, not a J-Jump store.
function __jj_native_cd
    set -l before "$PWD"
    if test (count $argv) -eq 1; and test "$argv[1]" = '-'
        set -l src dirprev
        set -l dest dirnext
        set -l direction next
        if test "$__fish_cd_direction" = next
            set src dirnext
            set dest dirprev
            set direction prev
        end
        set -l jj_history $$src
        test (count $jj_history) -gt 0; or return 1
        builtin cd -- "$jj_history[-1]"; or return $status
        set -g $dest $$dest "$before"
        set -e $src\[(count $jj_history)\]
        set -g __fish_cd_direction $direction
        return 0
    end
    builtin cd $argv; or return $status
    if test "$PWD" != "$before"
        if test (count $dirprev) -ge 25
            set -e dirprev[1]
        end
        if set -U -q dirprev
            set -U -a dirprev "$before"
        else
            set -g -a dirprev "$before"
        end
        set -e dirnext
        if set -U -q __fish_cd_direction
            set -U __fish_cd_direction prev
        else
            set -g __fish_cd_direction prev
        end
    end
    return 0
end
function __jj_resolve
    # Check the function's descriptors before Fish creates command substitution.
    # Its nested substitution can otherwise inherit the terminal stderr.
    set -l setup_option
    if isatty stdin; and isatty stderr
        set setup_option --setup-if-needed
    end
    set -l output (begin; command jjump query $setup_option $argv; printf '\001%d' $status; end | string collect --allow-empty --no-trim-newlines)
    set -l rc (string match -r --groups-only '\x01([0-9]+)$' -- "$output")
    test (count $rc) -eq 1; or return 6
    test "$rc" -eq 0; or return $rc
    set -l parsed (string match -r --groups-only '^(/[^\r\n]*)\n\x010$' -- "$output")
    test (count $parsed) -eq 1; or return 6
    set -g __jj_path "$parsed[1]"
end
function __JJ_CMD__
    if test (count $argv) -eq 1; and contains -- "$argv[1]" --help -h
        command jjump shell-help __JJ_CMD__
        return $status
    end
    functions -q __jj_native_cd; or begin; echo 'J5: native fish cd unavailable' >&2; return 5; end
    if test (count $argv) -eq 1; and test "$argv[1]" = '-'
        if isatty stdin; and isatty stderr
            command jjump setup-if-needed >&2; or return $status
        end
        __jj_native_cd -
        return $status
    end
    __jj_resolve $argv; or return $status
    __jj_native_cd -- "$__jj_path"
end
function __JJ_CMD__i
    if test (count $argv) -eq 1; and contains -- "$argv[1]" --help -h
        command jjump shell-help __JJ_CMD__i --interactive
        return $status
    end
    __jj_resolve --interactive $argv; or return $status
    __jj_native_cd -- "$__jj_path"
end
if status is-interactive
    function __jj_prompt --on-event fish_prompt
        set -l rc $status
        if not set -q __jj_last_pwd; or test "$__jj_last_pwd" != "$PWD"
            if set -q __jj_record_pid[1]; and kill -0 $__jj_record_pid[1] 2>/dev/null
                for __jj_attempt in 1 2 3 4
                    command sleep 0.005 2>/dev/null; or break
                    kill -0 $__jj_record_pid[1] 2>/dev/null; or break
                end
                if kill -0 $__jj_record_pid[1] 2>/dev/null
                    return $rc
                end
            end
            set -g __jj_record_pid (command env -u TYPESAFE_API_KEY jjump observe -- "$PWD" 2>/dev/null)
            if set -q __jj_record_pid[1]
                set -g __jj_last_pwd "$PWD"
            end
        end
        return $rc
    end
    function __jj_tab
        set -l parts (commandline -opc)
        if test (count $parts) -ge 1; and test "$parts[1]" = '__JJ_CMD__'; and string match -rq ' $' -- (commandline)
            __jj_resolve --complete (string join ' ' -- $parts[2..-1]); or return 0
            commandline --replace '__JJ_CMD__ -- '(string escape -- "$__jj_path")' '
        else
            commandline -f complete
        end
    end
    bind \t __jj_tab
end
