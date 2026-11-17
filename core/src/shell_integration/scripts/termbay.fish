# termbay shell integration for fish (protocol 1).
# Part of termbay, MIT License. https://github.com/termbay/termbay
#
# termbay runs `fish --init-command 'source <this file>'` only when
# `[autocomplete] enabled = true`. It runs after the user's config.fish,
# never writes any file and never runs the command line. It emits:
#
#   - OSC 7 (working folder), and OSC 133 A, C and D on fish 3 (fish 4
#     emits OSC 133 itself);
#   - termbay's private OSC 6973, always with the session nonce:
#       ready;<protocol>;fish;<version>;<history file>   once, at the first prompt
#       prompt;<exit code, or empty>                      before every prompt
#       exec                                              before every command
#       complete;<request>;<insert|command>;<line>;<cursor>;<word start>;
#                [<text>;<insert>;<description>]...       answer to a request
#     Fields are escaped: "\" as "\x5C", ";" as "\x3B" and every byte that
#     is not plain ASCII as "\xHH" (URL escaping with "\x" instead of "%").
#     Positions count characters.
#
# termbay asks for completions only when the user presses the trigger key
# at the prompt. It types private-use characters no keyboard sends:
# U+10FFF0..U+10FFF9 add a digit to the request number and U+10FFFD runs
# `complete -C` on the line up to the cursor. The answer goes out as the
# private OSC; the command line and the screen are left untouched.
#
# Supported: fish 3.3 or newer (Ubuntu 22.04 ships 3.3.1). Older versions
# get no integration (no handshake, so the menu never shows). fish 3.3 has
# no `complete --escape` (new in 3.4): there the candidates are escaped with
# `string escape`, and a name with a line break comes out split, as fish
# 3.3 prints it.
#
# This file is termbay's own code. OSC 7 and OSC 133 (FinalTerm semantic
# prompts) are public protocols; nothing here comes from Warp, iTerm2 or
# kitty.

set -l __termbay_major (string replace -r '^(\d+).*$' '$1' -- $version)
set -l __termbay_minor (string replace -r '^\d+\.(\d+).*$' '$1' -- $version)
set -l __termbay_supported 0
if string match -qr '^\d+$' -- "$__termbay_major"; and string match -qr '^\d+$' -- "$__termbay_minor"
    if test $__termbay_major -gt 3; or test $__termbay_major -eq 3 -a $__termbay_minor -ge 3
        set __termbay_supported 1
    end
end

