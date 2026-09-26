#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUTPUT_DIRECTORY" >&2
  exit 2
fi

upstream_url=https://github.com/ggml-org/whisper.cpp.git
upstream_commit=927cfce34f31707e17f2bff35c349632fb9e2c3a
upstream_version=v1.9.4
repo_root=$(cd "$(dirname "$0")/.." && pwd)
patch_path="$repo_root/vendor/whisper.cpp-patches/0001-add-prompt-file.patch"
output=$(mkdir -p "$1" && cd "$1" && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/speech2md-whisper-build.XXXXXXXX")
trap 'rm -rf -- "$work"' EXIT

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) platform=linux-x86_64 ;;
  Darwin-arm64) platform=macos-aarch64 ;;
  Darwin-x86_64) platform=macos-x86_64 ;;
  *) echo "unsupported build platform: $(uname -s)-$(uname -m)" >&2; exit 2 ;;
esac

git clone --filter=blob:none --no-checkout "$upstream_url" "$work/source"
git -C "$work/source" checkout --detach "$upstream_commit"
test "$(git -C "$work/source" rev-parse HEAD)" = "$upstream_commit"
git -C "$work/source" apply --check "$patch_path"
git -C "$work/source" apply "$patch_path"

cmake -S "$work/source" -B "$work/build" \
  -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
  -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DGGML_CUDA=OFF \
  -DWHISPER_BUILD_TESTS=ON -DWHISPER_BUILD_EXAMPLES=ON -DWHISPER_FFMPEG=OFF
cmake --build "$work/build" --config Release --target whisper-cli --parallel
ctest --test-dir "$work/build" -C Release -R test-whisper-cli-prompt-file --output-on-failure

binary=$(find "$work/build" -type f -name whisper-cli -print -quit)
test -n "$binary"
mkdir -p "$work/stage/bin"
cp "$binary" "$work/stage/bin/whisper-cli"
cp "$work/source/LICENSE" "$work/stage/LICENSE"
patch_hash=$(sha256_file "$patch_path")
compiler_file=$(find "$work/build/CMakeFiles" -name CMakeCXXCompiler.cmake -print -quit)
compiler=$(sed -n 's/^set(CMAKE_CXX_COMPILER_VERSION "\([^"]*\)")/\1/p' "$compiler_file")
test -n "$compiler"
python3 - "$work/stage/build-metadata.json" "$upstream_url" "$upstream_version" "$upstream_commit" "$patch_hash" "$platform" "$compiler" <<'PY'
import json, sys
path, url, version, commit, patch_hash, platform, compiler = sys.argv[1:]
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump({"schema_version": 1, "upstream_url": url, "upstream_version": version,
               "upstream_commit": commit, "patch_sha256": patch_hash, "platform": platform,
               "compiler": compiler, "cmake_options": ["BUILD_SHARED_LIBS=OFF", "GGML_NATIVE=OFF",
               "GGML_OPENMP=OFF", "GGML_CUDA=OFF", "WHISPER_BUILD_TESTS=ON",
               "WHISPER_BUILD_EXAMPLES=ON", "WHISPER_FFMPEG=OFF"]}, f, indent=2)
    f.write("\n")
PY

archive="$output/speech2md-whispercpp-$upstream_version-$platform.tar.gz"
if tar --version 2>/dev/null | grep -q GNU; then
  tar --sort=name --mtime='UTC 2020-01-01' --owner=0 --group=0 --numeric-owner -C "$work/stage" -czf "$archive" .
else
  COPYFILE_DISABLE=1 tar -C "$work/stage" -czf "$archive" .
fi
printf '{"path":"%s","size":%s,"sha256":"%s"}\n' "$archive" "$(wc -c < "$archive" | tr -d ' ')" "$(sha256_file "$archive")"
