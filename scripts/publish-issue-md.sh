#!/usr/bin/env bash
set -euo pipefail

TEMP_DIR=""

usage() {
    cat <<'EOF'
Usage: scripts/publish-issue-md.sh [--repo OWNER/REPO] [--dry-run] [FILE ...]

Publish GitHub issue drafts from Markdown files with the gh CLI.

Rules:
- The first level-1 heading (# Title) becomes the issue title.
- The remainder of the file becomes the issue body.
- If no files are supplied, the script publishes every Markdown file in
  docs/draft-issues/ except README.md.

Examples:
  scripts/publish-issue-md.sh docs/draft-issues/028-add-content-security-policy-header-to-web-gateway.md
  scripts/publish-issue-md.sh --repo leynos/axinite docs/draft-issues/*.md
  scripts/publish-issue-md.sh --dry-run
EOF
}

require_command() {
    local command_name="$1"
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "publish-issue-md: required command not found: $command_name" >&2
        exit 1
    fi
}

extract_title() {
    local file_path="$1"
    local title

    title=$(sed -n 's/^# //p' "$file_path" | head -n1)

    if [[ -z "$title" ]]; then
        echo "publish-issue-md: missing level-1 heading in $file_path" >&2
        exit 1
    fi

    printf '%s\n' "$title"
}

write_body_file() {
    local source_file="$1"
    local destination_file="$2"

    awk '
        BEGIN {
            seen_title = 0
        }
        /^# / && seen_title == 0 {
            seen_title = 1
            next
        }
        seen_title == 1 {
            print
        }
    ' "$source_file" >"$destination_file"

    python - "$destination_file" <<'PY'
from pathlib import Path
import sys

body_path = Path(sys.argv[1])
body = body_path.read_text(encoding="utf-8")
body = body.lstrip("\n")
body_path.write_text(body, encoding="utf-8")
PY
}

main() {
    require_command gh

    local repo=""
    local dry_run=false
    local -a files=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --repo)
                if [[ $# -lt 2 ]]; then
                    echo "publish-issue-md: --repo requires OWNER/REPO" >&2
                    exit 1
                fi
                repo="$2"
                shift 2
                ;;
            --dry-run)
                dry_run=true
                shift
                ;;
            --help|-h)
                usage
                exit 0
                ;;
            --)
                shift
                files+=("$@")
                break
                ;;
            -*)
                echo "publish-issue-md: unknown option: $1" >&2
                usage >&2
                exit 1
                ;;
            *)
                files+=("$1")
                shift
                ;;
        esac
    done

    if [[ ${#files[@]} -eq 0 ]]; then
        mapfile -t files < <(
            find docs/draft-issues -maxdepth 1 -type f -name '*.md' ! -name 'README.md' | sort
        )
    fi

    if [[ ${#files[@]} -eq 0 ]]; then
        echo "publish-issue-md: no Markdown issue drafts found" >&2
        exit 1
    fi

    TEMP_DIR=$(mktemp -d)
    trap '[[ -n "${TEMP_DIR:-}" ]] && rm -rf -- "${TEMP_DIR}"' EXIT

    local file_path
    for file_path in "${files[@]}"; do
        if [[ ! -f "$file_path" ]]; then
            echo "publish-issue-md: file not found: $file_path" >&2
            exit 1
        fi

        local title
        title=$(extract_title "$file_path")

        local body_file
        body_file="$TEMP_DIR/$(basename "$file_path").body.md"
        write_body_file "$file_path" "$body_file"

        local -a command=(gh issue create --title "$title" --body-file "$body_file")

        if [[ -n "$repo" ]]; then
            command+=(--repo "$repo")
        fi

        echo "Publishing $file_path"
        echo "Title: $title"

        if [[ "$dry_run" == true ]]; then
            printf 'Command:'
            printf ' %q' "${command[@]}"
            printf '\n'
            continue
        fi

        "${command[@]}"
    done
}

main "$@"
