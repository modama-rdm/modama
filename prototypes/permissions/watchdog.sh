#!/usr/bin/env bash

# Based on
# https://unix.stackexchange.com/questions/209009/set-sticky-bit-by-default-for-new-directories-via-acl

#set -x
set -e

inotifywait -m -r -e create /data/ |
while read -r path event file; do
    echo "$path" "$event" "$file"
    if [[ $path != /data/sharing/* ]] ; then
        case "$event" in
            *ISDIR*)
                echo "Watchdog: Add sticky bit $path$file"
                chmod +t "$path$file"
                ;;
            *)
                echo "Watchdog: Remove group write permission $path$file"
                setfacl --remove-all --remove-default "$path$file"
                chmod g-w "$path$file"
                ;;
        esac
    fi
done