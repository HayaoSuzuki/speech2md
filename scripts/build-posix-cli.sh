#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUTPUT_DIRECTORY" >&2
  exit 2
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) platform=linux-x86_64; target=x86_64-unknown-linux-gnu ;;
  Darwin-arm64) platform=macos-aarch64; target=aarch64-apple-darwin ;;
  Darwin-x86_64) platform=macos-x86_64; target=x86_64-apple-darwin ;;
  *) echo "unsupported build platform: $(uname -s)-$(uname -m)" >&2; exit 2 ;;
esac

repo_root=$(cd "$(dirname "$0")/.." && pwd)
output=$(mkdir -p "$1" && cd "$1" && pwd)
cd "$repo_root"
metadata=$(cargo metadata --no-deps --format-version 1 --locked)
version=$(python3 -c 'import json, sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "speech2md-cli"))' <<<"$metadata")
target_dir=$(python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])' <<<"$metadata")

# Select the native target explicitly, even if CARGO_BUILD_TARGET is configured.
cargo build --release -p speech2md-cli --locked --target "$target"
binary="$target_dir/$target/release/speech2md"
"$binary" --version
"$binary" --help >/dev/null
"$binary" doctor

archive="$output/speech2md-v$version-$platform.tar.gz"
python3 - "$repo_root" "$binary" "$archive" <<'PY'
import gzip
import hashlib
import os
import sys
import tarfile

root, binary, archive = sys.argv[1:]
files = ((binary, "bin/speech2md", 0o755),
         (os.path.join(root, "LICENSE"), "LICENSE", 0o644),
         (os.path.join(root, "README.md"), "README.md", 0o644),
         (os.path.join(root, "engines", "README.md"), "engines/README.md", 0o644))
with open(archive, "wb") as raw:
    with gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.GNU_FORMAT) as tar:
            for source, name, mode in files:
                info = tar.gettarinfo(source, arcname=name)
                info.uid = info.gid = 0
                info.uname = info.gname = "root"
                info.mtime = 0
                info.mode = mode
                with open(source, "rb") as data:
                    tar.addfile(info, data)
with open(archive, "rb") as data:
    digest = hashlib.sha256(data.read()).hexdigest()
with open(archive + ".sha256", "w", encoding="utf-8", newline="\n") as checksum:
    checksum.write(f"{digest}  {os.path.basename(archive)}\n")
print(archive)
print(f"sha256: {digest}")
PY
