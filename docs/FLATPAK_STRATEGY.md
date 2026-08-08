# Flatpak strategy (decision record)

Date: 2026-08-03  
Status: **Active policy** — refer here before spending more time on Flatpak.

---

## One-line policy

> **Native = full ApexShot. Flatpak = sandboxed edition for the store. Distribution yes; product compromise no.**

---

## Decision

**Keep Flatpak as a side channel. Do not let it own the roadmap.**

Native stays primary. Flatpak stays portal-only and “good enough.”  
Do not chase KDE/wlr parity inside the sandbox.

---

## Why this is not “breaking the tool”

| | Native (deb / rpm / AUR) | Flatpak |
|---|---|---|
| Role | **Full product** | Extra storefront |
| KDE `ScreenShot2` | yes | no — uses Screenshot portal |
| wlroots `wlr-screencopy` | yes | no — uses Screenshot portal |
| `wf-recorder` | yes | no — ScreenCast portal + PipeWire |
| Qt `apexshot-capture` | yes | not built |
| Host hotkeys / extension install | yes | limited / link-out |
| Capture still works? | yes | **yes**, if desktop portals work |
| Feels like today’s power tool? | yes | **no** on KDE/wlroots edge cases |

Flatpak does **not** remove native methods from the project. It forces portals only in the
`--features flatpak` / sandboxed build. Native packages stay the real ApexShot for
power users.

On GNOME, portal-first is already the right path. On KDE/Hyprland/Sway, Flatpak is a
weaker edition: more permission UI, depends on portal quality (`xdg-desktop-portal`,
`xdg-desktop-portal-wlr`, etc.).

---

## What we already have (enough to pause)

Done under plan §§10.2–10.4 — see also `docs/FLATPAK_PORTAL_ONLY_CHANGES.md`:

- `flatpak` Cargo feature + `portal_only()` / `host_escape_blocked()`
- Qt helper skipped; host install/uninstall/native-host rejected
- Screenshot/ScreenCast portal forced; KDE/wlr direct paths skipped in portal-only
- Clipboard (arboard), notifications (D-Bus), open URL/file (GIO), Background portal autostart
- Telemetry default off; Tesseract optional (ocrs on Flatpak)
- Manifest skeleton: `flatpak/org.apexshot.ApexShot.yml` (app ID `org.apexshot.ApexShot`)

**Do not undo this.** It doesn’t hurt native and is required if Flathub happens later.

---

## Do next (priority order)

### 1. Native first (do this now / tomorrow)

Phase 0B — distribution without weakening the tool:

1. Harden and publish the **next native release** (AppStream in every package).
2. **AUR** live for the existing package.
3. **EGO** (GNOME extension) submit.
4. Truthful download-page facts for the website (artifacts, checksums, support matrix).

These grow installs without portal compromises.

### 2. Flatpak — park until native is solid

Only resume Flathub polish when:

- native tagged release is in good shape, **and**
- you can spare a focused block for real sandbox work.

Then, in order:

1. Install SDKs + `flatpak-builder`, run a real local build.
2. Generate `flatpak/cargo-sources.json` for offline/Flathub builds.
3. One test pass on GNOME (and skim KDE/wlroots portal behavior).
4. Submit; measure. Plan gate: ~250 Flathub installs / 30 days — if missed, **maintain listing, stop Flatpak-only enhancements**.

### 3. Explicitly do **not** do

- Host escapes / `flatpak-spawn --host` to fake native KDE/wlr inside Flatpak  
- Broad `--filesystem=host`  
- Full feature parity project for Flatpak  
- Renaming all native packages to `org.apexshot.ApexShot` until Flathub is actually submitting  
- Blocking normal product work on “but Flatpak…”  

---

## App ID

| Surface | ID |
|---|---|
| Flatpak / Flathub | `org.apexshot.ApexShot` (keep — same pattern as Flameshot’s `org.flameshot.Flameshot`) |
| Native packages today | `io.github.codegoddy.apexshot` (keep until one coordinated rename) |

Verification later: `apexshot.org/.well-known/org.flathub.VerifiedApps.txt`.

---

## Messaging (README / site)

Be honest:

- **Native** = full capture stack (KDE / wlroots / portals).  
- **Flatpak** = portal-only, sandbox-safe, easy install; not identical to native on every compositor.  
- Power users who want silent KDE grabs or raw Hyprland paths → **native package**.

Do not market Flatpak as “same as native on Hyprland.”

---

## When to revisit this doc

- After the next native release + AUR/EGO status update  
- Before any multi-day Flatpak/Flathub push  
- If portal capture is broken on a DE you personally use daily (spike that before polish)  
- After Flathub numbers hit (or miss) the 250-install gate  

---

## Related docs

- `docs/FLATPAK_PORTAL_ONLY_CHANGES.md` — what the code changes do  
- `flatpak/README.md` — how to build the Flatpak  
- `APEXSHOT_DESKTOP_IMPLEMENTATION_PLAN.md` — full plan (§10 Flatpak, §8 native quick wins)  
- `docs/FLATHUB_LAUNCH_PLAN.md` — earlier Flathub audit  

---

## Tomorrow’s default checklist

- [ ] Native release / packaging / AUR / EGO — not Flatpak polish  
- [ ] If touching Flatpak at all: only fix a blocker, don’t expand scope  
- [ ] Re-read the one-line policy before any “make Flatpak feel native” idea  
