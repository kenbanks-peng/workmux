#!/bin/sh
set -eu

config_home="${XDG_CONFIG_HOME:-${HOME}/.config}"
config_dir="$config_home/${NVIM_APPNAME:-nvim}"
if [ ! -e "$config_dir" ]; then
    mkdir -p "$config_home"
    cp -R /opt/lazyvim-starter "$config_dir"
fi

exec /opt/nvim/bin/nvim "$@"
