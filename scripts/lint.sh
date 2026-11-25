#!/bin/sh
set -eu

repo_root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
docs_paths=$(git -C "$repo_root" ls-files --cached --others --exclude-standard -- ':(top,literal)docs')

if [ -n "$docs_paths" ]; then
    printf '%s\n' 'Do not create files under docs/. Agent-generated docs are not useful. They go stale and mislead agents.' "$docs_paths" >&2
    exit 1
fi
