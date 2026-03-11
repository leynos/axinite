#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

cat >"$workdir/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

case "$*" in
  api\ repos/test/ironclaw/pulls/1/files\ --paginate\ --jq*)
    printf '5156\n5373\n'
    ;;
  pr\ view\ 1\ --repo\ test/ironclaw\ --json\ labels\ --jq\ .labels\[\].name)
    ;;
  pr\ view\ 1\ --repo\ test/ironclaw\ --json\ author\ --jq\ .author.login)
    printf 'contributor\n'
    ;;
  pr\ list\ --repo\ test/ironclaw\ --state\ merged\ --author\ contributor\ --limit\ 100\ --json\ number\ --jq\ length)
    printf '0\n'
    ;;
  pr\ edit\ 1\ --repo\ test/ironclaw\ --add-label\ size:\ XL)
    printf 'ADD:%s\n' "size: XL" >>"$GH_LOG"
    ;;
  pr\ edit\ 1\ --repo\ test/ironclaw\ --add-label\ risk:\ low)
    printf 'ADD:%s\n' "risk: low" >>"$GH_LOG"
    ;;
  pr\ edit\ 1\ --repo\ test/ironclaw\ --add-label\ contributor:\ new)
    printf 'ADD:%s\n' "contributor: new" >>"$GH_LOG"
    ;;
  *)
    printf 'unexpected gh invocation: %s\n' "$*" >&2
    exit 1
    ;;
esac
EOF
chmod +x "$workdir/gh"

PATH="$workdir:$PATH"
GH_LOG="$workdir/gh.log"
export GH_LOG
export PR_NUMBER=1
export REPO=test/ironclaw

output="$("$repo_root/.github/scripts/pr-labeler.sh" 2>&1)"

printf '%s\n' "$output"

grep -Fq 'Size: 10529 changed lines -> size: XL' <<<"$output"
grep -Fq 'Risk: low' <<<"$output"
grep -Fq 'Contributor: contributor has 0 merged PRs -> contributor: new' <<<"$output"
grep -Fq 'ADD:size: XL' "$GH_LOG"
grep -Fq 'ADD:risk: low' "$GH_LOG"
grep -Fq 'ADD:contributor: new' "$GH_LOG"
