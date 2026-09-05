#!/usr/bin/env python3
"""Fail if a registered UI message is missing from any shipped PO catalog."""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
PO = ROOT / "po"
SHIPPED = ["ar", "de", "es", "fr", "ja", "pt_BR", "ru", "zh_CN"]
SKIP = {"", "+", "-", "⌄"}
CALL_RE = re.compile(
    r'(?:^|[^A-Za-z0-9_])(?:t|tfmt|markup_title|markup_bold)\(\s*"((?:\\.|[^"\\])*)"',
)


def unescape_rust(value: str) -> str:
    out: list[str] = []
    i = 0
    while i < len(value):
        if value[i] == "\\" and i + 1 < len(value):
            mapping = {"n": "\n", "t": "\t", "r": "\r", '"': '"', "\\": "\\", "'": "'"}
            nxt = value[i + 1]
            out.append(mapping.get(nxt, "\\" + nxt))
            i += 2
            continue
        out.append(value[i])
        i += 1
    return "".join(out)


def registered_messages() -> list[str]:
    found: set[str] = set()
    for path in SRC.rglob("*.rs"):
        text = path.read_text(encoding="utf-8")
        for match in CALL_RE.finditer(text):
            msgid = unescape_rust(match.group(1))
            if msgid not in SKIP:
                found.add(msgid)
    extra = [
        "Area",
        "Fullscreen",
        "Scroll",
        "Timer",
        "OCR",
        "Recording",
        "Freeform",
        "Mic",
        "Speaker",
        "1 : 1 (Square)",
        "5 : 4 (10 : 8)",
        "4 : 3",
        "7 : 5",
        "3 : 2",
        "16 : 10",
        "16 : 9",
        "2.35 : 1",
        "2 : 3",
        "9 : 16",
        "Thin",
        "Medium",
        "Thick",
        "Very Thick",
        "Small",
        "Large",
        "Extra Large",
        "Pixelate",
        "Blur",
        "Blackout",
        "Standard",
        "Fancy",
        "Curved",
        "Double",
        "Square",
        "Adwaita",
        "Yaru",
        "White",
        "Black",
        "macOS",
        "Tahoe",
        "Tahoe Inverted",
        "Dot",
        "Minimal",
        "Spotlight",
        "Ripple",
        "Echo",
        "Glide",
        "Smooth",
        "Snappy",
        "Linear",
        "Focused",
        "1, 2, 3, 4...",
        "A, B, C, D...",
        "a, b, c, d...",
        "i, ii, iii, iv...",
        "Video recording is not supported on Fedora. Screenshots still work. For screen recording, use Spectacle or Kooha.",
    ]
    found.update(extra)
    return sorted(found)


def parse_po(path: pathlib.Path) -> dict[str, str]:
    messages: dict[str, str] = {}
    msgid = ""
    msgstr = ""
    state = None

    def parse_quoted(line: str) -> str:
        inner = line.strip()[1:-1]
        out: list[str] = []
        i = 0
        while i < len(inner):
            if inner[i] == "\\" and i + 1 < len(inner):
                mapping = {"n": "\n", "t": "\t", "r": "\r", "\\": "\\", '"': '"'}
                out.append(mapping.get(inner[i + 1], "\\" + inner[i + 1]))
                i += 2
                continue
            out.append(inner[i])
            i += 1
        return "".join(out)

    def finish() -> None:
        nonlocal msgid, msgstr, state
        if msgid and msgstr:
            messages[msgid] = msgstr
        msgid = ""
        msgstr = ""
        state = None

    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            finish()
            continue
        if line.startswith("#"):
            continue
        if line.startswith("msgid "):
            finish()
            msgid = parse_quoted(line[6:])
            state = "id"
        elif line.startswith("msgstr "):
            msgstr = parse_quoted(line[7:])
            state = "str"
        elif line.startswith('"'):
            chunk = parse_quoted(line)
            if state == "id":
                msgid += chunk
            elif state == "str":
                msgstr += chunk
        else:
            raise SystemExit(f"{path}: invalid PO syntax: {line}")
    finish()
    return messages


def placeholders(text: str) -> list[str]:
    return re.findall(r"\{[A-Za-z0-9_]+\}", text)


def main() -> int:
    registered = registered_messages()
    failures: list[str] = []
    for lang in SHIPPED:
        catalog = parse_po(PO / f"{lang}.po")
        for msgid in registered:
            msgstr = catalog.get(msgid)
            if msgstr is None:
                failures.append(f"{lang}: missing {msgid!r}")
            elif not msgstr.strip():
                failures.append(f"{lang}: empty msgstr for {msgid!r}")
            elif placeholders(msgid) != placeholders(msgstr):
                failures.append(f"{lang}: placeholder mismatch for {msgid!r} -> {msgstr!r}")
    if failures:
        print(f"catalog coverage failed ({len(failures)}):")
        print("\n".join(failures))
        return 1
    print(f"ok: {len(registered)} messages present in {', '.join(SHIPPED)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
