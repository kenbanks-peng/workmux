# Source from interactive Bash shells so Yazi can change the shell's directory.
f() {
    local tmp cwd status
    tmp="$(mktemp -t yazi-cwd.XXXXXX)" || return
    command yazi "$@" --cwd-file="$tmp"
    status=$?
    cwd="$(< "$tmp")"
    rm -f -- "$tmp"
    if [ -n "$cwd" ] && [ "$cwd" != "$PWD" ]; then
        builtin cd -- "$cwd" || return
    fi
    return "$status"
}
