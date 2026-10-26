#!/usr/bin/env python3
"""Fail if a registered UI message is missing from any shipped PO catalog.

Both UIs are checked: the Linux app in `src/` and the Windows app in
`windows-ui/src/`, which reuses the same catalogs even though the Windows
package does not yet ship them.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = [ROOT / "src", ROOT / "windows-ui" / "src"]
PO = ROOT / "po"
SHIPPED = ["ar", "de", "es", "fr", "ja", "pt_BR", "ru", "zh_CN"]
SKIP = {"", "+", "-", "⌄"}
CALL_RE = re.compile(
    r'(?:^|[^A-Za-z0-9_])(?:t|tfmt|markup_title|markup_bold)\(\s*"((?:\\.|[^"\\])*)"',
)
FUNCTION_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")
CALL_HEAD_RE = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(")
TRANSLATORS = {"t", "tfmt", "markup_title", "markup_bold"}


def matching(text: str, start: int, left: str, right: str) -> int | None:
    depth = 0
    quote = None
    escaped = False
    for index in range(start, len(text)):
        char = text[index]
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            continue
        if char in ('"', "'"):
            # Rust lifetimes are not quoted strings.
            if char == "'" and index + 1 < len(text) and (text[index + 1].isalpha() or text[index + 1] == "_"):
                continue
            quote = char
        elif char == left:
            depth += 1
        elif char == right:
            depth -= 1
            if depth == 0:
                return index
    return None


def split_arguments(text: str) -> list[str]:
    args: list[str] = []
    start = 0
    stack: list[str] = []
    quote = None
    escaped = False
    pairs = {")": "(", "]": "[", "}": "{"}
    for index, char in enumerate(text):
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            continue
        if char == '"':
            quote = char
        elif char in "([{":
            stack.append(char)
        elif char in ")]}":
            if stack and stack[-1] == pairs[char]:
                stack.pop()
        elif char == "," and not stack:
            args.append(text[start:index].strip())
            start = index + 1
    if text[start:].strip():
        args.append(text[start:].strip())
    return args


def function_definitions(text: str) -> list[tuple[str, list[str], str]]:
    definitions = []
    for match in FUNCTION_RE.finditer(text):
        open_paren = text.find("(", match.start())
        close_paren = matching(text, open_paren, "(", ")")
        if close_paren is None:
            continue
        params = []
        for param in split_arguments(text[open_paren + 1 : close_paren]):
            param_match = re.search(r"(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:", param)
            if param_match:
                params.append(param_match.group(1))
        body_start = text.find("{", close_paren + 1)
        if body_start < 0:
            continue
        body_end = matching(text, body_start, "{", "}")
        if body_end is not None:
            definitions.append((match.group(1), params, text[body_start + 1 : body_end]))
    return definitions


def calls(text: str):
    for match in CALL_HEAD_RE.finditer(text):
        name = match.group(1)
        open_paren = text.find("(", match.start())
        close_paren = matching(text, open_paren, "(", ")")
        if close_paren is not None:
            yield name, split_arguments(text[open_paren + 1 : close_paren])


def translated_parameter_positions(
    definitions: list[tuple[str, list[str], str]],
) -> dict[str, set[int]]:
    translated = {name: set() for name, _, _ in definitions}
    by_name = {name: (params, body) for name, params, body in definitions}
    changed = True
    while changed:
        changed = False
        for name, params, body in definitions:
            positions = translated[name]
            param_indexes = {param: index for index, param in enumerate(params)}
            for callee, args in calls(body):
                if callee in TRANSLATORS and args:
                    arg = args[0].strip()
                    if arg in param_indexes and param_indexes[arg] not in positions:
                        positions.add(param_indexes[arg])
                        changed = True
                elif callee in translated:
                    target_params = by_name[callee][0]
                    for index in translated[callee]:
                        if index < len(args):
                            arg = args[index].strip()
                            if arg in param_indexes and param_indexes[arg] not in positions:
                                positions.add(param_indexes[arg])
                                changed = True
    return translated


def helper_messages(text: str, translated: dict[str, set[int]]) -> set[str]:
    found: set[str] = set()
    for name, args in calls(text):
        for index in translated.get(name, set()):
            if index < len(args):
                literal = re.fullmatch(r'"((?:\\.|[^"\\])*)"', args[index].strip(), re.S)
                if literal:
                    found.add(unescape_rust(literal.group(1)))
    return found


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
    sources = [
        path.read_text(encoding="utf-8") for root in SRC for path in root.rglob("*.rs")
    ]
    definitions = [definition for text in sources for definition in function_definitions(text)]
    translated = translated_parameter_positions(definitions)
    for text in sources:
        for match in CALL_RE.finditer(text):
            msgid = unescape_rust(match.group(1))
            if msgid not in SKIP:
                found.add(msgid)
        found.update(msg for msg in helper_messages(text, translated) if msg not in SKIP)
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
        "Circle",
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


def self_test() -> None:
    definitions = '''
fn translated(title: &str) { let slider = Slider::new(&t(title), move |value| format!("{value}")); }
fn wrapper(label: &str) { translated(label); }
fn untranslated(label: &str) { println!("{label}"); }
'''
    callsites = '''
fn build() {
    wrapper("Found through two helpers");
    untranslated("Not translated");
}
'''
    translated = translated_parameter_positions(function_definitions(definitions))
    assert helper_messages(callsites, translated) == {"Found through two helpers"}


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
    self_test()
    if "--self-test" in sys.argv[1:]:
        print("ok: helper parameter discovery self-test")
        return 0
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
