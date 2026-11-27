# Smarter Copy text

Capturing with OCR copies the recognised text. This makes the copy keep its
layout, without ever making the copy wait or changing what the text says.

## Flow

1. The capture shell copies the raw text instantly, as before.
2. In the background it sends the OCR lines and word boxes to the organizer
   helper (`"task": "clean_text"`).
3. When the result lands, the shell replaces the clipboard only if it still
   holds the raw text, and the result card offers "Text cleaned up · Undo".

## Layer 1: layout (free, rules)

From word boxes: tables (aligned columns) become tab-separated text plus an
HTML table; code keeps its indentation; paragraphs rejoin lines and
hyphenated words; lists keep one item per line. Instant and lossless.

## Not shipped: AI corrections

A Studio layer asked the local 2B model to fix OCR reading mistakes, behind a
fidelity check. On the test set (article, steps, Python, YAML, spreadsheet,
dashboard, post, settings) it took 2.6-9 s and returned the text unchanged
every time: Windows OCR is already accurate, and its remaining mistakes ("Al"
for "AI", "QI" for "Q1") are ones a small, cautious model cannot tell from
real text. Layout carries the value, so the feature is layout only and free.

## Settings

- Advanced, OCR: "Keep layout (tables, code, paragraphs)", on by default.

## Checking it

A set of real captures (spreadsheet, invoice, Python/Rust/YAML, chat,
article, numbered list, mixed page) compared raw vs layout vs AI for
fidelity, paste format, and time before the app is tested by hand.
