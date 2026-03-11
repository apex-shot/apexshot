# Auto

## Configuration
- **Artifacts Path**: {@artifacts_path} → `.zenflow/tasks/{task_id}`

---

## Agent Instructions

Ask the user questions when anything is unclear or needs their input. This includes:
- Ambiguous or incomplete requirements
- Technical decisions that affect architecture or user experience
- Trade-offs that require business context

Do not make assumptions on important decisions — get clarification first.

---

## Workflow Steps

### [x] Step: Implementation
<!-- chat-id: 4e6b2abf-71fe-407c-874f-e6d9daee3b5f -->

Adjust the backend Rust editor toolbar so the crop tool appears first and is visually isolated in its own container. Keep the remaining tools grouped separately and preserve the active crop state and shortcut/index behavior. Latest update: keep the toolbar layout stable while crop mode is active so crop dropdown interactions do not toggle the crop tool off, remove the extra off-state gap by swapping between standard and crop toolbar modes through a shared stack container, anchor the crop-mode controls beside the crop button instead of centering them in the toolbar, make the crop-type dropdown functional by applying aspect-ratio presets to crop initialization, drawing, and resizing, with preset selection now visibly snapping the crop box to a centered preset-sized frame and redrawing the canvas immediately on dropdown selection, add live width/height inputs beside the preset dropdown to show the current crop dimensions in pixels, lay the groundwork for add-background by letting the crop frame move and resize beyond the image bounds while keeping crop overlay rendering and crop handle hit-testing active outside the image, expand the editor canvas sizing/transform offsets dynamically from crop overflow so upward and downward crop extensions remain visible instead of being clipped, reserve a fixed bounded crop-mode overflow margin instead of a viewport-scaled one so vertical resizing stays stable without crashing the editor, tint everything outside the crop frame with the same `QColor(0, 0, 0, 140)` feel used by the C++ capture-area overlay while leaving the entire crop region clear even when it extends beyond the image, wire the shared editor color picker into crop mode so the extended crop area previews and exports with the selected background color while the picker state swaps cleanly between crop background color and normal annotation color, and keep the color picker in its original standard-toolbar placement while still showing it directly beside the crop size controls when crop mode is active, avoid showing a misleading preselected crop color until the user explicitly picks one, and tint the full crop-mode checkerboard background with the chosen crop background color so users can immediately see the added background before extending the crop, while keeping the initial crop-mode background on the default checkerboard until a color is explicitly picked from the color picker, give the crop preset selector the same dark boxed tool-group treatment as the rest of the toolbar controls so it no longer looks like a floating standalone input, and increase the crop edge drag dashes so the resize handles read more clearly.

**Debug requests, questions, and investigations:** answer or investigate first. Do not create a plan upfront — the user needs an answer, not a plan. A plan may become relevant later once the investigation reveals what needs to change.

**For all other tasks**, before writing any code, assess the scope of the actual change (not the prompt length — a one-sentence prompt can describe a large feature). Scale your approach:

- **Trivial** (typo, config tweak, single obvious change): implement directly, no plan needed.
- **Small** (a few files, clear what to do): write 2–3 sentences in `plan.md` describing what and why, then implement. No substeps.
- **Medium** (multiple components, design decisions, edge cases): write a plan in `plan.md` with requirements, affected files, key decisions, verification. Break into 3–5 steps.
- **Large** (new feature, cross-cutting, unclear scope): gather requirements and write a technical spec first (`requirements.md`, `spec.md` in `{@artifacts_path}/`). Then write `plan.md` with concrete steps referencing the spec.

**Skip planning and implement directly when** the task is trivial, or the user explicitly asks to "just do it" / gives a clear direct instruction.

To reflect the actual purpose of the first step, you can rename it to something more relevant (e.g., Planning, Investigation). Do NOT remove meta information like comments for any step.

Rule of thumb for step size: each step = a coherent unit of work (component, endpoint, test suite). Not too granular (single function), not too broad (entire feature). Unit tests are part of each step, not separate.

Update `{@artifacts_path}/plan.md`.
