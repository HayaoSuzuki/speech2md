#!/usr/bin/env python3
"""Seed and run every fuzz target with bounded time, memory and input size."""

import argparse
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "fuzz" / "corpus"


def vector(items):
    """arbitrary 1.4's Vec encoding: continuation byte before each element."""
    return b"".join(b"\x01" + item for item in items) + b"\x00"


def span(start=0, duration=0, mode=0):
    return struct.pack("<QQB", start, duration, mode)


def text(value):
    return vector(bytes([byte]) for byte in value.encode("utf-8"))


def seed(target, name, data):
    directory = CORPUS / target
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"seed-{name}").write_bytes(data)


def prepare_seeds():
    fixtures = ROOT / "crates" / "yasumaro-runtime" / "tests" / "fixtures"
    for name in ("tone.wav", "tone.mp3", "tone.m4a", "silent.wav", "no-audio.mp4"):
        seed("decode_audio", name, (fixtures / name).read_bytes())
    seed("decode_audio", "empty", b"")
    # An MP4 whose stts atom declares far more entries than it holds. This shape
    # made symphonia read past the atom and overflow its duration accumulation.
    mp4 = (fixtures / "no-audio.mp4").read_bytes()
    count = mp4.index(b"stts") + 8
    oversized = mp4[:count] + struct.pack(">I", 5373953) + mp4[count + 4:]
    seed("decode_audio", "stts-entry-count", oversized)
    # The same file behind another atom, because symphonia searches its whole probe
    # window for the ftyp marker instead of requiring it at the start.
    seed("decode_audio", "stts-entry-count-prefixed", struct.pack(">I", 8) + b"free" + oversized)
    # Probe::format also consumes leading metadata and resumes looking for a
    # container. Use a valid ID3v2.4 tag with one frame-sized padding block.
    id3 = b"ID3\x04\0\0\0\0\0\x0a" + bytes(10)
    seed("decode_audio", "stts-entry-count-after-id3", id3 + oversized)
    # A variable sample whose declared size is almost 4 GiB. Symphonia 0.5.5
    # allocated this untrusted size before discovering that the source is short.
    m4a = (fixtures / "tone.m4a").read_bytes()
    first_sample_size = m4a.index(b"stsz") + 16
    huge_sample = (m4a[:first_sample_size] + struct.pack(">I", 2**32 - 1)
                   + m4a[first_sample_size + 4:])
    seed("decode_audio", "stsz-sample-size", huge_sample)
    seed("whisper_json", "fixture", (fixtures / "whisper-output.json").read_bytes())
    for name, start, end in (("zero", 0, 0), ("max", 2**64 - 1, 2**64 - 1),
                             ("negative", -1, 1), ("reversed", 1, 0),
                             ("overflow", 0, 2**64), ("fraction", 0.5, 1)):
        value = {"transcription": [{"offsets": {"from": start, "to": end}, "text": "日本語\n<&>"}]}
        seed("whisper_json", name, json.dumps(value).encode())
    seed("whisper_json", "empty", b'{"transcription":[]}')
    for kind in range(8):
        segments = vector([span(0, 0) + text(""), span(0, 1, 1) + text("日本語\n\x00<&>")])
        seed("whisper_json_structured", str(kind), bytes([kind]) + segments)

    # A 20% overlap, a tied candidate, tokenless and tokenized segments.
    # Adjacent thresholds prove the comparison is inclusive at the boundary.
    for threshold in (0, 1999, 2000, 2001, 10000):
        for mode in (0, 1, 2):
            segments = vector([
                span(1000, 1000, mode) + vector([]),
                span(1000, 1000, mode) + vector([span(1000, 0, mode), span(1000, 1000, mode)]),
            ])
            turns = vector([span(1000, 200, mode) + struct.pack("<I", speaker) for speaker in (9, 2)])
            seed("assign_speakers", f"{threshold}-{mode}", struct.pack("<H", threshold) + segments + turns)

    for chars in (0, 13, 14, 15, 65535):
        values = vector([
            span(10, 100) + b"\x01\x00",
            span(20, 100) + b"\x01\x00",
            span(120, 0) + b"\x00",
            span(0, 1, 1) + b"\x01\x00",
        ])
        seed("normalize_utterances", str(chars), struct.pack("<HH", 0, chars) + values)
    for gap in (9, 10, 11):
        for speaker in (b"\x00", b"\x01\x00"):
            values = vector([span(0, 10) + speaker, span(10 + gap, 1) + speaker])
            seed("normalize_utterances", f"gap-{gap}-{speaker.hex()}", struct.pack("<HH", 10, 14) + values)

    markdown = ["", "日本語 & <tag>", "# heading\n- list\n> quote", "[link](https://example.com)",
                "![image](x)\n<script>x</script>", "```\ncode\n```", "    spaces\n\ttab",
                "a\r\nb\rc\n\x00", "**bold** _italic_ `code` &amp; \\", "---\n===\n***"]
    seed("render_commonmark", "empty", text("") + vector([]))
    for index, value in enumerate(markdown):
        utterances = vector([
            struct.pack("<Q", 0) + b"\x00" + text(value),
            struct.pack("<Q", 2**64 - 1) + b"\x01" + struct.pack("<I", 2**32 - 1) + text(value),
        ])
        seed("render_commonmark", str(index), text(value) + utterances)

    for rate in range(6):
        for stereo in (0, 1):
            for frames in (1, 63, 64, 65, 4095, 4096, 4097):
                samples = vector(struct.pack("<h", (-32768, 0, 32767)[i % 3])
                                 for i in range(frames * (stereo + 1)))
                seed("decode_wav", f"{rate}-{stereo}-{frames}", bytes([rate, stereo]) + samples)
            seed("decode_wav", f"silence-{rate}-{stereo}", bytes([rate, stereo]) + vector([]))


def main():
    manifest = tomllib.loads((ROOT / "fuzz" / "Cargo.toml").read_text())
    targets = [entry["name"] for entry in manifest["bin"]]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=targets, help="default: every target")
    parser.add_argument("--seconds", type=int, default=60, help="time budget per target")
    parser.add_argument("--seed-only", action="store_true")
    parser.add_argument("--replay", action="store_true", help="replay corpus without mutations")
    args = parser.parse_args()
    if args.seconds < 1:
        parser.error("--seconds must be positive")
    prepare_seeds()
    if args.seed_only:
        return 0
    cargo = shutil.which("cargo")
    if cargo is None:
        parser.error("cargo is not on PATH")
    failed = []
    for target in ([args.target] if args.target else targets):
        limit = 1048576 if target == "decode_audio" else 65536
        command = [cargo, "+nightly", "fuzz", "run", target, str(CORPUS / target), "--",
                   "-runs=0" if args.replay else f"-max_total_time={args.seconds}",
                   f"-max_len={limit}", "-timeout=10", "-rss_limit_mb=2048"]
        print(f"Running {target}", flush=True)
        if subprocess.run(command, cwd=ROOT, check=False).returncode:
            failed.append(target)
    if failed:
        print("Failed targets: " + ", ".join(failed), flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
