#!/usr/bin/env python3
"""Generate Cadenza's test library.

Everything is synthesised (sine tones, noise, generated images); no copyrighted audio. Output is a
directory tree plus ``manifest.json`` describing, for every file, what a correct reader must find:
stream properties, the exact PCM (as an MD5 of a canonical byte layout), tags, unknown items that
must survive a rewrite, artwork, and whether the file is deliberately broken.

Tags are written with mutagen and the reference encoders (flac, lame, opusenc, ...), never with
Cadenza's own code, so the fixtures are an independent oracle for Cadenza's tag and decode crates.

Canonical PCM layout for ``pcm_md5``: interleaved, little-endian; integers signed at the stream's
bit depth using ceil(bits/8) bytes (so 24-bit is packed 3-byte); float as IEEE f32. This is the
layout FLAC's STREAMINFO MD5 uses, so for FLAC the two must agree.

Canonical DSD layout for ``dsd_md5``: per byte-time, one byte per channel in channel order, MSB =
earliest bit (the DSDIFF layout).

Usage: generate.py --out DIR [--only GROUP ...] [--keep-going]
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import math
import os
import random
import shutil
import struct
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

import mutagen
from mutagen import apev2, id3
from mutagen.aiff import AIFF
from mutagen.dsdiff import DSDIFF
from mutagen.dsf import DSF
from mutagen.flac import FLAC, MetadataBlock, Picture
from mutagen.mp4 import MP4, MP4Cover, MP4FreeForm
from mutagen.oggopus import OggOpus
from mutagen.oggvorbis import OggVorbis
from mutagen.wave import WAVE

GENERATOR_VERSION = 1

# --------------------------------------------------------------------------------------------
# Tools


def find_tool(name: str, env: str | None = None) -> str:
    if env and os.environ.get(env):
        return os.environ[env]
    path = shutil.which(name)
    if not path:
        sys.exit(f"fixtures: required tool `{name}` not found on PATH")
    return path


@dataclass
class Tools:
    ffmpeg: str
    ffprobe: str
    sox: str
    flac: str
    lame: str
    opusenc: str
    oggenc: str
    wavpack: str
    mac: str

    @staticmethod
    def discover() -> "Tools":
        return Tools(
            ffmpeg=find_tool("ffmpeg"),
            ffprobe=find_tool("ffprobe"),
            sox=find_tool("sox"),
            flac=find_tool("flac"),
            lame=find_tool("lame"),
            opusenc=find_tool("opusenc"),
            oggenc=find_tool("oggenc"),
            wavpack=find_tool("wavpack"),
            mac=find_tool("mac", "CADENZA_MAC"),
        )

    def versions(self) -> dict[str, str]:
        def first_line(*cmd: str) -> str:
            try:
                out = subprocess.run(cmd, capture_output=True, text=True, errors="replace")
                text = (out.stdout or out.stderr).strip().splitlines()
                return text[0] if text else "?"
            except OSError:
                return "?"

        return {
            "ffmpeg": first_line(self.ffmpeg, "-version"),
            "sox": first_line(self.sox, "--version"),
            "flac": first_line(self.flac, "--version"),
            "lame": first_line(self.lame, "--version"),
            "opusenc": first_line(self.opusenc, "--version"),
            "oggenc": first_line(self.oggenc, "--version"),
            "wavpack": first_line(self.wavpack, "--version"),
            "mutagen": mutagen.version_string,
        }


def run(*cmd: str | Path) -> None:
    args = [str(c) for c in cmd]
    proc = subprocess.run(args, capture_output=True, text=True, errors="replace")
    if proc.returncode != 0:
        raise RuntimeError(f"command failed ({proc.returncode}): {' '.join(args)}\n{proc.stderr}")


# --------------------------------------------------------------------------------------------
# Signals and PCM


@dataclass(frozen=True)
class Pcm:
    rate: int
    bits: int
    fmt: str  # "s" signed integer | "f" float
    channels: int

    @property
    def sox_args(self) -> list[str]:
        enc = "floating-point" if self.fmt == "f" else "signed-integer"
        return ["-r", str(self.rate), "-b", str(self.bits), "-e", enc, "-c", str(self.channels)]

    @property
    def tag(self) -> str:
        return f"{self.fmt}{self.bits}_{self.rate}" + (f"_{self.channels}ch" if self.channels != 2 else "")


# Distinct frequency per channel so channel swaps and mixes are detectable.
CHANNEL_FREQS = [997.0, 1499.0, 251.0, 3001.0, 1999.0, 499.0, 2503.0, 701.0]


def synth_wav(tools: Tools, out: Path, pcm: Pcm, seconds: float, freqs: list[float] | None = None,
              gain_db: float = -6.0) -> None:
    """Write a WAV of sine tones (one per channel) with sox; -D keeps it free of random dither."""
    freqs = freqs or CHANNEL_FREQS[: pcm.channels]
    synth = []
    for f in freqs:
        synth += ["sine", f"{f}"]
    run(tools.sox, "-D", "-n", *pcm.sox_args, out, "synth", f"{seconds}", *synth, "gain", f"{gain_db}")


def wav_data(path: Path) -> tuple[dict, bytes]:
    """Return (fmt fields, raw data chunk) of a RIFF/WAVE file."""
    blob = path.read_bytes()
    assert blob[:4] == b"RIFF" and blob[8:12] == b"WAVE", path
    pos, fmt, data = 12, None, None
    while pos + 8 <= len(blob):
        cid, size = blob[pos:pos + 4], struct.unpack("<I", blob[pos + 4:pos + 8])[0]
        body = blob[pos + 8:pos + 8 + size]
        if cid == b"fmt ":
            tag, ch, rate, _, align, bits = struct.unpack("<HHIIHH", body[:16])
            fmt = {"tag": tag, "channels": ch, "rate": rate, "align": align, "bits": bits}
        elif cid == b"data":
            data = body
        pos += 8 + size + (size & 1)
    assert fmt and data is not None, path
    return fmt, data


def pcm_md5(data: bytes) -> str:
    return hashlib.md5(data).hexdigest()


def frames_of(data: bytes, pcm: Pcm) -> int:
    return len(data) // ((pcm.bits + 7) // 8 * pcm.channels)


# --------------------------------------------------------------------------------------------
# DSD


def sigma_delta(n: int, rate: int, freq: float, amp: float, phase: float) -> bytearray:
    """Second-order sigma-delta modulation of a sine; returns one bit per sample (0/1)."""
    bits = bytearray(n)
    i1 = i2 = 0.0
    y = 1.0
    w = 2.0 * math.pi * freq / rate
    sin = math.sin
    for k in range(n):
        x = amp * sin(w * k + phase)
        i1 += x - y
        i2 += i1 - y
        if i2 >= 0.0:
            y = 1.0
            bits[k] = 1
        else:
            y = -1.0
    return bits


def pack_bits(bits: bytearray, msb_first: bool) -> bytes:
    out = bytearray((len(bits) + 7) // 8)
    for i, b in enumerate(bits):
        if b:
            shift = 7 - (i & 7) if msb_first else (i & 7)
            out[i >> 3] |= 1 << shift
    return bytes(out)


_dsd_cache: dict[tuple[int, float], list[bytearray]] = {}


def dsd_signal(rate: int, seconds: float, channels: int = 2) -> list[bytearray]:
    key = (rate, seconds)
    if key not in _dsd_cache:
        n = int(rate * seconds)
        n -= n % 8
        _dsd_cache[key] = [sigma_delta(n, rate, CHANNEL_FREQS[c], 0.5, 0.0) for c in range(channels)]
    return _dsd_cache[key]


def dsd_canonical_md5(chans: list[bytearray]) -> str:
    packed = [pack_bits(c, msb_first=True) for c in chans]
    inter = bytearray()
    for i in range(len(packed[0])):
        for p in packed:
            inter.append(p[i])
    return hashlib.md5(bytes(inter)).hexdigest()


def write_dsf(path: Path, rate: int, chans: list[bytearray]) -> None:
    block = 4096
    n = len(chans[0])
    per_ch = [pack_bits(c, msb_first=False) for c in chans]
    nblocks = (len(per_ch[0]) + block - 1) // block
    data = bytearray()
    for b in range(nblocks):
        for p in per_ch:
            chunk = p[b * block:(b + 1) * block]
            data += chunk + bytes(block - len(chunk))
    channel_type = {1: 1, 2: 2}[len(chans)]
    fmt = struct.pack("<4sQIIIIIIQI4x", b"fmt ", 52, 1, 0, channel_type, len(chans), rate, 1, n, block)
    data_chunk = struct.pack("<4sQ", b"data", 12 + len(data)) + data
    total = 28 + len(fmt) + len(data_chunk)
    header = struct.pack("<4sQQQ", b"DSD ", 28, total, 0)
    path.write_bytes(header + fmt + data_chunk)


def write_dff(path: Path, rate: int, chans: list[bytearray]) -> None:
    def chunk(cid: bytes, body: bytes) -> bytes:
        pad = b"\0" if len(body) & 1 else b""
        return cid + struct.pack(">Q", len(body)) + body + pad

    packed = [pack_bits(c, msb_first=True) for c in chans]
    inter = bytearray()
    for i in range(len(packed[0])):
        for p in packed:
            inter.append(p[i])
    ids = [b"SLFT", b"SRGT"][: len(chans)] if len(chans) <= 2 else [b"C%03d" % i for i in range(len(chans))]
    compression_name = b"not compressed"
    prop = b"SND " + chunk(b"FS  ", struct.pack(">I", rate)) + chunk(
        b"CHNL", struct.pack(">H", len(chans)) + b"".join(ids)
    ) + chunk(b"CMPR", b"DSD " + bytes([len(compression_name)]) + compression_name + b"\0")
    body = b"DSD " + chunk(b"FVER", struct.pack(">I", 0x01050000)) + chunk(b"PROP", prop) + chunk(b"DSD ", bytes(inter))
    path.write_bytes(chunk(b"FRM8", body))


# --------------------------------------------------------------------------------------------
# Manifest


@dataclass
class Entry:
    id: str
    path: str
    group: str
    container: str
    codec: str
    lossless: bool
    channels: int
    sample_rate: int | None = None
    bits: int | None = None
    sample_format: str | None = None  # s | f | dsd
    frames: int | None = None
    pcm_md5: str | None = None
    dsd_rate: int | None = None
    dsd_md5: str | None = None
    tags: dict[str, list[str]] | None = None
    native: dict | None = None
    preserve: list[dict] = field(default_factory=list)
    artwork: list[dict] = field(default_factory=list)
    gapless: dict | None = None
    cue: dict | None = None
    album_key: str | None = None
    expect: str = "ok"  # ok | error | partial
    notes: str = ""

    def to_json(self) -> dict:
        return {k: v for k, v in self.__dict__.items() if v not in (None, [], "")}


class Library:
    def __init__(self, root: Path, tools: Tools):
        self.root = root
        self.tools = tools
        self.entries: list[Entry] = []
        self.tmp = Path(tempfile.mkdtemp(prefix="cadenza-fixtures-"))

    def path(self, rel: str) -> Path:
        p = self.root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        return p

    def scratch(self, name: str) -> Path:
        return self.tmp / name

    def add(self, entry: Entry) -> Entry:
        self.entries.append(entry)
        return entry


# --------------------------------------------------------------------------------------------
# Encoders: each takes a source WAV and writes the target, returning (container, codec, lossless).


def encode(lib: Library, src: Path, dst: Path, kind: str, pcm: Pcm) -> None:
    t = lib.tools
    if kind == "wav":
        shutil.copyfile(src, dst)
    elif kind == "flac":
        run(t.flac, "--silent", "--force", "-8", "-o", dst, src)
    elif kind == "alac":
        sample_fmt = "s16p" if pcm.bits == 16 else "s32p"
        run(t.ffmpeg, "-v", "error", "-y", "-i", src, "-c:a", "alac", "-sample_fmt", sample_fmt, dst)
    elif kind == "aiff":
        run(t.sox, "-D", src, dst)
    elif kind == "wavpack":
        run(t.wavpack, "-q", "-y", "-hh", src, "-o", dst)
    elif kind == "ape":
        # Monkey's Audio 3.99 only parses the plain PCM WAV header, not WAVE_FORMAT_EXTENSIBLE.
        plain = lib.scratch(f"plain_{src.name}")
        run(t.sox, "-D", src, "-t", "wavpcm", plain)
        run(t.mac, plain, dst, "-c2000")
    elif kind == "mp3-cbr":
        run(t.lame, "--quiet", "--noreplaygain", "-b", "320", src, dst)
    elif kind == "mp3-vbr":
        run(t.lame, "--quiet", "--noreplaygain", "-V0", src, dst)
    elif kind == "aac":
        # A movie timescale equal to the sample rate makes the edit list sample-exact
        # (ffmpeg's default of 1000 rounds the trim to the millisecond).
        run(t.ffmpeg, "-v", "error", "-y", "-i", src, "-c:a", "aac", "-b:a", "256k",
            "-movie_timescale", str(pcm.rate), dst)
    elif kind == "aac-itunsmpb":
        # No edit list; the gapless info lives only in the iTunSMPB freeform atom instead.
        run(t.ffmpeg, "-v", "error", "-y", "-i", src, "-c:a", "aac", "-b:a", "256k", "-use_editlist", "0", dst)
        add_itunsmpb(lib, dst, frames_of(wav_data(src)[1], pcm))
    elif kind == "opus":
        run(t.opusenc, "--quiet", "--bitrate", "160", src, dst)
    elif kind == "vorbis":
        run(t.oggenc, "--quiet", "-q", "6", "-o", dst, src)
    else:
        raise ValueError(kind)


AAC_PRIMING = 1024  # ffmpeg's native AAC encoder delay


def add_itunsmpb(lib: Library, path: Path, frames: int) -> None:
    out = subprocess.run(
        [lib.tools.ffprobe, "-v", "error", "-select_streams", "a:0", "-count_packets",
         "-show_entries", "stream=nb_read_packets", "-of", "csv=p=0", str(path)],
        capture_output=True, text=True, check=True,
    )
    packets = int(out.stdout.strip())
    padding = packets * 1024 - AAC_PRIMING - frames
    value = " 00000000 %08X %08X %016X" % (AAC_PRIMING, padding, frames) + " 00000000" * 8
    mp4 = MP4(path)
    if mp4.tags is None:
        mp4.add_tags()
    mp4.tags["----:com.apple.iTunes:iTunSMPB"] = [MP4FreeForm(value.encode("ascii"))]
    mp4.save()


EXT = {
    "wav": ("wav", "pcm", True), "flac": ("flac", "flac", True), "alac": ("m4a", "alac", True),
    "aiff": ("aiff", "pcm", True), "wavpack": ("wv", "wavpack", True), "ape": ("ape", "ape", True),
    "mp3-cbr": ("mp3", "mp3", False), "mp3-vbr": ("mp3", "mp3", False), "aac": ("m4a", "aac", False),
    "aac-itunsmpb": ("m4a", "aac", False), "opus": ("opus", "opus", False), "vorbis": ("ogg", "vorbis", False),
}


def make_audio(lib: Library, rel_stem: str, kind: str, pcm: Pcm, seconds: float, group: str,
               src: Path | None = None, **extra) -> Entry:
    ext, codec, lossless = EXT[kind]
    rel = f"{rel_stem}.{ext}"
    dst = lib.path(rel)
    if src is None:
        src = lib.scratch(f"src_{pcm.tag}_{seconds}.wav")
        if not src.exists():
            synth_wav(lib.tools, src, pcm, seconds)
    encode(lib, src, dst, kind, pcm)
    _, data = wav_data(src)
    entry = Entry(
        id=rel_stem, path=rel, group=group, container=ext, codec=codec, lossless=lossless,
        channels=pcm.channels, sample_rate=pcm.rate if kind != "opus" else 48000,
        bits=pcm.bits if lossless else None, sample_format=pcm.fmt if lossless else None,
        frames=frames_of(data, pcm) if kind != "opus" else None,
        pcm_md5=pcm_md5(data) if lossless else None, **extra,
    )
    if kind == "opus":
        # Opus always decodes at 48 kHz; the source was 48 kHz so the frame count still holds.
        entry.frames = frames_of(data, pcm)
        entry.notes = "Opus header records the input rate; decode rate is always 48 kHz."
    return lib.add(entry)


# --------------------------------------------------------------------------------------------
# Group: formats


S, F = "s", "f"

FORMAT_MATRIX: list[tuple[str, Pcm]] = [
    ("wav", Pcm(44100, 16, S, 2)), ("wav", Pcm(48000, 24, S, 2)), ("wav", Pcm(96000, 24, S, 2)),
    ("wav", Pcm(192000, 24, S, 2)), ("wav", Pcm(384000, 32, S, 2)), ("wav", Pcm(768000, 24, S, 2)),
    ("wav", Pcm(44100, 32, F, 2)), ("wav", Pcm(192000, 32, F, 2)), ("wav", Pcm(44100, 16, S, 1)),
    ("flac", Pcm(44100, 16, S, 2)), ("flac", Pcm(48000, 16, S, 2)), ("flac", Pcm(88200, 24, S, 2)),
    ("flac", Pcm(96000, 24, S, 2)), ("flac", Pcm(176400, 24, S, 2)), ("flac", Pcm(192000, 24, S, 2)),
    ("flac", Pcm(352800, 24, S, 2)), ("flac", Pcm(384000, 24, S, 2)), ("flac", Pcm(96000, 32, S, 2)),
    ("alac", Pcm(44100, 16, S, 2)), ("alac", Pcm(96000, 24, S, 2)), ("alac", Pcm(192000, 24, S, 2)),
    ("aiff", Pcm(44100, 16, S, 2)), ("aiff", Pcm(96000, 24, S, 2)),
    ("wavpack", Pcm(44100, 16, S, 2)), ("wavpack", Pcm(96000, 24, S, 2)),
    ("wavpack", Pcm(192000, 32, F, 2)), ("wavpack", Pcm(384000, 32, S, 2)),
    ("ape", Pcm(44100, 16, S, 2)), ("ape", Pcm(96000, 24, S, 2)),
    ("mp3-cbr", Pcm(44100, 16, S, 2)), ("mp3-vbr", Pcm(48000, 16, S, 2)),
    ("aac", Pcm(44100, 16, S, 2)), ("aac-itunsmpb", Pcm(44100, 16, S, 2)),
    ("opus", Pcm(48000, 16, S, 2)), ("vorbis", Pcm(44100, 16, S, 2)),
]

DSD_MATRIX = [("dsf", 2822400, 1.0), ("dsf", 5644800, 1.0), ("dsf", 11289600, 0.5),
              ("dsf", 22579200, 0.25), ("dff", 2822400, 1.0), ("dff", 5644800, 1.0)]


def group_formats(lib: Library) -> None:
    for kind, pcm in FORMAT_MATRIX:
        make_audio(lib, f"formats/{kind}_{pcm.tag}", kind, pcm, 1.0, "formats")
    for container, rate, seconds in DSD_MATRIX:
        make_dsd(lib, f"formats/{container}_dsd{rate // 44100}", container, rate, seconds, "formats")
    # WavPack can carry DSD too (lossless DSD compression); encoded from our DSF.
    src = lib.path("formats/dsf_dsd64.dsf")
    rel = "formats/wavpack_dsd64.wv"
    run(lib.tools.wavpack, "-q", "-y", src, "-o", lib.path(rel))
    chans = dsd_signal(2822400, 1.0)
    lib.add(Entry(id="formats/wavpack_dsd64", path=rel, group="formats", container="wv", codec="wavpack-dsd",
                  lossless=True, channels=2, sample_format="dsd", dsd_rate=2822400, frames=len(chans[0]),
                  dsd_md5=dsd_canonical_md5(chans)))


def make_dsd(lib: Library, stem: str, container: str, rate: int, seconds: float, group: str) -> Entry:
    chans = dsd_signal(rate, seconds)
    rel = f"{stem}.{container}"
    (write_dsf if container == "dsf" else write_dff)(lib.path(rel), rate, chans)
    return lib.add(Entry(id=stem, path=rel, group=group, container=container, codec="dsd", lossless=True,
                         channels=len(chans), sample_format="dsd", dsd_rate=rate, frames=len(chans[0]),
                         dsd_md5=dsd_canonical_md5(chans)))


# --------------------------------------------------------------------------------------------
# Group: multichannel


def group_multichannel(lib: Library) -> None:
    make_audio(lib, "multichannel/flac_5.1", "flac", Pcm(48000, 24, S, 6), 1.0, "multichannel",
               notes="5.1 (FL FR FC LFE BL BR); each channel a distinct tone (CHANNEL_FREQS order).")
    make_audio(lib, "multichannel/wav_5.1", "wav", Pcm(48000, 24, S, 6), 1.0, "multichannel",
               notes="WAVE_FORMAT_EXTENSIBLE with channel mask 0x3F.")
    make_audio(lib, "multichannel/wav_7.1", "wav", Pcm(48000, 16, S, 8), 1.0, "multichannel")
    make_audio(lib, "multichannel/wavpack_quad", "wavpack", Pcm(44100, 16, S, 4), 1.0, "multichannel")


# --------------------------------------------------------------------------------------------
# Group: gapless


GAPLESS_SPLITS_441 = [0, 100003, 200011, 308700]  # 7 s at 44.1 kHz; cuts deliberately off frame grids
GAPLESS_SPLITS_48 = [0, 109999, 230017, 336000]  # 7 s at 48 kHz
GAPLESS_FREQS = [441.7, 662.3]  # non-integer periods so every cut lands mid-cycle


def group_gapless(lib: Library) -> None:
    for kind in ["wav", "flac", "wavpack", "alac", "mp3-cbr", "aac", "aac-itunsmpb", "opus", "vorbis"]:
        rate = 48000 if kind == "opus" else 44100
        splits = GAPLESS_SPLITS_48 if rate == 48000 else GAPLESS_SPLITS_441
        pcm = Pcm(rate, 16, S, 2)
        full = lib.scratch(f"gapless_full_{rate}.wav")
        if not full.exists():
            synth_wav(lib.tools, full, pcm, splits[-1] / rate, GAPLESS_FREQS)
        _, full_data = wav_data(full)
        for i in range(len(splits) - 1):
            part = lib.scratch(f"gapless_{rate}_{i}.wav")
            run(lib.tools.sox, "-D", full, part, "trim", f"{splits[i]}s", f"{splits[i + 1] - splits[i]}s")
            entry = make_audio(
                lib, f"gapless/{kind}/{i + 1:02d}", kind, pcm, 0, "gapless", src=part,
                gapless={"set": kind, "index": i, "count": len(splits) - 1, "source_frames": splits[-1],
                         "source_md5": pcm_md5(full_data), "freqs_hz": GAPLESS_FREQS, "gain_db": -6.0,
                         "start_frame": splits[i]},
            )
            entry.tags = tag_simple(lib.root / entry.path, kind, title=f"Gapless {kind} part {i + 1}",
                                    album=f"Gapless Set ({kind})", artist="Fixture Tone Generator",
                                    track=i + 1, total=len(splits) - 1)


# --------------------------------------------------------------------------------------------
# Tag writing


RICH: dict[str, list[str]] = {
    "title": ["Ｔｈｅ Rich Title — 東京 🎵"],
    "artist": ["Alpha Artist", "Beta Artist feat. Γάμμα"],
    "album": ["Fixture Album"],
    "albumartist": ["Alpha Artist"],
    "composer": ["Clara Composer", "Дмитрий Композитор"],
    "lyricist": ["Lyric Writer"],
    "conductor": ["Conductor Person"],
    "performer:violin": ["Ann Player"],
    "performer:cello": ["Bob Player"],
    "genre": ["Classical", "Electronic"],
    "date": ["2021-03-04"],
    "originaldate": ["1999-12-31"],
    "tracknumber": ["3"],
    "tracktotal": ["12"],
    "discnumber": ["1"],
    "disctotal": ["2"],
    "discsubtitle": ["The First Disc"],
    "compilation": ["1"],
    "grouping": ["Fixture Grouping"],
    "work": ["Symphony No. 9 in D minor"],
    "movementname": ["Allegro ma non troppo"],
    "movement": ["1"],
    "movementtotal": ["4"],
    "bpm": ["128"],
    "key": ["Am"],
    "mood": ["Calm"],
    "comment": ["Line one\nLine two"],
    "lyrics": ["First line\nSecond line"],
    "isrc": ["USRC17607839"],
    "barcode": ["0123456789012"],
    "catalognumber": ["CAT-001"],
    "label": ["Fixture Records"],
    "releasetype": ["album", "live"],
    "releasecountry": ["GB"],
    "musicbrainz_trackid": ["5b11f4ce-a62d-471e-81fc-a69a8278c7da"],
    "musicbrainz_albumid": ["9e6b2c5f-3e4a-4b8e-9b0a-0f0e2c3d4b5a"],
    "musicbrainz_artistid": ["a74b1b7f-71a5-4011-9441-d0b5e4122711", "b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"],
    "musicbrainz_albumartistid": ["a74b1b7f-71a5-4011-9441-d0b5e4122711"],
    "musicbrainz_releasegroupid": ["1d4b6a5e-2f3c-4a8b-9c0d-1e2f3a4b5c6d"],
    "musicbrainz_workid": ["2e5c7b6f-3a4d-4b9c-8d1e-2f3a4b5c6d7e"],
    "replaygain_track_gain": ["-7.89 dB"],
    "replaygain_track_peak": ["0.987654"],
    "replaygain_album_gain": ["-8.12 dB"],
    "replaygain_album_peak": ["0.999000"],
    "titlesort": ["Rich Title, The"],
    "artistsort": ["Artist, Alpha", "Artist, Beta"],
    "albumsort": ["Fixture Album"],
    "albumartistsort": ["Artist, Alpha"],
    "composersort": ["Composer, Clara", "Композитор, Дмитрий"],
    "custom:MY CUSTOM FIELD": ["custom value", "second custom value"],
    "rating": ["80"],  # 0-100
    "playcount": ["7"],
}

# Picard-compatible names (https://picard-docs.musicbrainz.org/en/appendices/tag_mapping.html).
VORBIS_KEYS = {
    "title": "TITLE", "artist": "ARTIST", "album": "ALBUM", "albumartist": "ALBUMARTIST", "composer": "COMPOSER",
    "lyricist": "LYRICIST", "conductor": "CONDUCTOR", "genre": "GENRE", "date": "DATE",
    "originaldate": "ORIGINALDATE", "tracknumber": "TRACKNUMBER", "tracktotal": "TRACKTOTAL",
    "discnumber": "DISCNUMBER", "disctotal": "DISCTOTAL", "discsubtitle": "DISCSUBTITLE",
    "compilation": "COMPILATION", "grouping": "GROUPING", "work": "WORK", "movementname": "MOVEMENTNAME",
    "movement": "MOVEMENT", "movementtotal": "MOVEMENTTOTAL", "bpm": "BPM", "key": "KEY", "mood": "MOOD",
    "comment": "COMMENT", "lyrics": "LYRICS", "isrc": "ISRC", "barcode": "BARCODE",
    "catalognumber": "CATALOGNUMBER", "label": "LABEL", "releasetype": "RELEASETYPE",
    "releasecountry": "RELEASECOUNTRY", "musicbrainz_trackid": "MUSICBRAINZ_TRACKID",
    "musicbrainz_albumid": "MUSICBRAINZ_ALBUMID", "musicbrainz_artistid": "MUSICBRAINZ_ARTISTID",
    "musicbrainz_albumartistid": "MUSICBRAINZ_ALBUMARTISTID",
    "musicbrainz_releasegroupid": "MUSICBRAINZ_RELEASEGROUPID", "musicbrainz_workid": "MUSICBRAINZ_WORKID",
    "replaygain_track_gain": "REPLAYGAIN_TRACK_GAIN", "replaygain_track_peak": "REPLAYGAIN_TRACK_PEAK",
    "replaygain_album_gain": "REPLAYGAIN_ALBUM_GAIN", "replaygain_album_peak": "REPLAYGAIN_ALBUM_PEAK",
    "titlesort": "TITLESORT", "artistsort": "ARTISTSORT", "albumsort": "ALBUMSORT",
    "albumartistsort": "ALBUMARTISTSORT", "composersort": "COMPOSERSORT",
}

APE_KEYS = dict(VORBIS_KEYS) | {
    "title": "Title", "artist": "Artist", "album": "Album", "albumartist": "Album Artist",
    "composer": "Composer", "lyricist": "Lyricist", "conductor": "Conductor", "genre": "Genre",
    "date": "Year", "discsubtitle": "DiscSubtitle", "compilation": "Compilation", "grouping": "Grouping",
    "comment": "Comment", "lyrics": "Lyrics", "isrc": "ISRC", "barcode": "Barcode",
    "catalognumber": "CatalogNumber", "label": "Label", "releasetype": "MUSICBRAINZ_ALBUMTYPE",
    "musicbrainz_trackid": "MUSICBRAINZ_TRACKID", "bpm": "BPM", "key": "Key", "mood": "Mood",
    "originaldate": "ORIGINALDATE",
}
for _k in ("tracknumber", "tracktotal", "discnumber", "disctotal"):
    APE_KEYS.pop(_k)

MP4_KEYS = {
    "title": "©nam", "artist": "©ART", "album": "©alb", "albumartist": "aART", "composer": "©wrt",
    "genre": "©gen", "date": "©day", "grouping": "©grp", "work": "©wrk", "movementname": "©mvn",
    "comment": "©cmt", "lyrics": "©lyr", "titlesort": "sonm", "artistsort": "soar", "albumsort": "soal",
    "albumartistsort": "soaa", "composersort": "soco",
}
MP4_FREEFORM = {
    "lyricist": "LYRICIST", "conductor": "CONDUCTOR", "originaldate": "ORIGINALDATE",
    "discsubtitle": "DISCSUBTITLE", "key": "initialkey", "mood": "MOOD", "isrc": "ISRC", "barcode": "BARCODE",
    "catalognumber": "CATALOGNUMBER", "label": "LABEL", "releasetype": "MusicBrainz Album Type",
    "releasecountry": "MusicBrainz Album Release Country", "musicbrainz_trackid": "MusicBrainz Track Id",
    "musicbrainz_albumid": "MusicBrainz Album Id", "musicbrainz_artistid": "MusicBrainz Artist Id",
    "musicbrainz_albumartistid": "MusicBrainz Album Artist Id",
    "musicbrainz_releasegroupid": "MusicBrainz Release Group Id", "musicbrainz_workid": "MusicBrainz Work Id",
    "replaygain_track_gain": "replaygain_track_gain", "replaygain_track_peak": "replaygain_track_peak",
    "replaygain_album_gain": "replaygain_album_gain", "replaygain_album_peak": "replaygain_album_peak",
}

ID3_TEXT = {
    "title": "TIT2", "artist": "TPE1", "album": "TALB", "albumartist": "TPE2", "composer": "TCOM",
    "lyricist": "TEXT", "conductor": "TPE3", "genre": "TCON", "date": "TDRC", "originaldate": "TDOR",
    "discsubtitle": "TSST", "compilation": "TCMP", "grouping": "TIT1", "movementname": "MVNM", "bpm": "TBPM",
    "key": "TKEY", "mood": "TMOO", "isrc": "TSRC", "label": "TPUB", "titlesort": "TSOT",
    "artistsort": "TSOP", "albumsort": "TSOA", "albumartistsort": "TSO2", "composersort": "TSOC",
}
ID3_TXXX = {
    "work": "WORK", "barcode": "BARCODE", "catalognumber": "CATALOGNUMBER",
    "releasetype": "MusicBrainz Album Type", "releasecountry": "MusicBrainz Album Release Country",
    "musicbrainz_albumid": "MusicBrainz Album Id", "musicbrainz_artistid": "MusicBrainz Artist Id",
    "musicbrainz_albumartistid": "MusicBrainz Album Artist Id",
    "musicbrainz_releasegroupid": "MusicBrainz Release Group Id", "musicbrainz_workid": "MusicBrainz Work Id",
    "replaygain_track_gain": "REPLAYGAIN_TRACK_GAIN", "replaygain_track_peak": "REPLAYGAIN_TRACK_PEAK",
    "replaygain_album_gain": "REPLAYGAIN_ALBUM_GAIN", "replaygain_album_peak": "REPLAYGAIN_ALBUM_PEAK",
}
# Frames that ID3v2.3 cannot represent: absent from expectations for v2.3 files.
ID3_V24_ONLY = {"discsubtitle", "mood", "titlesort", "artistsort", "albumsort", "performer:violin",
                "performer:cello"}

POPM_EMAIL = "no@email"  # Picard's default

SYNCED_LYRICS = [("First line", 1000), ("Second line", 4500)]  # (text, ms)


def performers(fields: dict[str, list[str]]) -> list[tuple[str, str]]:
    return [(k.split(":", 1)[1], v) for k, vs in fields.items() if k.startswith("performer:") for v in vs]


def customs(fields: dict[str, list[str]]) -> dict[str, list[str]]:
    return {k.split(":", 1)[1]: v for k, v in fields.items() if k.startswith("custom:")}


def rating_0_1(fields) -> str:
    return "%.1f" % (int(fields["rating"][0]) / 100)


def write_vorbis_like(tags, fields: dict[str, list[str]]) -> list:
    native = []
    for f, key in VORBIS_KEYS.items():
        if f in fields:
            tags[key] = fields[f]
            native.append([key, fields[f]])
    perf = [f"{name} ({role})" for role, name in performers(fields)]
    if perf:
        tags["PERFORMER"] = perf
        native.append(["PERFORMER", perf])
    for k, v in customs(fields).items():
        tags[k] = v
        native.append([k, v])
    if "rating" in fields:
        tags["FMPS_RATING"] = [rating_0_1(fields)]
        tags["FMPS_PLAYCOUNT"] = fields["playcount"]
        native += [["FMPS_RATING", [rating_0_1(fields)]], ["FMPS_PLAYCOUNT", fields["playcount"]]]
    return native


def write_ape(tags, fields: dict[str, list[str]]) -> list:
    native = []

    def put(k, v):
        tags[k] = v
        native.append([k, v])

    for f, key in APE_KEYS.items():
        if f in fields:
            put(key, fields[f])
    if "tracknumber" in fields:
        put("Track", [f"{fields['tracknumber'][0]}/{fields['tracktotal'][0]}"])
    if "discnumber" in fields:
        put("Disc", [f"{fields['discnumber'][0]}/{fields['disctotal'][0]}"])
    perf = [f"{name} ({role})" for role, name in performers(fields)]
    if perf:
        put("Performer", perf)
    for k, v in customs(fields).items():
        put(k, v)
    if "rating" in fields:
        put("FMPS_RATING", [rating_0_1(fields)])
        put("FMPS_PLAYCOUNT", fields["playcount"])
    return native


def write_mp4(tags, fields: dict[str, list[str]]) -> list:
    native = []
    for f, key in MP4_KEYS.items():
        if f in fields:
            tags[key] = fields[f]
            native.append([key, fields[f]])
    for f, name in MP4_FREEFORM.items():
        if f in fields:
            key = f"----:com.apple.iTunes:{name}"
            tags[key] = [MP4FreeForm(v.encode()) for v in fields[f]]
            native.append([key, fields[f]])
    if "tracknumber" in fields:
        tags["trkn"] = [(int(fields["tracknumber"][0]), int(fields["tracktotal"][0]))]
        native.append(["trkn", [f"{fields['tracknumber'][0]}/{fields['tracktotal'][0]}"]])
    if "discnumber" in fields:
        tags["disk"] = [(int(fields["discnumber"][0]), int(fields["disctotal"][0]))]
        native.append(["disk", [f"{fields['discnumber'][0]}/{fields['disctotal'][0]}"]])
    if "bpm" in fields:
        tags["tmpo"] = [int(fields["bpm"][0])]
        native.append(["tmpo", fields["bpm"]])
    if "compilation" in fields:
        tags["cpil"] = fields["compilation"][0] == "1"
        native.append(["cpil", fields["compilation"]])
    if "movement" in fields:
        tags["©mvi"] = [int(fields["movement"][0])]
        tags["©mvc"] = [int(fields["movementtotal"][0])]
        native += [["©mvi", fields["movement"]], ["©mvc", fields["movementtotal"]]]
    perf = [f"{name} ({role})" for role, name in performers(fields)]
    if perf:
        tags["----:com.apple.iTunes:PERFORMER"] = [MP4FreeForm(p.encode()) for p in perf]
        native.append(["----:com.apple.iTunes:PERFORMER", perf])
    for k, v in customs(fields).items():
        key = f"----:com.apple.iTunes:{k}"
        tags[key] = [MP4FreeForm(x.encode()) for x in v]
        native.append([key, v])
    if "rating" in fields:
        tags["----:com.apple.iTunes:FMPS_Rating"] = [MP4FreeForm(rating_0_1(fields).encode())]
        tags["----:com.apple.iTunes:FMPS_Playcount"] = [MP4FreeForm(fields["playcount"][0].encode())]
        native += [["----:com.apple.iTunes:FMPS_Rating", [rating_0_1(fields)]],
                   ["----:com.apple.iTunes:FMPS_Playcount", fields["playcount"]]]
    return native


def write_id3(tags: id3.ID3, fields: dict[str, list[str]], v23: bool) -> list:
    native = []
    enc = id3.Encoding.UTF16 if v23 else id3.Encoding.UTF8
    for f, fid in ID3_TEXT.items():
        if f in fields and not (v23 and f in ID3_V24_ONLY):
            tags.add(getattr(id3, fid)(encoding=enc, text=fields[f]))
            native.append([fid, fields[f]])
    for f, desc in ID3_TXXX.items():
        if f in fields:
            tags.add(id3.TXXX(encoding=enc, desc=desc, text=fields[f]))
            native.append([f"TXXX:{desc}", fields[f]])
    if "tracknumber" in fields:
        v = f"{fields['tracknumber'][0]}/{fields['tracktotal'][0]}"
        tags.add(id3.TRCK(encoding=enc, text=[v]))
        native.append(["TRCK", [v]])
    if "discnumber" in fields:
        v = f"{fields['discnumber'][0]}/{fields['disctotal'][0]}"
        tags.add(id3.TPOS(encoding=enc, text=[v]))
        native.append(["TPOS", [v]])
    if "movement" in fields:
        v = f"{fields['movement'][0]}/{fields['movementtotal'][0]}"
        tags.add(id3.MVIN(encoding=enc, text=[v]))
        native.append(["MVIN", [v]])
    if "comment" in fields:
        tags.add(id3.COMM(encoding=enc, lang="eng", desc="", text=fields["comment"]))
        native.append(["COMM::eng", fields["comment"]])
    if "lyrics" in fields:
        tags.add(id3.USLT(encoding=enc, lang="eng", desc="", text=fields["lyrics"][0]))
        native.append(["USLT::eng", fields["lyrics"]])
        tags.add(id3.SYLT(encoding=enc, lang="eng", format=2, type=1, desc="", text=SYNCED_LYRICS))
        native.append(["SYLT::eng", [f"{t}@{ms}" for t, ms in SYNCED_LYRICS]])
    if "musicbrainz_trackid" in fields:
        tags.add(id3.UFID(owner="http://musicbrainz.org", data=fields["musicbrainz_trackid"][0].encode()))
        native.append(["UFID:http://musicbrainz.org", fields["musicbrainz_trackid"]])
    perf = performers(fields)
    if perf and not v23:
        tags.add(id3.TMCL(encoding=enc, people=[[role, name] for role, name in perf]))
        native.append(["TMCL", [f"{role}={name}" for role, name in perf]])
    for k, v in customs(fields).items():
        tags.add(id3.TXXX(encoding=enc, desc=k, text=v))
        native.append([f"TXXX:{k}", v])
    if "rating" in fields:
        popm = round(int(fields["rating"][0]) * 255 / 100)
        tags.add(id3.POPM(email=POPM_EMAIL, rating=popm, count=int(fields["playcount"][0])))
        native.append([f"POPM:{POPM_EMAIL}", [f"rating={popm}", f"count={fields['playcount'][0]}"]])
    return native


def expected_fields(fields: dict[str, list[str]], tagfmt: str) -> dict[str, list[str]]:
    """What a correct reader returns for these written fields in this tag format."""
    out = dict(fields)
    if tagfmt == "id3v2.3":
        for f in ID3_V24_ONLY:
            out.pop(f, None)
        out["originaldate"] = [fields["originaldate"][0][:4]] if "originaldate" in fields else []
        if not out["originaldate"]:
            out.pop("originaldate")
        # ID3v2.3 has no multi-value text frames; mutagen joins values with "/" (Picard's default).
        # A reader must not split them back (think "AC/DC"), so the joined string is what it sees.
        for f, values in list(out.items()):
            text_frame = f in ID3_TEXT or f in ID3_TXXX or f.startswith("custom:")
            if text_frame and len(values) > 1:
                out[f] = ["/".join(values)]
    return out


# --------------------------------------------------------------------------------------------
# Unknown-item preservation helpers


def id3_append_raw_frame(path: Path, frame_id: bytes, payload: bytes) -> dict:
    """Insert a raw frame with an ID no library knows into an existing ID3v2 tag's padding.

    Works for a tag at file start (MP3) or embedded at a known offset (found by searching).
    """
    blob = bytearray(path.read_bytes())
    start = blob.find(b"ID3")
    assert start >= 0, path
    major = blob[start + 3]
    size = id3_syncsafe(blob[start + 6:start + 10])
    body_start = start + 10
    end = body_start + size
    pos = body_start
    while pos + 10 <= end and blob[pos] != 0:
        fsize = id3_syncsafe(blob[pos + 4:pos + 8]) if major == 4 else struct.unpack(">I", blob[pos + 4:pos + 8])[0]
        pos += 10 + fsize
    frame_size = id3_to_syncsafe(len(payload)) if major == 4 else struct.pack(">I", len(payload))
    frame = frame_id + frame_size + b"\0\0" + payload
    assert pos + len(frame) <= end, f"not enough ID3 padding in {path}"
    blob[pos:pos + len(frame)] = frame
    path.write_bytes(bytes(blob))
    return {"kind": "id3-frame", "id": frame_id.decode(), "data_hex": payload.hex()}


def id3_syncsafe(b: bytes) -> int:
    return (b[0] << 21) | (b[1] << 14) | (b[2] << 7) | b[3]


def id3_to_syncsafe(n: int) -> bytes:
    return bytes([(n >> 21) & 0x7F, (n >> 14) & 0x7F, (n >> 7) & 0x7F, n & 0x7F])


def riff_append_chunk(path: Path, cid: bytes, body: bytes, big_endian: bool = False) -> dict:
    blob = bytearray(path.read_bytes())
    fmt = ">I" if big_endian else "<I"
    chunk = cid + struct.pack(fmt, len(body)) + body + (b"\0" if len(body) & 1 else b"")
    blob += chunk
    blob[4:8] = struct.pack(fmt, len(blob) - 8)
    path.write_bytes(bytes(blob))
    return {"kind": "riff-chunk" if not big_endian else "iff-chunk", "id": cid.decode(), "data_hex": body.hex()}


def riff_info_list(items: list[tuple[bytes, str]]) -> bytes:
    body = b"INFO"
    for cid, text in items:
        data = text.encode("utf-8") + b"\0"
        body += cid + struct.pack("<I", len(data)) + data + (b"\0" if len(data) & 1 else b"")
    return body


class ApplicationBlock(MetadataBlock):
    code = 2  # FLAC APPLICATION block


# --------------------------------------------------------------------------------------------
# Artwork


def make_image(lib: Library, name: str, w: int, h: int, fmt: str, pattern: str = "testsrc2") -> Path:
    out = lib.scratch(f"{name}.{fmt}")
    if not out.exists():
        src = f"{pattern}=size={w}x{h}:rate=1" if pattern != "noise" else f"color=c=gray:size={w}x{h}:rate=1"
        vf = ["-vf", "noise=alls=100:allf=u"] if pattern == "noise" else []
        q = ["-q:v", "2"] if fmt == "jpg" else []
        run(lib.tools.ffmpeg, "-v", "error", "-y", "-f", "lavfi", "-i", src, *vf, "-frames:v", "1", *q, out)
    return out


def image_info(path: Path, picture_type: int, w: int, h: int) -> dict:
    data = path.read_bytes()
    mime = {"jpg": "image/jpeg", "png": "image/png", "webp": "image/webp"}[path.suffix[1:]]
    return {"type": picture_type, "mime": mime, "width": w, "height": h, "bytes": len(data),
            "sha1": hashlib.sha1(data).hexdigest()}


def flac_picture(path: Path, ptype: int, w: int, h: int, desc: str = "") -> Picture:
    pic = Picture()
    pic.type = ptype
    pic.mime = {"jpg": "image/jpeg", "png": "image/png"}[path.suffix[1:]]
    pic.desc = desc
    pic.width, pic.height, pic.depth = w, h, 24
    pic.data = path.read_bytes()
    return pic


# --------------------------------------------------------------------------------------------
# Group: tags (rich metadata + unknown items in every tag container)


def group_tags(lib: Library) -> None:
    front = make_image(lib, "front_600", 600, 600, "jpg")
    back = make_image(lib, "back_400", 400, 400, "png", "smptebars")
    art = [image_info(front, 3, 600, 600), image_info(back, 4, 400, 400)]
    pcm16 = Pcm(44100, 16, S, 2)

    # FLAC: Vorbis comments + pictures + an APPLICATION block + padding.
    e = make_audio(lib, "tags/flac_vorbis", "flac", Pcm(96000, 24, S, 2), 1.0, "tags")
    f = FLAC(lib.root / e.path)
    native = write_vorbis_like(f, RICH)
    f["CADENZA_UNKNOWN_FIELD"] = ["must survive"]
    f.add_picture(flac_picture(front, 3, 600, 600))
    f.add_picture(flac_picture(back, 4, 400, 400, "back"))
    app = ApplicationBlock(b"CDZA" + bytes(range(32)))
    f.metadata_blocks.append(app)
    f.save()
    e.tags, e.native, e.artwork = expected_fields(RICH, "vorbis"), {"format": "vorbis", "items": native}, art
    e.preserve = [{"kind": "vorbis-field", "id": "CADENZA_UNKNOWN_FIELD", "value": "must survive"},
                  {"kind": "flac-block", "id": "APPLICATION", "data_hex": (b"CDZA" + bytes(range(32))).hex()}]

    # Ogg Vorbis and Opus: Vorbis comments + METADATA_BLOCK_PICTURE.
    for kind, cls, stem in [("vorbis", OggVorbis, "ogg_vorbis"), ("opus", OggOpus, "opus")]:
        pcm = Pcm(48000, 16, S, 2) if kind == "opus" else pcm16
        e = make_audio(lib, f"tags/{stem}", kind, pcm, 1.0, "tags")
        o = cls(lib.root / e.path)
        native = write_vorbis_like(o, RICH)
        o["CADENZA_UNKNOWN_FIELD"] = ["must survive"]
        o["METADATA_BLOCK_PICTURE"] = [base64.b64encode(flac_picture(front, 3, 600, 600).write()).decode()]
        o.save()
        e.tags, e.native, e.artwork = expected_fields(RICH, "vorbis"), {"format": "vorbis", "items": native}, art[:1]
        e.preserve = [{"kind": "vorbis-field", "id": "CADENZA_UNKNOWN_FIELD", "value": "must survive"}]

    # MP3 with ID3v2.4 and v2.3 (+ ID3v1), including PRIV/GEOB, chapters and a raw unknown frame.
    for v23 in (False, True):
        ver = "id3v2.3" if v23 else "id3v2.4"
        e = make_audio(lib, f"tags/mp3_{ver.replace('.', '')}", "mp3-cbr", pcm16, 1.0, "tags")
        p = lib.root / e.path
        tags = id3.ID3()
        native = write_id3(tags, RICH, v23)
        tags.add(id3.APIC(encoding=3, mime="image/jpeg", type=3, desc="", data=front.read_bytes()))
        tags.add(id3.APIC(encoding=3, mime="image/png", type=4, desc="back", data=back.read_bytes()))
        tags.add(id3.PRIV(owner="com.example.cadenza", data=b"\x00\x01private\xff"))
        tags.add(id3.GEOB(encoding=3, mime="application/octet-stream", filename="blob.bin", desc="fixture",
                          data=bytes(range(64))))
        tags.add(id3.TXXX(encoding=3, desc="CADENZA_UNKNOWN_FIELD", text=["must survive"]))
        tags.add(id3.COMM(encoding=3, lang="deu", desc="second", text=["Zweiter Kommentar"]))
        tags.add(id3.CTOC(element_id="toc", flags=id3.CTOCFlags.TOP_LEVEL | id3.CTOCFlags.ORDERED,
                          child_element_ids=["ch0", "ch1"], sub_frames=[]))
        tags.add(id3.CHAP(element_id="ch0", start_time=0, end_time=500, sub_frames=[id3.TIT2(text=["Intro"])]))
        tags.add(id3.CHAP(element_id="ch1", start_time=500, end_time=1000, sub_frames=[id3.TIT2(text=["Outro"])]))
        if v23:
            tags.update_to_v23()  # save(v2_version=3) alone would leave v2.4 frames (TDRC...) in place
        tags.save(p, v2_version=3 if v23 else 4, v1=2 if v23 else 0, padding=lambda info: 2048)
        raw = id3_append_raw_frame(p, b"XCDZ", b"\x01unknown frame payload\x00\xfe")
        e.tags, e.native, e.artwork = expected_fields(RICH, ver), {"format": ver, "items": native}, art
        e.preserve = [raw,
                      {"kind": "id3-frame", "id": "PRIV", "owner": "com.example.cadenza"},
                      {"kind": "id3-frame", "id": "GEOB", "desc": "fixture"},
                      {"kind": "id3-frame", "id": "TXXX", "desc": "CADENZA_UNKNOWN_FIELD"},
                      {"kind": "id3-frame", "id": "COMM", "lang": "deu", "desc": "second"},
                      {"kind": "id3-frame", "id": "CTOC"}, {"kind": "id3-frame", "id": "CHAP", "count": 2}]
        if v23:
            e.notes = "Also carries an ID3v1 tag (title truncated to 30 bytes, Latin-1 lossy)."

    # MP4: AAC and ALAC with atoms, freeform atoms, an unknown text atom and two covers.
    for kind, stem, pcm in [("aac", "m4a_aac", pcm16), ("alac", "m4a_alac", Pcm(96000, 24, S, 2))]:
        e = make_audio(lib, f"tags/{stem}", kind, pcm, 1.0, "tags")
        m = MP4(lib.root / e.path)
        if m.tags is None:
            m.add_tags()
        native = write_mp4(m.tags, RICH)
        m.tags["covr"] = [MP4Cover(front.read_bytes(), MP4Cover.FORMAT_JPEG),
                          MP4Cover(back.read_bytes(), MP4Cover.FORMAT_PNG)]
        m.tags["----:com.example.cadenza:UNKNOWN"] = [MP4FreeForm(b"must survive")]
        m.tags["xcdz"] = ["unknown atom text"]
        m.save()
        e.tags, e.native = expected_fields(RICH, "mp4"), {"format": "mp4", "items": native}
        e.artwork = [dict(art[0], type=3), dict(art[1], type=3)]
        e.preserve = [{"kind": "mp4-freeform", "id": "----:com.example.cadenza:UNKNOWN", "value": "must survive"},
                      {"kind": "mp4-atom", "id": "xcdz", "value": "unknown atom text"}]
        e.notes = "MP4 covers have no picture type; both read as front (3). No synced lyrics in MP4."

    # APEv2 in WavPack and Monkey's Audio, with binary and external-link items.
    for kind, stem in [("wavpack", "wv_apev2"), ("ape", "ape_apev2")]:
        e = make_audio(lib, f"tags/{stem}", kind, Pcm(44100, 16, S, 2), 1.0, "tags")
        a = apev2.APEv2()
        native = write_ape(a, RICH)
        a["Cover Art (Front)"] = apev2.APEValue(b"front.jpg\0" + front.read_bytes(), apev2.BINARY)
        a["CADENZA_UNKNOWN_BINARY"] = apev2.APEValue(bytes(range(48)), apev2.BINARY)
        a["Related"] = apev2.APEValue("https://example.invalid/fixture", apev2.EXTERNAL)
        a["CADENZA_UNKNOWN_FIELD"] = "must survive"
        a.save(lib.root / e.path)
        e.tags, e.native, e.artwork = expected_fields(RICH, "apev2"), {"format": "apev2", "items": native}, art[:1]
        e.preserve = [{"kind": "ape-item", "id": "CADENZA_UNKNOWN_BINARY", "data_hex": bytes(range(48)).hex()},
                      {"kind": "ape-item", "id": "Related", "value": "https://example.invalid/fixture"},
                      {"kind": "ape-item", "id": "CADENZA_UNKNOWN_FIELD", "value": "must survive"}]

    # WAV: LIST/INFO + unknown chunk + ID3 chunk.  AIFF: unknown chunk + ID3 (v2.3).
    for kind, stem, ver in [("wav", "wav_id3", "id3v2.4"), ("aiff", "aiff_id3", "id3v2.3")]:
        e = make_audio(lib, f"tags/{stem}", kind, Pcm(48000, 24, S, 2), 1.0, "tags")
        p = lib.root / e.path
        preserve = []
        if kind == "wav":
            preserve.append(riff_append_chunk(p, b"LIST", riff_info_list([(b"INAM", "INFO Title"),
                                                                          (b"IART", "INFO Artist")])))
            preserve.append(riff_append_chunk(p, b"cdza", b"unknown riff chunk payload"))
            container = WAVE(p)
        else:
            preserve.append(riff_append_chunk(p, b"CDZA", b"unknown iff chunk payload", big_endian=True))
            container = AIFF(p)
        container.add_tags()
        native = write_id3(container.tags, RICH, ver == "id3v2.3")
        container.tags.add(id3.APIC(encoding=3, mime="image/jpeg", type=3, desc="", data=front.read_bytes()))
        if ver == "id3v2.3":
            container.tags.update_to_v23()
        container.save(v2_version=3 if ver == "id3v2.3" else 4)
        e.tags, e.native, e.artwork = expected_fields(RICH, ver), {"format": ver, "items": native}, art[:1]
        e.preserve = preserve

    # DSF (ID3v2.4 at the end of file) and DFF (ID3 chunk).
    for container, cls in [("dsf", DSF), ("dff", DSDIFF)]:
        e = make_dsd(lib, f"tags/{container}_id3", container, 2822400, 1.0, "tags")
        d = cls(lib.root / e.path)
        d.add_tags()
        native = write_id3(d.tags, RICH, False)
        d.tags.add(id3.APIC(encoding=3, mime="image/jpeg", type=3, desc="", data=front.read_bytes()))
        d.save()
        e.tags, e.native, e.artwork = expected_fields(RICH, "id3v2.4"), {"format": "id3v2.4", "items": native}, art[:1]
        if container == "dff":
            e.notes = "The DSDIFF 'ID3 ' chunk is a de-facto extension, not in the DSDIFF 1.5 spec."


def tag_simple(path: Path, kind: str, *, title: str, album: str, artist: str, track: int, total: int,
               extra: dict[str, list[str]] | None = None) -> dict[str, list[str]]:
    fields = {"title": [title], "album": [album], "artist": [artist], "albumartist": [artist],
              "tracknumber": [str(track)], "tracktotal": [str(total)]}
    fields |= extra or {}
    write_fields(path, kind, fields)
    return fields


def write_fields(path: Path, kind: str, fields: dict[str, list[str]], art: Path | None = None) -> None:
    if kind == "flac":
        f = FLAC(path)
        write_vorbis_like(f, fields)
        if art:
            f.add_picture(flac_picture(art, 3, 600, 600))
        f.save()
    elif kind in ("vorbis", "opus"):
        o = (OggVorbis if kind == "vorbis" else OggOpus)(path)
        write_vorbis_like(o, fields)
        o.save()
    elif kind.startswith("mp3"):
        t = id3.ID3()
        write_id3(t, fields, False)
        if art:
            t.add(id3.APIC(encoding=3, mime="image/jpeg", type=3, desc="", data=art.read_bytes()))
        t.save(path)
    elif kind in ("aac", "aac-itunsmpb", "alac"):
        m = MP4(path)
        if m.tags is None:
            m.add_tags()
        write_mp4(m.tags, fields)
        m.save()
    elif kind in ("wavpack", "ape"):
        a = apev2.APEv2()
        write_ape(a, fields)
        a.save(path)
    elif kind in ("wav", "aiff"):
        c = (WAVE if kind == "wav" else AIFF)(path)
        c.add_tags()
        write_id3(c.tags, fields, False)
        c.save()
    else:
        raise ValueError(kind)


# --------------------------------------------------------------------------------------------
# Group: unicode and legacy encodings


UNICODE_CASES = {
    "cjk": "東京事変 — 群青日和",
    "emoji": "Party 🎉🎶 Mix 👩‍👩‍👧",  # astral + ZWJ sequence (UTF-16 surrogates in ID3v2.3)
    "rtl": "فيروز — שלום",
    "nfd": "Café Olé",  # decomposed accents: must not be normalised on write
    "long": "L" + "o" * 998 + "ng",
    "separators": "AC/DC; Earth, Wind & Fire",  # must stay a single value
}

LEGACY = [
    # (case, charset, text): stored as raw bytes in an ID3v2.3 Latin-1 frame
    ("shift_jis", "shift_jis", "東京事変"),
    ("cp1251", "cp1251", "Кино — Группа крови"),
    ("latin1", "latin-1", "Björk - Jóga"),
]


def group_encodings(lib: Library) -> None:
    pcm = Pcm(44100, 16, S, 2)
    for case, text in UNICODE_CASES.items():
        for kind in ("flac", "mp3-cbr"):
            ext = EXT[kind][0]
            # Unicode in the file name too (except the 1000-char case, which would exceed NAME_MAX).
            name = f"{case}_{text[:40]}" if case != "long" else case
            name = name.replace("/", "∕")
            e = make_audio(lib, f"encodings/unicode/{ext}/{name}", kind, pcm, 0.5, "encodings")
            fields = {"title": [text], "artist": [text], "album": ["Unicode Fixtures"]}
            write_fields(lib.root / e.path, kind, fields)
            e.tags = fields
        # ID3v2.3 stores Unicode as UTF-16 with BOM; emoji need surrogate pairs.
        e = make_audio(lib, f"encodings/unicode/mp3_v23/{case}", "mp3-cbr", pcm, 0.5, "encodings")
        t = id3.ID3()
        t.add(id3.TIT2(encoding=id3.Encoding.UTF16, text=[text]))
        t.save(lib.root / e.path, v2_version=3)
        e.tags = {"title": [text]}

    for case, charset, text in LEGACY:
        e = make_audio(lib, f"encodings/legacy/{case}", "mp3-cbr", pcm, 0.5, "encodings")
        raw = text.encode(charset)
        mangled = raw.decode("latin-1")  # mutagen writes this back out as the original bytes
        t = id3.ID3()
        t.add(id3.TIT2(encoding=id3.Encoding.LATIN1, text=[mangled]))
        t.add(id3.TPE1(encoding=id3.Encoding.LATIN1, text=[mangled]))
        t.save(lib.root / e.path, v2_version=3, v1=2)
        e.tags = {"title": [text], "artist": [text]}
        e.expect = "ok"
        e.notes = (f"Frames are flagged Latin-1 but hold {charset} bytes ({raw.hex()}); a correct reader "
                   f"shows the mojibake by default and the encoding fixer must offer {charset}. "
                   "An ID3v1 tag with the same bytes is present.")
        e.native = {"format": "id3v2.3", "legacy_charset": charset, "raw_hex": raw.hex()}


# --------------------------------------------------------------------------------------------
# Group: artwork


def group_artwork(lib: Library) -> None:
    pcm = Pcm(44100, 16, S, 2)
    # FLAC metadata blocks are limited to 16 MiB, so the FLAC case stays just under that; the
    # MP3 case (ID3v2 frames may be up to 256 MB) carries the truly huge image.
    big = make_image(lib, "big_noise_3500", 3500, 3500, "jpg", "noise")
    assert big.stat().st_size < (1 << 24) - 64, "FLAC fixture art must fit in one metadata block"
    e = make_audio(lib, "artwork/flac_huge_embedded", "flac", pcm, 0.5, "artwork")
    f = FLAC(lib.root / e.path)
    f.add_picture(flac_picture(big, 3, 3500, 3500))
    f.save()
    e.artwork = [image_info(big, 3, 3500, 3500)]
    e.notes = "Embedded front cover is a %.1f MB 3500x3500 noise JPEG." % (big.stat().st_size / 1e6)

    huge = make_image(lib, "huge_noise_5000", 5000, 5000, "jpg", "noise")
    e = make_audio(lib, "artwork/mp3_huge_embedded", "mp3-cbr", pcm, 0.5, "artwork")
    t = id3.ID3()
    t.add(id3.APIC(encoding=3, mime="image/jpeg", type=3, desc="", data=huge.read_bytes()))
    t.save(lib.root / e.path)
    e.artwork = [image_info(huge, 3, 5000, 5000)]
    e.notes = "Embedded front cover is a %.1f MB 5000x5000 noise JPEG." % (huge.stat().st_size / 1e6)

    types = [(3, "front"), (4, "back"), (8, "artist"), (6, "media"), (0, "other")]
    e = make_audio(lib, "artwork/flac_many_types", "flac", pcm, 0.5, "artwork")
    f = FLAC(lib.root / e.path)
    for i, (ptype, desc) in enumerate(types):
        img = make_image(lib, f"type_{desc}", 300 + i * 10, 300 + i * 10, "png", "testsrc")
        f.add_picture(flac_picture(img, ptype, 300 + i * 10, 300 + i * 10, desc))
        e.artwork.append(image_info(img, ptype, 300 + i * 10, 300 + i * 10))
    f.save()

    # A truncated JPEG: the file must still play and the art must fail gracefully.
    front = make_image(lib, "front_600", 600, 600, "jpg")
    broken = lib.scratch("broken.jpg")
    broken.write_bytes(front.read_bytes()[: front.stat().st_size // 3])
    e = make_audio(lib, "artwork/mp3_truncated_jpeg", "mp3-cbr", pcm, 0.5, "artwork")
    t = id3.ID3()
    t.add(id3.APIC(encoding=3, mime="image/jpeg", type=3, desc="", data=broken.read_bytes()))
    t.save(lib.root / e.path)
    e.expect = "partial"
    e.notes = "Audio is fine; embedded JPEG is truncated to a third."

    # Folder images alongside an untagged-art album.
    for name, fmt, w in [("cover", "jpg", 800), ("folder", "png", 500), ("back", "jpg", 400)]:
        img = make_image(lib, f"folder_{name}", w, w, fmt, "smptehdbars")
        shutil.copyfile(img, lib.path(f"artwork/folder_art_album/{name}.{fmt}"))
    for i in range(2):
        e = make_audio(lib, f"artwork/folder_art_album/{i + 1:02d}", "flac", pcm, 0.5, "artwork")
        e.tags = tag_simple(lib.root / e.path, "flac", title=f"Folder Art {i + 1}", album="Folder Art Album",
                            artist="Fixture Artist", track=i + 1, total=2)
    lib.entries[-1].notes = "Folder has cover.jpg (800px), folder.png (500px), back.jpg; cover.jpg should win."


# --------------------------------------------------------------------------------------------
# Group: cue sheets


def group_cue(lib: Library) -> None:
    pcm = Pcm(44100, 16, S, 2)
    total = 44100 * 6
    src = lib.scratch("cue_src.wav")
    synth_wav(lib.tools, src, pcm, 6.0, GAPLESS_FREQS)
    _, data = wav_data(src)
    # Track starts in CD frames (1/75 s): 0:00:00, 0:02:00 (INDEX 00 at 0:01:60 = pregap), 0:04:37.
    starts = [(0, None), (150, 135), (337, None)]
    titles = ["Première", "二番目", "Third / Last"]
    lines = ['REM GENRE "Fixture"', 'REM DATE 2020', 'PERFORMER "Cue Artist"', 'TITLE "Cue Album — Live"',
             'FILE "album.flac" WAVE']
    for i, ((start, pregap), title) in enumerate(zip(starts, titles)):
        lines.append(f"  TRACK {i + 1:02d} AUDIO")
        lines.append(f'    TITLE "{title}"')
        lines.append('    PERFORMER "Cue Artist"')
        if pregap is not None:
            lines.append(f"    INDEX 00 {cd_time(pregap)}")
        lines.append(f"    INDEX 01 {cd_time(start)}")
    cue = "\r\n".join(lines) + "\r\n"
    tracks = [{"number": i + 1, "title": t, "start_frame": s * 588, "pregap_frame": (p * 588 if p is not None else None)}
              for i, ((s, p), t) in enumerate(zip(starts, titles))]

    e = make_audio(lib, "cue/sidecar_utf8/album", "flac", pcm, 0, "cue", src=src)
    lib.path("cue/sidecar_utf8/album.cue").write_bytes(b"\xef\xbb\xbf" + cue.encode("utf-8"))
    e.cue = {"sheet": "album.cue", "encoding": "utf-8-bom", "tracks": tracks, "total_frames": total}

    e = make_audio(lib, "cue/sidecar_cp1252/album", "flac", pcm, 0, "cue", src=src)
    cue_1252 = cue.replace("二番目", "Deuxième").replace("—", "-")
    lib.path("cue/sidecar_cp1252/album.cue").write_bytes(cue_1252.encode("cp1252"))
    t2 = [dict(t) for t in tracks]
    t2[1]["title"] = "Deuxième"
    e.cue = {"sheet": "album.cue", "encoding": "cp1252", "tracks": t2, "total_frames": total}

    e = make_audio(lib, "cue/embedded/album", "flac", pcm, 0, "cue", src=src)
    f = FLAC(lib.root / e.path)
    f["CUESHEET"] = [cue]
    f.save()
    e.cue = {"sheet": "embedded CUESHEET comment", "encoding": "utf-8", "tracks": tracks, "total_frames": total}
    assert len(data) == total * 4


def cd_time(frames75: int) -> str:
    return f"{frames75 // 75 // 60:02d}:{frames75 // 75 % 60:02d}:{frames75 % 75:02d}"


# --------------------------------------------------------------------------------------------
# Group: corrupt and hostile files


def group_corrupt(lib: Library) -> None:
    pcm = Pcm(44100, 16, S, 2)
    good_flac = make_audio(lib, "corrupt/_reference", "flac", pcm, 1.0, "corrupt")
    good_flac.notes = "Undamaged reference for the corrupt variants."
    ref = (lib.root / good_flac.path).read_bytes()
    mp3_ref_entry = make_audio(lib, "corrupt/_reference_mp3", "mp3-cbr", pcm, 1.0, "corrupt")
    mp3_ref = (lib.root / mp3_ref_entry.path).read_bytes()
    m4a_ref_entry = make_audio(lib, "corrupt/_reference_m4a", "aac", pcm, 1.0, "corrupt")
    m4a_ref = (lib.root / m4a_ref_entry.path).read_bytes()

    rng = random.Random(1234)
    cases = [
        ("truncated_half.flac", ref[: len(ref) // 2], "partial", "FLAC cut mid-stream."),
        ("bad_frame_crc.flac", flip(ref, len(ref) * 2 // 3, 16), "partial", "Bytes flipped in an audio frame."),
        ("zero_bytes.flac", b"", "error", "Empty file."),
        ("random_bytes.flac", bytes(rng.getrandbits(8) for _ in range(65536)), "error", "Noise with a .flac name."),
        ("header_only.flac", ref[:42], "error", "Only fLaC + STREAMINFO."),
        ("garbage_prefix.mp3", bytes(rng.getrandbits(8) for _ in range(5000)) + mp3_ref, "partial",
         "5000 junk bytes before the first MP3 frame."),
        ("id3_size_too_big.mp3", b"ID3\x04\x00\x00" + id3_to_syncsafe(10_000_000) + mp3_ref[10:], "error",
         "ID3 header claims a 10 MB tag in a small file."),
        ("broken_atom.m4a", broken_mp4(m4a_ref), "error", "moov atom size points past end of file."),
        ("text_file.mp3", b"This is not audio.\n" * 100, "error", "Plain text with an .mp3 name."),
        ("wrong_extension.mp3", ref, "ok", "A valid FLAC file named .mp3; content sniffing must win."),
    ]
    for name, blob, expect, notes in cases:
        rel = f"corrupt/{name}"
        lib.path(rel).write_bytes(blob)
        ext = name.rsplit(".", 1)[1]
        lib.add(Entry(id=rel.rsplit(".", 1)[0], path=rel, group="corrupt", container=ext,
                      codec="flac" if name == "wrong_extension.mp3" else "unknown",
                      lossless=name == "wrong_extension.mp3", channels=2, expect=expect, notes=notes,
                      pcm_md5=good_flac.pcm_md5 if name == "wrong_extension.mp3" else None,
                      frames=good_flac.frames if name == "wrong_extension.mp3" else None,
                      sample_rate=44100 if name == "wrong_extension.mp3" else None,
                      bits=16 if name == "wrong_extension.mp3" else None,
                      sample_format="s" if name == "wrong_extension.mp3" else None))
    readonly = make_audio(lib, "corrupt/read_only", "flac", pcm, 0.5, "corrupt")
    os.chmod(lib.root / readonly.path, 0o444)
    readonly.notes = "File mode 0444: tag writes must fail cleanly and leave it untouched."


def flip(blob: bytes, at: int, n: int) -> bytes:
    b = bytearray(blob)
    for i in range(at, at + n):
        b[i] ^= 0xFF
    return bytes(b)


def broken_mp4(blob: bytes) -> bytes:
    i = blob.find(b"moov")
    assert i > 4
    b = bytearray(blob)
    b[i - 4:i] = struct.pack(">I", len(blob) * 4)
    return bytes(b)


# --------------------------------------------------------------------------------------------
# Group: library (realistic structure for grouping, classical, compilations, editions)


def group_library(lib: Library) -> None:
    pcm = Pcm(44100, 16, S, 2)
    cover = make_image(lib, "front_600", 600, 600, "jpg")

    def album(folder: str, kind: str, album_key: str, tracks: list[dict], common: dict[str, list[str]],
              art: bool = True) -> None:
        for i, t in enumerate(tracks):
            fname = f"{t.get('disc', 1)}-{i + 1:02d} {t['title']}" if "disc" in t else f"{i + 1:02d} {t['title']}"
            e = make_audio(lib, f"library/{folder}/{fname}", kind, pcm, 0.5, "library")
            fields = dict(common)
            fields |= {"title": [t["title"]], "tracknumber": [str(t.get("n", i + 1))],
                       "tracktotal": [str(t.get("total", len(tracks)))]}
            fields |= {k: v for k, v in t.items() if k not in ("title", "n", "total", "disc")}
            write_fields(lib.root / e.path, kind, fields, cover if art else None)
            e.tags, e.album_key = fields, album_key

    gh = [{"title": "First Hit"}, {"title": "Second Hit"}, {"title": "Third Hit"}]
    album("Alpha Band/2001 - Greatest Hits", "flac", "alpha-greatest-hits", gh,
          {"artist": ["Alpha Band"], "albumartist": ["Alpha Band"], "album": ["Greatest Hits"], "date": ["2001"]})
    album("Beta Collective/1999 - Greatest Hits", "mp3-cbr", "beta-greatest-hits", gh[:2],
          {"artist": ["Beta Collective"], "albumartist": ["Beta Collective"], "album": ["Greatest Hits"],
           "date": ["1999"]})
    nd = [{"title": "Night Drive"}, {"title": "Neon"}]
    rg = ["3f1c0b6e-1111-4a8b-9c0d-1e2f3a4b5c6d"]
    album("Gamma/2010 - Night Drive", "flac", "gamma-night-drive-2010", nd,
          {"artist": ["Gamma"], "albumartist": ["Gamma"], "album": ["Night Drive"], "date": ["2010"],
           "musicbrainz_albumid": ["aaaaaaaa-0000-4000-8000-000000002010"], "musicbrainz_releasegroupid": rg})
    album("Gamma/2020 - Night Drive (Deluxe Remaster)", "flac", "gamma-night-drive-2020",
          nd + [{"title": "Neon (Demo)"}],
          {"artist": ["Gamma"], "albumartist": ["Gamma"], "album": ["Night Drive"], "date": ["2020"],
           "musicbrainz_albumid": ["aaaaaaaa-0000-4000-8000-000000002020"], "musicbrainz_releasegroupid": rg,
           "releasetype": ["album"], "comment": ["Deluxe remaster"]})
    album("Various Artists/2005 - Summer Mix", "mp3-cbr", "summer-mix",
          [{"title": "Sun", "artist": ["Delta"]}, {"title": "Sea", "artist": ["Epsilon", "Zeta"]},
           {"title": "Sand", "artist": ["Eta feat. Theta"]}],
          {"albumartist": ["Various Artists"], "album": ["Summer Mix"], "compilation": ["1"], "date": ["2005"]})
    movements = ["I. Allegro", "II. Adagio", "III. Scherzo", "IV. Finale"]
    album("Clara Composer/Symphony No. 1 (Orchestra Fixture)", "flac", "symphony-1",
          [{"title": m, "movementname": [m.split('. ', 1)[1]], "movement": [str(i + 1)], "movementtotal": ["4"],
            "discnumber": ["1" if i < 2 else "2"], "disctotal": ["2"], "disc": 1 if i < 2 else 2,
            "n": i % 2 + 1, "total": 2}
           for i, m in enumerate(movements)],
          {"artist": ["Fixture Orchestra", "Conductor Person"], "albumartist": ["Clara Composer"],
           "album": ["Symphony No. 1"], "composer": ["Clara Composer"], "conductor": ["Conductor Person"],
           "performer:orchestra": ["Fixture Orchestra"], "work": ["Symphony No. 1 in C"], "genre": ["Classical"],
           "date": ["2015"]})
    # Untagged files: titles must come from filenames.
    for name in ["track01", "Some Artist - Some Song"]:
        e = make_audio(lib, f"library/Unsorted/{name}", "wav", pcm, 0.5, "library")
        e.notes = "No tags at all."
    # Sidecar LRC lyrics.
    e = make_audio(lib, "library/Lyrics Artist/Lyric Song", "flac", pcm, 0.5, "library")
    e.tags = tag_simple(lib.root / e.path, "flac", title="Lyric Song", album="Lyric Album", artist="Lyrics Artist",
                        track=1, total=1)
    lib.path("library/Lyrics Artist/Lyric Song.lrc").write_text(
        "[ar:Lyrics Artist]\n[ti:Lyric Song]\n[00:00.10]First line\n[00:00.30]Second line\n", encoding="utf-8")
    e.notes = "Sidecar .lrc with two timed lines."


# --------------------------------------------------------------------------------------------
# Main


GROUPS = {
    "formats": group_formats,
    "multichannel": group_multichannel,
    "gapless": group_gapless,
    "tags": group_tags,
    "encodings": group_encodings,
    "artwork": group_artwork,
    "cue": group_cue,
    "corrupt": group_corrupt,
    "library": group_library,
}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--only", nargs="*", choices=sorted(GROUPS), help="generate only these groups")
    args = ap.parse_args()

    tools = Tools.discover()
    out: Path = args.out
    groups = args.only or list(GROUPS)
    if out.exists():
        # Read-only fixtures must be made writable again before they can be removed.
        for p in out.rglob("*"):
            if p.is_file():
                os.chmod(p, 0o644)
        for g in groups:
            shutil.rmtree(out / g, ignore_errors=True)
    out.mkdir(parents=True, exist_ok=True)
    lib = Library(out, tools)
    manifest_path = out / "manifest.json"
    previous = []
    if args.only and manifest_path.exists():
        previous = [e for e in json.loads(manifest_path.read_text())["entries"] if e["group"] not in groups]
    try:
        for g in groups:
            print(f"fixtures: {g}", flush=True)
            GROUPS[g](lib)
    finally:
        shutil.rmtree(lib.tmp, ignore_errors=True)
    manifest = {
        "generator_version": GENERATOR_VERSION,
        "pcm_layout": "interleaved little-endian; ints signed, ceil(bits/8) bytes; floats f32",
        "dsd_layout": "byte-interleaved per channel, MSB first",
        "tools": tools.versions(),
        "entries": previous + [e.to_json() for e in lib.entries],
    }
    manifest_path.write_text(json.dumps(manifest, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    total = sum(p.stat().st_size for p in out.rglob("*") if p.is_file())
    print(f"fixtures: {len(manifest['entries'])} entries, {total / 1e6:.1f} MB in {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
