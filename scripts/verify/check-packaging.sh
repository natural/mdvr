#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
app=${1:-$root/packaging/build/mdvr.app}

[ "$#" -le 1 ] || {
    printf 'Usage: sh scripts/verify/check-packaging.sh [PATH_TO_APP]\n' >&2
    exit 2
}

sh "$root/packaging/inspect-app.sh" "$app"
