#!/usr/bin/env bash
set -euo pipefail

contract_only=false
if [[ ${1:-} == --contract-only ]]; then
  contract_only=true
  shift
fi
if { [[ $contract_only == true ]] && [[ $# -ne 1 ]]; } || { [[ $contract_only == false ]] && [[ $# -ne 3 ]]; }; then
  echo "usage: $0 [--contract-only] ARCHIVE [MODEL FIXTURE]" >&2
  exit 2
fi

archive=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d "${TMPDIR:-/tmp}/speech2md-whisper-verify.XXXXXXXX")
trap 'rm -rf -- "$work"' EXIT

while IFS= read -r entry; do
  case "$entry" in
    bin/whisper-cli|LICENSE|build-metadata.json) ;;
    *) echo "unexpected archive entry: $entry" >&2; exit 1 ;;
  esac
done < <(tar -tf "$archive")
for required in bin/whisper-cli LICENSE build-metadata.json; do
  tar -tf "$archive" | grep -Fxq "$required"
done
tar -xf "$archive" -C "$work"
exe="$work/bin/whisper-cli"
test -f "$exe" && test ! -L "$exe"
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

if [[ $contract_only == false ]]; then
  HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9 \
    http_proxy=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9 all_proxy=http://127.0.0.1:9 \
    NO_PROXY='' no_proxy='' "$exe" --model "$2" --file "$3" --language ja \
    --output-json --output-file "$work/verified" --no-prints
  HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9 \
    http_proxy=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9 all_proxy=http://127.0.0.1:9 \
    NO_PROXY='' no_proxy='' "$exe" --model "$2" --file "$3" --language ja \
    --output-json --output-file "$work/cancelled" --no-prints &
  child=$!
  sleep 0.1
  if ! kill -0 "$child" 2>/dev/null; then
    wait "$child"
    echo "fixture completed before cancellation could be exercised" >&2
    exit 1
  fi
  kill -TERM "$child"
  set +e
  wait "$child"
  set -e
  if kill -0 "$child" 2>/dev/null; then
    echo "cancelled whisper process remained alive" >&2
    exit 1
  fi
fi
echo "verified: $archive"
