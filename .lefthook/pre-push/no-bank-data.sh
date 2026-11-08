#!/usr/bin/env bash
# Refuse to push bank exports, statements, or databases that live outside
# fixtures/. Checks every file added or modified in the commits being pushed,
# not just the tip, so a file added and later deleted is still caught.
#
# git feeds pre-push one line per ref on stdin:
#   <local ref> <local sha> <remote ref> <remote sha>
set -euo pipefail

data_pattern='\.(csv|tsv|ofx|qfx|qif|xls|xlsx|pdf|db|sqlite|sqlite3)$'

files_in_range() {
  git log --format= --name-only --diff-filter=AMRC "$@" | sort -u
}

pushed=""
saw_ref=0
while read -r _local_ref local_sha _remote_ref remote_sha; do
  saw_ref=1
  [[ $local_sha =~ ^0+$ ]] && continue # branch deletion: nothing pushed
  if [[ $remote_sha =~ ^0+$ ]]; then
    # New remote branch: everything not already on any remote.
    pushed+=$(files_in_range "$local_sha" --not --remotes)$'\n'
  else
    pushed+=$(files_in_range "$remote_sha..$local_sha")$'\n'
  fi
done

# Run by hand (lefthook run pre-push) with no stdin: check unpushed HEAD.
if [[ $saw_ref -eq 0 ]]; then
  pushed=$(files_in_range HEAD --not --remotes)
fi

offending=$(
  printf '%s\n' "$pushed" |
    grep -Ei -e "$data_pattern" -e '(^|/)samples/' |
    grep -v '^fixtures/' |
    sort -u || true
)

if [[ -n $offending ]]; then
  echo "Refusing to push possible financial data outside fixtures/:" >&2
  sed 's/^/  /' <<<"$offending" >&2
  echo "Real exports belong in the gitignored samples/ directory." >&2
  echo "Redacted fixtures belong in fixtures/. Rewrite the commits to remove them." >&2
  exit 1
fi
