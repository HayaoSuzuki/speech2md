# whisper.cpp engine artifacts

speech2md ships no engine binary in the repository. Release artifacts are built from
whisper.cpp `v1.9.4` commit `927cfce34f31707e17f2bff35c349632fb9e2c3a` plus the
patch in `vendor/whisper.cpp-patches/`.

Build on the target operating system:

```powershell
.\scripts\build-whisper-engine.ps1 -OutputDirectory .\dist
.\scripts\verify-whisper-engine.ps1 -Archive .\dist\speech2md-whispercpp-v1.9.4-windows-x86_64.zip
```

```sh
./scripts/build-whisper-engine.sh ./dist
./scripts/verify-whisper-engine.sh ./dist/speech2md-whispercpp-v1.9.4-linux-x86_64.tar.gz
```

For an end-to-end offline check, pass a local model and a short local WAV fixture as
the final two verification arguments. Neither file is uploaded or packaged.

Release procedure:

1. Build and verify on each target OS. Do not cross-compile an untested archive.
2. Upload the exact verified archive to a GitHub Release.
3. Measure its byte size and lowercase SHA-256 after upload/download.
4. Add only real published artifacts to `manifest.json`, using an HTTPS GitHub
   Releases URL and `bin/whisper-cli` (`.exe` on Windows) as `executable_path`.
5. Run the normal CI suite and `speech2md engine install`, `verify`, and `prune`.

Missing platforms intentionally remain absent from the manifest. The CLI reports
`MissingArtifact`; placeholder URLs or hashes are never used.
