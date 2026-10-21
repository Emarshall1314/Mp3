#!/usr/bin/env python3
"""Self-check for the generated test library.

Verifies, independently of the generator's own bookkeeping, that every fixture really is what the
manifest claims: ffprobe sees the right stream, ffmpeg's decode matches ``pcm_md5``, FLAC's
STREAMINFO MD5 agrees, tags and artwork read back with mutagen, and bytes that must be preserved
are present. Cadenza's tests trust the manifest, so the manifest has to be right.

Failures exit non-zero. Things that depend on ffmpeg's own gapless handling (which is not the
system under test) are reported as warnings.

Usage: check.py FIXTURES_DIR
"""

from __future__ import annotations

import base64
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

import mutagen
from mutagen import apev2, id3
from mutagen.flac import FLAC, Picture
from mutagen.mp4 import MP4

failures: list[str] = []
warnings: list[str] = []


def fail(entry: dict, msg: str) -> None:
    failures.append(f"{entry['id']}: {msg}")


def warn(entry: dict, msg: str) -> None:
    warnings.append(f"{entry['id']}: {msg}")


def ffprobe_stream(path: Path) -> dict | None:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_streams", "-show_format", "-of", "json",
         str(path)], capture_output=True, text=True)
    if out.returncode != 0:
        return None
    data = json.loads(out.stdout)
    streams = data.get("streams") or []
    return (streams[0] | {"format": data.get("format", {})}) if streams else None


PCM_CODEC = {("s", 16): "s16le", ("s", 24): "s24le", ("s", 32): "s32le", ("f", 32): "f32le"}


def decode(path: Path, entry: dict) -> bytes | None:
    fmt = PCM_CODEC[(entry.get("sample_format", "s"), entry.get("bits", 16))] if entry["lossless"] else "s16le"
    out = subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-map", "0:a:0", "-f", fmt, "-c:a",
                          "pcm_" + fmt, "-"], capture_output=True)
    return out.stdout if out.returncode == 0 else None


def frame_bytes(entry: dict) -> int:
    bits = entry.get("bits", 16) if entry["lossless"] else 16
    return (bits + 7) // 8 * entry["channels"]


CODEC_NAMES = {
    "pcm": {"pcm_s16le", "pcm_s24le", "pcm_s32le", "pcm_f32le", "pcm_s16be", "pcm_s24be", "pcm_s32be"},
    "flac": {"flac"}, "alac": {"alac"}, "wavpack": {"wavpack"}, "ape": {"ape"}, "mp3": {"mp3"},
    "aac": {"aac"}, "opus": {"opus"}, "vorbis": {"vorbis"},
    "dsd": {"dsd_lsbf_planar", "dsd_msbf", "dsd_lsbf", "dsd_msbf_planar"}, "wavpack-dsd": {"wavpack"},
}


def check_stream(entry: dict, path: Path) -> None:
    s = ffprobe_stream(path)
    if s is None:
        fail(entry, "ffprobe cannot open it")
        return
    names = CODEC_NAMES.get(entry["codec"], {entry["codec"]})
    if s["codec_name"] not in names:
        fail(entry, f"codec {s['codec_name']} not in {sorted(names)}")
    if int(s["channels"]) != entry["channels"]:
        fail(entry, f"channels {s['channels']} != {entry['channels']}")
    if entry.get("sample_format") == "dsd":
        # ffmpeg reports DSD as bytes per second per channel (rate / 8).
        if int(s["sample_rate"]) * 8 != entry["dsd_rate"] and entry["codec"] != "wavpack-dsd":
            fail(entry, f"DSD rate {int(s['sample_rate']) * 8} != {entry['dsd_rate']}")
        return
    if entry.get("sample_rate") and int(s["sample_rate"]) != entry["sample_rate"]:
        fail(entry, f"sample rate {s['sample_rate']} != {entry['sample_rate']}")
    if entry["lossless"] and entry.get("bits"):
        raw = int(s.get("bits_per_raw_sample") or 0) or int(s.get("bits_per_sample") or 0)
        if raw and raw != entry["bits"]:
            fail(entry, f"bits {raw} != {entry['bits']}")


