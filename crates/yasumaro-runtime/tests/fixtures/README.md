# Audio test fixtures

These files are generated signals and are dedicated to the public domain under CC0-1.0.
They contain no recorded speech or third-party media.
FFmpeg is required only to regenerate the fixtures; yasumaro does not invoke FFmpeg at runtime.

The fixtures were generated with FFmpeg 8.0.1 on Windows:

```powershell
ffmpeg -f lavfi -i "sine=frequency=1000:sample_rate=48000:duration=2" -filter_complex "[0:a]pan=stereo|c0=c0|c1=c0[a]" -map "[a]" -c:a pcm_s16le -y tone.wav
ffmpeg -i tone.wav -c:a libmp3lame -q:a 2 -y tone.mp3
ffmpeg -i tone.wav -c:a aac -b:a 128k -movflags +faststart -y tone.m4a
ffmpeg -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 2 -c:a pcm_s16le -y silent.wav
ffmpeg -f lavfi -i "color=c=black:s=16x16:r=1:d=1" -an -c:v libx264 -pix_fmt yuv420p -movflags +faststart -y no-audio.mp4
```

- `tone.*`: 2 seconds, 48 kHz stereo, 1 kHz sine wave
- `silent.wav`: 2 seconds, 48 kHz stereo silence
- `no-audio.mp4`: 1 second synthetic black video without an audio track

## ローカルの実音声テスト

VOICEPEAKで生成した音声とSRV-DBの音声および原稿はコミットしません。

実モデルを使うテストでは、モデルとローカル音声を環境変数で指定します。

```powershell
$env:YASUMARO_MODEL_DIR = "C:\path\to\models"
$env:YASUMARO_DIARIZATION_FIXTURE = (Resolve-Path "samples\manjyu_kowai.wav")
cargo test -p yasumaro-runtime --test diarization_model -- --ignored --nocapture
```

CLI全体を通す場合は、エンジンの配置先も指定します。

```powershell
$env:YASUMARO_ENGINE_DIR = "C:\path\to\engines"
cargo test -p yasumaro-cli --test real_cli -- --ignored --nocapture
```

通常のGitHub Actionsはモデルを取得せず、実モデルテストも実行しません。
再配布可能な複数話者fixtureが用意できるまでは、実モデルE2Eをローカル専用とします。
