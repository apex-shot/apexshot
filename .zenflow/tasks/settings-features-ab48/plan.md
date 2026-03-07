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
<!-- chat-id: d5bbf610-693e-41b5-9322-0d5fa5a92f54 -->

Updated the Rust General settings tab to show centered Startup, Sound, Shutter sound, and Menu bar controls, and wired save behavior to the existing configuration fields.
Refined the After capture table styling so the header divider reaches the table edges, the table background is removed, alternating faint row striping is applied, and the Recording checkbox column aligns with the main settings checkbox line while the table stays centered.
Tightened and restyled the custom toolbar traffic lights to look more like compact system controls by reducing their size and refining their colors and symbols.
Updated the settings window root to clip its contents to the same rounded edges used by the editor tool, and matched the top-level window transparency styling used by the editor so those rounded corners can actually show.
Refactored the settings styling into `src/settings/ui_support.rs` to mirror the editor structure, removed the invalid inline GTK CSS, and replaced the negative-margin layout with a scrollable content container and edge-to-edge separators.
Aligned the settings shell more closely with the editor by balancing the toolbar groups and moving the footer out of the scroll body so the rounded outer frame can read more like the editor window.
Matched the settings traffic-light widget and CSS exactly to the editor implementation instead of using a smaller variant.
Removed the forced vertical expansion from the settings body, reduced the bottom content margin, and lowered the default window height so the General tab no longer leaves excessive empty space below the After capture table.
Validated with `cargo fmt`, `cargo check`, `cargo build --release`, and `cargo test config::tests`.

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