def check_pcm(entry: dict, path: Path) -> None:
    pcm = decode(path, entry)
    if pcm is None:
        fail(entry, "ffmpeg decode failed")
        return
    frames = len(pcm) // frame_bytes(entry)
    if entry["lossless"]:
        if hashlib.md5(pcm).hexdigest() != entry["pcm_md5"]:
            fail(entry, f"decoded PCM md5 differs (decoded {frames} frames, manifest {entry['frames']})")
    elif entry.get("frames") and frames != entry["frames"]:
        warn(entry, f"ffmpeg decodes {frames} frames, source had {entry['frames']} "
                    f"({frames - entry['frames']:+d}); gapless info is ffmpeg's business here")
    if entry["codec"] == "flac" and shutil.which("metaflac"):
        md5 = subprocess.run(["metaflac", "--show-md5sum", str(path)], capture_output=True, text=True).stdout.strip()
        if md5 != entry["pcm_md5"]:
            fail(entry, f"STREAMINFO MD5 {md5} != pcm_md5 {entry['pcm_md5']}")


# ---------------------------------------------------------------- tags


def id3_values(tags: id3.ID3, key: str) -> list[str] | None:
    if key.startswith("POPM:"):
        f = tags.get(key)
        return [f"rating={f.rating}", f"count={f.count}"] if f else None
    if key == "TMCL":
        f = tags.get("TMCL")
        return [f"{r}={n}" for r, n in f.people] if f else None
    if key.startswith("UFID:"):
        f = tags.get(key)
        return [f.data.decode()] if f else None
    if key.startswith("SYLT"):
        f = tags.get(key)
        return [f"{t}@{ms}" for t, ms in f.text] if f else None
    f = tags.get(key)
    if f is None:
        return None
    if key.startswith("USLT"):
        return [f.text]
    return [str(t) for t in f.text]


V23_RENAMED = {"TDRC", "TDOR"}  # converted to TYER/TDAT and TORY on save


def check_native(entry: dict, path: Path) -> None:
    native = entry.get("native")
    if not native or "items" not in native:
        return
    fmt = native["format"]
    f = mutagen.File(path)
    if f is None or f.tags is None:
        fail(entry, "mutagen finds no tags")
        return
    tags = f.tags
    for key, values in native["items"]:
        if fmt.startswith("id3"):
            if fmt == "id3v2.3" and key in V23_RENAMED:
                continue
            got = id3_values(tags, key)
            if fmt == "id3v2.3" and got is not None and len(values) > 1 and not key.startswith(("POPM", "SYLT", "TMCL")):
                values = ["/".join(values)]
        elif fmt == "vorbis":
            got = tags.get(key)
        elif fmt == "apev2":
            v = tags.get(key)
            got = list(v) if v is not None else None
        elif fmt == "mp4":
            got = mp4_values(tags, key)
        else:
            raise ValueError(fmt)
        if got != values:
            fail(entry, f"{fmt} {key}: read {got!r}, wrote {values!r}")
    if fmt == "id3v2.3" and "date" in (entry.get("tags") or {}):
        # mutagen upgrades TYER+TDAT to TDRC on load, so check the merged value and that the file
        # really holds the v2.3 frames (not v2.4 TDRC) on disk.
        want = entry["tags"]["date"][0]
        got = id3_values(tags, "TDRC")
        blob = path.read_bytes()
        if got != [want] or b"TYER" not in blob or b"TDAT" not in blob or b"TDRC" in blob:
            fail(entry, f"v2.3 date: merged {got}, want {want}; TYER/TDAT on disk, no TDRC")


def mp4_values(tags, key: str) -> list[str] | None:
    v = tags.get(key)
    if v is None:
        return None
    if key in ("trkn", "disk"):
        return [f"{a}/{b}" for a, b in v]
    if key == "cpil":
        return ["1" if v else "0"]
    if key in ("tmpo", "©mvi", "©mvc"):
        return [str(x) for x in v]
    if key.startswith("----"):
        return [bytes(x).decode() for x in v]
    return [str(x) for x in v]


def check_preserve(entry: dict, path: Path) -> None:
    if not entry.get("preserve"):
        return
    blob = path.read_bytes()
    for item in entry["preserve"]:
        needles = []
        if "data_hex" in item:
            needles.append(bytes.fromhex(item["data_hex"]))
        if "value" in item:
            needles.append(item["value"].encode())
        if item.get("kind") == "id3-frame" and "data_hex" not in item:
            needles.append(item["id"].encode())
        for n in needles:
            if n not in blob:
                fail(entry, f"preserved item {item} missing from file bytes")


# ---------------------------------------------------------------- artwork


