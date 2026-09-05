#!/usr/bin/env python3
"""Translate missing PO catalog entries via DeepL's text-array API.

Reads DEEPL_AUTH_KEY from the environment. Never prints the key.
"""

from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
import argparse

ROOT = pathlib.Path(__file__).resolve().parents[1]
PO = ROOT / "po"
CHECKER = ROOT / "scripts" / "check-i18n-catalogs.py"

TARGET_LANGS = {
    "ar": "AR",
    "de": "DE",
    "es": "ES",
    "fr": "FR",
    "ja": "JA",
    "pt_BR": "PT-BR",
    "ru": "RU",
    "zh_CN": "ZH",
}

PLACEHOLDER_RE = re.compile(r"\{([A-Za-z0-9_]+)\}")
# DeepL may translate ordinary words inside `{placeholders}`. Use a compact,
# no-space identifier for retry requests; it is treated as a product token and
# survives translation unchanged.
TOKEN_RE = re.compile(r"APEXSHOT_TOKEN_(\d+)_END")


def load_checker():
    spec = importlib.util.spec_from_file_location("chk", CHECKER)
    chk = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(chk)
    return chk


def po_escape(text: str) -> str:
    return (
        text.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("\n", "\\n")
        .replace("\t", "\\t")
        .replace("\r", "\\r")
    )


def format_po_string(kind: str, text: str) -> str:
    escaped = po_escape(text)
    if "\\n" in escaped or len(escaped) > 72:
        parts = escaped.split("\\n")
        lines = [f'{kind} ""']
        for i, part in enumerate(parts):
            suffix = "\\n" if i < len(parts) - 1 else ""
            lines.append(f'"{part}{suffix}"')
        return "\n".join(lines)
    return f'{kind} "{escaped}"'


def format_entry(msgid: str, msgstr: str) -> str:
    return f"{format_po_string('msgid', msgid)}\n{format_po_string('msgstr', msgstr)}\n"


def protect(text: str) -> tuple[str, dict[str, str]]:
    """Replace placeholders with untranslatable tokens and retain exact spelling."""
    replacements: dict[str, str] = {}

    def replace(match: re.Match[str]) -> str:
        token = f"APEXSHOT_TOKEN_{len(replacements)}_END"
        replacements[token] = match.group(0)
        return token

    return PLACEHOLDER_RE.sub(replace, text), replacements


def unprotect(text: str, replacements: dict[str, str]) -> str:
    return TOKEN_RE.sub(lambda m: replacements.get(m.group(0), m.group(0)), text)


def placeholders(text: str) -> list[str]:
    return [f"{{{name}}}" for name in PLACEHOLDER_RE.findall(text)]


def restore_placeholder_names(source: str, text: str) -> str:
    """Keep DeepL's sentence but restore the app-owned placeholder identifiers."""
    source_tokens = placeholders(source)
    translated_tokens = placeholders(text)
    if not source_tokens or len(source_tokens) != len(translated_tokens):
        return text
    iterator = iter(source_tokens)
    return PLACEHOLDER_RE.sub(lambda _match: next(iterator), text)


def deepl_endpoint(auth_key: str) -> str:
    if auth_key.endswith(":fx") or "api-free.deepl.com" in os.environ.get("DEEPL_API_URL", ""):
        return os.environ.get("DEEPL_API_URL", "https://api-free.deepl.com/v2/translate")
    return os.environ.get("DEEPL_API_URL", "https://api.deepl.com/v2/translate")


