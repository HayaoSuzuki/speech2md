# whisper.cpp engine artifacts

yasumaro ships no engine binary in the repository. Release artifacts are built from
whisper.cpp `v1.9.4` commit `927cfce34f31707e17f2bff35c349632fb9e2c3a` plus the
patch in `vendor/whisper.cpp-patches/`.

Build on the target operating system:

```powershell
.\scripts\build-whisper-engine.ps1 -OutputDirectory .\dist
.\scripts\verify-whisper-engine.ps1 -Archive .\dist\yasumaro-whispercpp-v1.9.4-windows-x86_64.zip -ContractOnly
```

```sh
./scripts/build-whisper-engine.sh ./dist
./scripts/verify-whisper-engine.sh --contract-only ./dist/yasumaro-whispercpp-v1.9.4-linux-x86_64.tar.gz
```

The shell script selects `linux-x86_64`, `macos-aarch64`, or `macos-x86_64`
from the build host. Substitute that platform in the archive name above.
It requires Git, CMake, a C/C++ compiler, and Python 3. Metal and CUDA are
explicitly disabled for CPU-only inference; no separate Metal resources are needed.

The `Build and release` workflow builds all four platforms on native runners.
Every engine is checked with a hash-verified base model and the repository's tone
fixture, including offline inference and cancellation. The workflow then generates
a manifest from the exact archives, embeds it in each CLI, and publishes the CLI
and engine archives together after a PR is merged into `main`. CLI versions match
the automatically reserved `vMAJOR.MINOR.PATCH` tag. The manifest's version includes
that tag so engine installations from different releases remain separate.

PR and manual runs build preview artifacts without publishing. Their generated
manifest URLs are not installable release URLs. The checked-in manifest remains
unchanged; published release CLIs use the generated manifest instead.
The checked-in manifest includes the Linux x86-64 and macOS Apple Silicon engines
published in `v0.1.2`. It also retains the published Windows `speech2md` engine archive:
only its repository URL changes to `HayaoSuzuki/yasumaro`. Its tag, file name, size,
and SHA-256 must continue to match the existing release. Newly built archives and
generated manifests use the `yasumaro` name.

`--contract-only` checks archive shape, executable startup, and prompt handling without
a model. It is suitable for build jobs, but not release approval. Before publishing,
omit that flag and pass a local model and a short local WAV fixture (PowerShell:
`-Model` and `-Fixture`). Full verification runs offline inference and terminates a
second inference process to check cancellation cleanup. The fixture must run for more
than 100 ms. Neither file is uploaded or packaged.

Manual engine release procedure (for source builds):

1. Build and verify on each target OS. Do not cross-compile an untested archive.
2. Upload the exact verified archive to a GitHub Release.
3. Measure its byte size and lowercase SHA-256 after upload/download.
4. Add only real published artifacts to `manifest.json`, using an HTTPS GitHub
   Releases URL and `bin/whisper-cli` (`.exe` on Windows) as `executable_path`.
5. Run the normal CI suite and `yasumaro engine install`, `verify`, and `prune`.

Missing platforms intentionally remain absent from the manifest. The CLI reports
`MissingArtifact`; placeholder URLs or hashes are never used.
