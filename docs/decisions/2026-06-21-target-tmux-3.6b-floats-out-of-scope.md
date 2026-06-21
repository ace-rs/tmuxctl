# Target stable tmux 3.6b; native floating panes deferred

**Date:** 2026-06-21
**Status:** Accepted (chakrit)
**Resolves:** the open "which tmux to pin" question in
[2026-06-18-lock-step-tmux-and-robustness.md](2026-06-18-lock-step-tmux-and-robustness.md).

## Context

The lock-step ADR fixed the *policy* — one pinned tmux, produce strictly / accept liberally,
no version matrix — but left the *value* open. In practice the repo had drifted to `next-3.7`
at `3.7-rc-86-gc6b8ad6e`: not a release, and 86 commits past even the 3.7 release candidate,
i.e. unfrozen `main`. Meanwhile the transcript fixtures and the integration test were already
captured against the latest stable, **3.6b**. So the port reference (the source map) and the
regression net disagreed on the target.

The `next-3.7` pull was floating panes — a 3.7 feature (`LAYOUT_CELL_FLOATING`, the `<…>`
layout section). hangar wants a floating-pane *effect*. But: (1) 3.7 is unreleased, so no user
runs it; (2) per the 3.7 notes, native floats land with substantial limitations hangar would
have to work around regardless; (3) the float *effect* does not require native tmux floats — it
can be composited client-side (a real pane in a not-switched-to window, drained continuously,
blitted as an overlay), which works on any tmux including 3.6b.

The wire delta 3.6b → 3.7-rc is tiny and verified: the control-mode **notification set is
identical** (zero added, zero removed across `control.c`/`control-notify.c`), and the only
wire-visible change is the `<…>` floating-pane layout section — the rest is non-wire `control.c`
refactoring. So `%subscription-changed`, `%extended-output`, `%pause`/`%continue`, and the whole
flow-control / subscription machinery are **already in 3.6b** and fully in scope. "Accept
liberally" still folds anything a *later* tmux adds into `Notification::Unknown`, so the pin
costs nothing forward.

## Decisions

**Pin tmux 3.6b.** `TARGET_TMUX` = the `3.6b` release tag,
`8f3f14f565d3dc2a1e7f8c37e0dc3d3499c70c97`. A *release* tag is immutable (unlike the `next-3.7`
branch alias), so it satisfies the lock-step ADR's "pin a stable pointer, not a moving ref"
intent; the SHA is recorded for precision. This resolves the lock-step open question and fixes
the rc+86 drift. The source map, spec, and CLAUDE.md are re-anchored to 3.6b.

**Native floating panes are deferred — for now, not forever.** 3.6b has no floats, so the
tiled-only `Layout` (no `<…>` parse/render) is *correct for the current target*, not a permanent
stance. Parsing/rendering the `<…>` layout section **is** protocol-layer work, and it will land in
a future tmuxctl as the pinned target bumps to a 3.7+ where floats have matured — tmuxctl upgrades
and target bumps move together. We hold off now because the target (3.6b) has no floats and 3.7's
floats are still alpha, not because floats don't belong in this crate.

Keep one distinction the first draft of this ADR blurred: the floating *overlay effect*
(compositing an overlay on screen) is the consumer's (hangar's) rendering job regardless — even
once tmuxctl parses native floats, tmux reports the geometry and hangar still draws. So while the
target lacks native floats, hangar gets the effect via client-side compositing; native support
later just feeds it the geometry rather than replacing the compositor.

## Consequences

- The "3.7 floating-pane `<…>` gap" in roadmap Phase 0 is reclassified from *tracked gap* to
  *deferred — not applicable to the current 3.6b target*. It becomes live protocol work when the
  target bumps to a 3.7+ with mature floats.
- hangar achieves floats via client-side compositing ("approach 4"): host the float's program
  in a `new-window -d` window sized to the float rect (`window-size manual` + `resize-window`),
  receive its `%output` like any pane, route input with `send-keys -t %pane`, composite as an
  overlay. tmux runs every pane continuously regardless of focus — **nothing pauses** — so
  hangar must keep draining output for the backgrounded panes or the single control pipe
  backpressures and stalls the whole session. Optional opt-in pause via control-mode flow
  control (`pause-after` / `%p:continue`, a roadmap Phase 3 item), at the cost of the paused
  program blocking on a full buffer.
- Known limitation of the composite: the host window is a *real* window — visible to other
  clients and `list-windows`; 3.6b has no truly-hidden window. Name/flag it; treat as a known
  artifact.
- The source map's line numbers were captured against `next-3.7`; they need re-anchoring to
  3.6b (tracked as a roadmap follow-up). The checksum loop and the core notification formats are
  identical across 3.6b ↔ 3.7, so the *algorithms* in the map already hold; only line numbers
  and the float section drift.
- Bump path to 3.7 stays exactly the lock-step runbook: bump the ref, regenerate fixtures, the
  `smoke` CHANGED diff is the wire changelog, add the `<…>` float layout section (the **only**
  wire-visible 3.7 delta — the notification set is unchanged), release.