def deepl_request(texts: list[str], target_lang: str, auth_key: str) -> list[str]:
    endpoint = deepl_endpoint(auth_key)
    params = [("source_lang", "EN"), ("target_lang", target_lang)]
    params.extend(("text", item) for item in texts)
    body = urllib.parse.urlencode(params).encode("utf-8")
    last_error = None
    payload = None
    for attempt in range(5):
        req = urllib.request.Request(
            endpoint,
            data=body,
            method="POST",
            headers={
                "Authorization": f"DeepL-Auth-Key {auth_key}",
                "Content-Type": "application/x-www-form-urlencoded",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                payload = json.loads(resp.read().decode("utf-8"))
            break
        except urllib.error.HTTPError as err:
            detail = err.read().decode("utf-8", errors="replace")
            last_error = f"DeepL HTTP {err.code} for {target_lang}: {detail[:300]}"
            if err.code in (429, 500, 502, 503, 504) and attempt < 4:
                time.sleep(2 ** attempt)
                continue
            raise SystemExit(last_error) from err
        except urllib.error.URLError as err:
            last_error = f"DeepL network error for {target_lang}: {err}"
            if attempt < 4:
                time.sleep(2 ** attempt)
                continue
            raise SystemExit(last_error) from err
    else:
        raise SystemExit(last_error or f"DeepL request failed for {target_lang}")

    translations = payload.get("translations") if payload else None
    if not isinstance(translations, list) or len(translations) != len(texts):
        raise SystemExit(
            f"DeepL returned {0 if not isinstance(translations, list) else len(translations)} "
            f"translations for {len(texts)} source messages ({target_lang})"
        )
    return [(item.get("text") or "") for item in translations]


def accept_translation(
    source: str, raw: str, replacements: dict[str, str] | None = None
) -> str | None:
    candidates = [restore_placeholder_names(source, raw.strip())]
    if replacements:
        candidates.insert(0, unprotect(raw.strip(), replacements))
    for text in candidates:
        if text and placeholders(source) == placeholders(text):
            text.encode("utf-8")
            return text
    return None


def normalize_translation(source: str, raw: str, target_lang: str, auth_key: str) -> str:
    accepted = accept_translation(source, raw)
    if accepted is not None:
        return accepted
    protected, replacements = protect(source)
    for payload, restore in ((source, None), (protected, replacements)):
        retried = deepl_request([payload], target_lang, auth_key)
        accepted = accept_translation(source, retried[0] if retried else "", restore)
        if accepted is not None:
            return accepted
    raise SystemExit(f"invalid DeepL translation for {source!r} ({target_lang}): {raw!r}")


def translate_batch(texts: list[str], target_lang: str, auth_key: str) -> list[str]:
    raw_values = deepl_request(texts, target_lang, auth_key)
    return [
        normalize_translation(source, raw, target_lang, auth_key)
        for source, raw in zip(texts, raw_values)
    ]


def translate_all(texts: list[str], target_lang: str, auth_key: str) -> list[str]:
    batch_size = 40
    chunks = [texts[start : start + batch_size] for start in range(0, len(texts), batch_size)]
    out: list[list[str] | None] = [None] * len(chunks)
    with ThreadPoolExecutor(max_workers=min(8, len(chunks))) as executor:
        futures = {
            executor.submit(translate_batch, chunk, target_lang, auth_key): index
            for index, chunk in enumerate(chunks)
        }
        for future in as_completed(futures):
            out[futures[future]] = future.result()
    flattened = [translation for batch in out for translation in batch or []]
    if len(flattened) != len(texts):
        raise SystemExit(f"translated count {len(flattened)} != source count {len(texts)}")
    return flattened


def append_entries(path: pathlib.Path, entries: list[tuple[str, str]]) -> None:
    existing = path.read_text(encoding="utf-8")
    if not existing.endswith("\n"):
        existing += "\n"
    blocks = ["\n".join(format_entry(msgid, msgstr) for msgid, msgstr in entries)]
    path.write_text(existing + "\n" + blocks[0], encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--language",
        choices=tuple(TARGET_LANGS),
        help="Translate one catalog only (useful where command runtimes are bounded).",
    )
    args = parser.parse_args()
    auth_key = os.environ.get("DEEPL_AUTH_KEY", "").strip()
    if not auth_key:
        print("DEEPL_AUTH_KEY is not set", file=sys.stderr)
        return 2

    chk = load_checker()
    registered = chk.registered_messages()
    target_langs = (
        {args.language: TARGET_LANGS[args.language]}
        if args.language
        else TARGET_LANGS
    )
    missing_by_lang: dict[str, list[str]] = {}
    for lang in target_langs:
        catalog = chk.parse_po(PO / f"{lang}.po")
        missing_by_lang[lang] = [
            msgid
            for msgid in registered
            if msgid not in catalog or not catalog[msgid].strip()
        ]
    unique_missing = sorted({msg for ids in missing_by_lang.values() for msg in ids})
    print(f"translating {len(unique_missing)} missing messages")
    for lang, ids in missing_by_lang.items():
        print(f"  {lang}: {len(ids)}")

    if not unique_missing:
        return 0

    translations: dict[str, dict[str, str]] = {}
    # Every target language is independent. Parallel requests keep this
    # all-or-nothing job within the CI/agent execution window while each
    # language still sends bounded batches to DeepL.
    with ThreadPoolExecutor(max_workers=len(TARGET_LANGS)) as executor:
        futures = {
            executor.submit(translate_all, unique_missing, target, auth_key): lang
            for lang, target in target_langs.items()
        }
        for future in as_completed(futures):
            lang = futures[future]
            values = future.result()
            translations[lang] = dict(zip(unique_missing, values))
            print(f"DeepL {lang} complete")

    for lang, ids in missing_by_lang.items():
        entries = [(msgid, translations[lang][msgid]) for msgid in ids]
        append_entries(PO / f"{lang}.po", entries)
        chk.parse_po(PO / f"{lang}.po")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
