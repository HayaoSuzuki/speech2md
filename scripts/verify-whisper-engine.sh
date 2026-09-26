#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 && $# -ne 3 ]]; then
  echo "usage: $0 ARCHIVE [MODEL FIXTURE]" >&2
  exit 2
fi

archive=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d "${TMPDIR:-/tmp}/speech2md-whisper-verify.XXXXXXXX")
trap 'rm -rf -- "$work"' EXIT

while IFS= read -r entry; do
  normalized=${entry#./}
  case "$normalized" in
    ''|bin|bin/|bin/whisper-cli|LICENSE|build-metadata.json) ;;
    *) echo "unexpected archive entry: $entry" >&2; exit 1 ;;
  esac
done < <(tar -tf "$archive")
for required in bin/whisper-cli LICENSE build-metadata.json; do
  tar -tf "$archive" | sed 's#^\./##' | grep -Fxq "$required"
done
tar -xf "$archive" -C "$work"
exe="$work/bin/whisper-cli"
"$exe" --help >/dev/null 2>&1

sentinel=speech2md-secret-prompt-7e18b1
printf %s "$sentinel" > "$work/prompt.txt"
set +e
output=$("$exe" --prompt-file "$work/prompt.txt" --file "$work/missing.wav" 2>&1)
status=$?
set -e
test "$status" -eq 2
! grep -Fq "$sentinel" <<<"$output"
set +e
output=$("$exe" --prompt "$sentinel" --prompt-file "$work/prompt.txt" --file "$work/missing.wav" 2>&1)
status=$?
set -e
test "$status" -eq 1
! grep -Fq "$sentinel" <<<"$output"

if [[ $# -eq 3 ]]; then
  NO_PROXY='*' no_proxy='*' "$exe" --model "$2" --file "$3" --language ja --output-json --no-prints
fi
echo "verified: $archive"
