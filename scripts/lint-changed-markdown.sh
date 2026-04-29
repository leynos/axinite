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

# shellcheck disable=SC2086
"$bunx_cmd" markdownlint-cli2 $files
