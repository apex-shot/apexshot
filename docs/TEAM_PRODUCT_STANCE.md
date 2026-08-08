# Team product stance

Date: 2026-08-03  
Status: hypothesis under validation — not committed roadmap until gates pass.

---

## One-line stance

> **Hope is fine. Don’t bet the company on hope. Distribution first; talk to teams; build tenancy only after named pilots clear the gate.**

---

## Why team *might* work

- Individuals often won’t pay ~$5/month for storage when free/XBackBone exists.
- Teams *do* pay for coordination: shared library, admin, retention, “this is how we share screenshots at work.”
- Tenancy isn’t greenfield — schema already has organizations / members / `uploads.organization_id`.
- Price hypothesis **$10–20/seat/month** (test around **$15**) is normal small B2B SaaS, not fantasy.

---

## Why it might not

- Screenshot tools are often **personal muscle memory**, not something companies procure.
- “Team ShareX” is real but **niche** — needs named pilots, not vibes.
- Without desktop distribution (AUR, EGO, clean native releases), you never get enough users to *find* those teams.

---

## What “works” means (go / no-go)

Not “we built workspaces.”

**Go** only with evidence in the ballpark of:

| Signal | Threshold (from desktop/cloud plans) |
|---|---|
| Discovery calls | ~10 qualified |
| Pilot orgs committed | ~3 |
| Expected seats | ~15+ |
| Willingness to pay | at least some at ≥ **$10/seat/month** |

**Miss the gate → do not build the team product.** Keep interviewing. That’s discipline, not failure.

Working price to test in discovery: **~$15/seat/month** (not hard-coded before validation).

---

## Practical sequence

1. **Grow desktop installs first** — native release, AUR, EGO, honest download page.  
   Teams come from people who already live in the tool.
2. **Talk before code** — “Would your team pay to share captures in one place?” beats any UI.
3. **Cloud owns team** (leads, API, billing, web UI).  
   **Desktop only** adds a Personal/workspace selector after APIs + auth matrix exist.
4. If team flops, you still have a **solid Linux capture app** + distribution channels. That’s not zero.

---

## What desktop must not do yet

- No workspace selector until cloud workspace API + authorization matrix pass.
- No client-side “authorization” — server rejects bad scopes.
- No moving existing personal uploads into a workspace silently.
- No desktop billing, SSO admin, or custom-domain admin.

See `APEXSHOT_DESKTOP_IMPLEMENTATION_PLAN.md` §11 (desktop workspace support) and the sibling cloud plan for the full gate table.

---

## Blunt take

Team is a **reasonable bet**, not a sure thing. The smart part of the plan is the gate: **evidence first, build second.**

Hope → ship distribution → talk to ~10 teams → decide.  
If it doesn’t clear the bar, you didn’t waste a year on admin UI — you learned cheap.

---

## Related docs

- `docs/FLATPAK_STRATEGY.md` — native primary, Flatpak side channel  
- `docs/FLATPAK_PORTAL_ONLY_CHANGES.md` — what portal-only code does  
- `APEXSHOT_DESKTOP_IMPLEMENTATION_PLAN.md` — desktop half of distribution + teams plan  
- Sibling: `apexshot-cloud` team/cloud implementation plan (leads, billing, workspaces)
