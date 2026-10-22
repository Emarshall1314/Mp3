#!/usr/bin/env bash
# Runs a CI command. On failure, the end of its output is also published as an error annotation,
# which can be read from the GitHub API where the full job log can't be downloaded.
set -o pipefail
log=$(mktemp)
"$@" 2>&1 | tee "$log"
status=$?
if [ "$status" -ne 0 ]; then
  tail -n 80 "$log" | cut -c1-300 | sed -e 's/\x1b\[[0-9;]*m//g' -e 's/%/%25/g' -e 's/\r/%0D/g' \
    | awk 'BEGIN { ORS = "%0A" } { print }' | sed 's/^/::error title=Failed\: '"$1"'::/'
  echo
fi
exit "$status"