if set -q TERMBAY_NONCE[1]; and status is-interactive; and test $__termbay_supported = 1
    set -g __termbay_nonce $TERMBAY_NONCE
    set -e TERMBAY_NONCE
    set -g __termbay_req ''
    set -g __termbay_ran 0
    set -g __termbay_status ''
    set -g __termbay_emit133 0
    test $__termbay_major -lt 4; and set -g __termbay_emit133 1
    # `complete --escape` exists since fish 3.4. TERMBAY_TEST_FISH_NO_ESCAPE
    # forces the fish 3.3 path (used by termbay's tests).
    set -g __termbay_escape 1
    if test $__termbay_major -eq 3 -a $__termbay_minor -lt 4; or set -q TERMBAY_TEST_FISH_NO_ESCAPE
        set -g __termbay_escape 0
    end

    # Escapes each argument, one per line (no argument: no output).
    function __termbay_enc
        set -q argv[1]; or return 0
        string escape --style=url -- $argv | string replace -a -- % '\\x'
    end

    function __termbay_osc
        set -l fields (__termbay_enc $argv)
        printf '\e]6973;%s;%s\a' $__termbay_nonce (string join ';' -- $fields)
    end

    function __termbay_histfile
        set -q fish_private_mode[1]; and return 0
        set -l name fish
        set -q fish_history; and set name $fish_history
        test -n "$name"; or return 0
        set -l dir $__fish_user_data_dir
        if test -z "$dir"
            if set -q XDG_DATA_HOME[1]
                set dir $XDG_DATA_HOME/fish
            else
                set dir ~/.local/share/fish
            end
        end
        echo $dir/$name"_history"
    end

    function __termbay_digit -a digit
        if test (string length -- $__termbay_req) -lt 20
            set -g __termbay_req "$__termbay_req$digit"
        end
    end

    # Binds the trigger in the modes of the emacs and vi key bindings.
    # Checked before every prompt: changing the key bindings erases them.
    function __termbay_bind_keys
        bind \U10FFFD >/dev/null 2>&1; and return 0
        for mode in default insert visual replace replace_one
            bind -M $mode \U10FFF0 '__termbay_digit 0'
            bind -M $mode \U10FFF1 '__termbay_digit 1'
            bind -M $mode \U10FFF2 '__termbay_digit 2'
            bind -M $mode \U10FFF3 '__termbay_digit 3'
            bind -M $mode \U10FFF4 '__termbay_digit 4'
            bind -M $mode \U10FFF5 '__termbay_digit 5'
            bind -M $mode \U10FFF6 '__termbay_digit 6'
            bind -M $mode \U10FFF7 '__termbay_digit 7'
            bind -M $mode \U10FFF8 '__termbay_digit 8'
            bind -M $mode \U10FFF9 '__termbay_digit 9'
            bind -M $mode \U10FFFD __termbay_complete
        end
    end

    function __termbay_complete
        set -l req $__termbay_req
        set -g __termbay_req ''
        # In vi normal or visual mode, typed text would run editor commands.
        set -l mode insert
        if test "$fish_bind_mode" != insert -a "$fish_bind_mode" != default
            set mode command
        else if test "$fish_bind_mode" = default
            # "default" is vi normal mode when the vi bindings are in use.
            # Ask fish which bindings are active; only for custom binding
            # functions fall back to looking for other insert mode bindings
            # (fish 4.9 defines insert mode bindings even for emacs).
            if string match -q -- '*vi*' $fish_key_bindings
                set mode command
            else if test "$fish_key_bindings" != fish_default_key_bindings
                and bind -M insert 2>/dev/null | string match -q -v -- '*__termbay_*'
                set mode command
            end
        end

        set -l line (commandline | string collect)
        set -l cursor (commandline -C)
        set -l token (commandline -ct | string collect)
        set -l from (math "max(0, $cursor - "(string length -- "$token")")")
        set -l fields complete $req $mode "$line" $cursor $from

        if test $mode = insert
            set -l before (commandline -cp | string collect)
            set -l found
            if test $__termbay_escape = 1
                set found (complete --escape --do-complete="$before")
            else
                set found (complete --do-complete="$before")
            end
            set -l count 0
            set -l size 0
            for candidate in $found
                set count (math $count + 1)
                test $count -le 500; or break
                set -l parts (string split -m1 \t -- $candidate)
                set -l insert $parts[1]
                set -l description ''
                set -q parts[2]; and set description $parts[2]
                set -l text $insert
                if test $__termbay_escape = 0
                    # Unescaped candidate: escape it like --escape would,
                    # keeping a leading "~/" as it is.
                    if string match -q -- '~/*' $text
                        set insert '~/'(string escape -n -- (string sub -s 3 -- $text))
                    else
                        set insert (string escape -n -- $text)
                    end
                else if string match -q -r '[\\\\\'"]' -- $insert
                    set text (string unescape -- $insert | string collect)
                end
                set size (math $size + (string length -- "$text$insert$description"))
                test $size -le 60000; or break
                set -a fields "$text" "$insert" "$description"
            end
        end
        __termbay_osc $fields
    end

    function __termbay_on_preexec --on-event fish_preexec
        set -g __termbay_ran 1
        __termbay_osc exec
        test $__termbay_emit133 = 1; and printf '\e]133;C\a'
    end

    function __termbay_on_postexec --on-event fish_postexec
        set -g __termbay_status $status
    end

    function __termbay_on_prompt --on-event fish_prompt
        set -l code ''
        if test $__termbay_ran = 1
            set code $__termbay_status
            test $__termbay_emit133 = 1; and printf '\e]133;D;%s\a' $code
        end
        __termbay_bind_keys
        if not set -q __termbay_ready
            set -g __termbay_ready 1
            set -l histfile (__termbay_histfile)
            __termbay_osc ready 1 fish $version "$histfile"
        end
        printf '\e]7;file://%s%s\a' $hostname (string escape --style=url -- $PWD)
        __termbay_osc prompt "$code"
        set -g __termbay_ran 0
        test $__termbay_emit133 = 1; and printf '\e]133;A\a'
    end
end
set -e TERMBAY_NONCE
