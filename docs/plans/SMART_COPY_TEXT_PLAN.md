# Smarter Copy text

Capturing with OCR copies the recognised text. This makes the copy keep its
layout and, with Studio, fixes the reader's mistakes, without ever making the
copy wait or changing what the text says.

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

## Layer 2: AI corrections (Studio)

The same local model fixes OCR character mistakes in prose and lists of up to
150 words. Never for tables or code. Every reply passes a fidelity check (edit
distance within 8% of the original, identical digits, links, and email
addresses) or is discarded. Uses the AI names gate: supported PC, model
installed, Studio or trial uses left.

## Settings

- Advanced, OCR: "Keep layout (tables, code, paragraphs)", on by default.
- Studio: "Fix reading mistakes with AI", and an entry in What's included.

## Checking it

A set of real captures (spreadsheet, invoice, Python/Rust/YAML, chat,
article, numbered list, mixed page) compared raw vs layout vs AI for
fidelity, paste format, and time before the app is tested by hand.