def pictures(path: Path) -> list[tuple[int, str]]:
    f = mutagen.File(path)
    out = []
    if isinstance(f, FLAC):
        out = [(p.type, hashlib.sha1(p.data).hexdigest()) for p in f.pictures]
    elif isinstance(f, MP4):
        out = [(3, hashlib.sha1(bytes(c)).hexdigest()) for c in (f.tags or {}).get("covr", [])]
    elif f is not None and isinstance(f.tags, id3.ID3):
        out = [(a.type, hashlib.sha1(a.data).hexdigest()) for a in f.tags.getall("APIC")]
    elif f is not None and isinstance(f.tags, apev2.APEv2):
        v = f.tags.get("Cover Art (Front)")
        if v is not None:
            data = bytes(v.value)
            out = [(3, hashlib.sha1(data[data.index(b"\0") + 1:]).hexdigest())]
    elif f is not None and f.tags is not None and "METADATA_BLOCK_PICTURE" in f.tags:
        for b64 in f.tags["METADATA_BLOCK_PICTURE"]:
            p = Picture(base64.b64decode(b64))
            out.append((p.type, hashlib.sha1(p.data).hexdigest()))
    return out


def check_artwork(entry: dict, path: Path) -> None:
    want = sorted((a["type"], a["sha1"]) for a in entry.get("artwork", []))
    if not want:
        return
    got = sorted(pictures(path))
    if got != want:
        fail(entry, f"artwork {got} != {want}")


# ---------------------------------------------------------------- groups


def check_gapless(entries: list[dict], root: Path) -> None:
    sets: dict[str, list[dict]] = {}
    for e in entries:
        if e.get("gapless"):
            sets.setdefault(e["gapless"]["set"], []).append(e)
    for name, parts in sets.items():
        parts.sort(key=lambda e: e["gapless"]["index"])
        g = parts[0]["gapless"]
        if [p["gapless"]["index"] for p in parts] != list(range(g["count"])):
            fail(parts[0], f"gapless set {name} incomplete")
            continue
        pcm = b"".join(decode(root / p["path"], p) or b"" for p in parts)
        frames = len(pcm) // 4
        if parts[0]["lossless"]:
            if hashlib.md5(pcm).hexdigest() != g["source_md5"]:
                fail(parts[0], f"gapless set {name}: joined decode differs from source")
        elif frames != g["source_frames"]:
            warn(parts[0], f"gapless set {name}: ffmpeg joins to {frames} frames, source {g['source_frames']}")


def check_cue(entry: dict, path: Path) -> None:
    cue = entry.get("cue")
    if not cue:
        return
    if cue["sheet"].endswith(".cue") and not (path.parent / cue["sheet"]).exists():
        fail(entry, "cue sheet missing")
    if entry.get("frames") != cue["total_frames"]:
        fail(entry, f"frames {entry.get('frames')} != cue total {cue['total_frames']}")


def check_title(entry: dict, path: Path) -> None:
    """For entries with expected fields but no native list, at least the title must read back."""
    tags = entry.get("tags")
    if not tags or entry.get("native", {}).get("items") or "title" not in tags:
        return
    if entry.get("native", {}).get("legacy_charset"):
        raw = bytes.fromhex(entry["native"]["raw_hex"])
        t = id3.ID3(path)
        if t["TIT2"].encoding != id3.Encoding.LATIN1 or str(t["TIT2"].text[0]).encode("latin-1") != raw:
            fail(entry, "legacy title bytes not stored as raw Latin-1")
        return
    f = mutagen.File(path)
    title = None
    if f is not None and f.tags is not None:
        if isinstance(f.tags, id3.ID3):
            title = id3_values(f.tags, "TIT2")
        elif isinstance(f, MP4):
            title = mp4_values(f.tags, "©nam")
        elif isinstance(f.tags, apev2.APEv2):
            title = list(f.tags["Title"]) if "Title" in f.tags else None
        else:
            title = f.tags.get("TITLE")
    if title != tags["title"]:
        fail(entry, f"title {title!r} != {tags['title']!r}")


def main() -> int:
    root = Path(sys.argv[1])
    manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    entries = manifest["entries"]
    for e in entries:
        path = root / e["path"]
        if not path.exists():
            fail(e, "file missing")
            continue
        if e.get("expect") == "error" or e.get("codec") == "unknown":
            continue
        check_stream(e, path)
        if e.get("pcm_md5") or (not e["lossless"] and e.get("frames")):
            check_pcm(e, path)
        check_native(e, path)
        check_preserve(e, path)
        check_artwork(e, path)
        check_cue(e, path)
        check_title(e, path)
    check_gapless(entries, root)
    for w in warnings:
        print(f"warning: {w}")
    for f in failures:
        print(f"FAIL: {f}")
    print(f"fixture check: {len(entries)} entries, {len(failures)} failures, {len(warnings)} warnings")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
