#!/usr/bin/env sh
set -eu

if [ "$#" -ne 1 ]; then
	echo "usage: $0 <bunx_cmd>" >&2
	exit 2
fi

bunx_cmd=$1
base_ref=$(git merge-base origin/main HEAD || true)

if [ -z "$base_ref" ]; then
	echo "origin/main is unavailable; fetch it before running markdownlint." >&2
	exit 1
fi

files=$(git diff --name-only "$base_ref"...HEAD -- '*.md')

if [ -z "$files" ]; then
	echo "No changed Markdown files to lint."
	exit 0
fi

set -f
old_ifs=$IFS
IFS='
'
set -- $files
IFS=$old_ifs
set +f

"$bunx_cmd" markdownlint-cli2 "$@"
