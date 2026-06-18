# Slice Loop & Audit Cadence

The standing autonomous workflow for hangar-driven and "keep going" runs. Self-contained:
the audit passes are procedures written here — do **not** invoke the `/ace-audit` skill;
follow this guide. Scale the mechanism to the work: small slices run inline in fresh
context; spawn a subagent per slice when the work is large or context is filling, and for
the audit passes. Adapted from `chakrit/kue`'s slice-loop, tuned for Rust and the tmux
protocol.

## Design philosophy — protocol fidelity + type-system first (the audits enforce this)

tmuxctl is the wire between Rust and a tmux server. Two invariants the audits check first:

1. **Protocol fidelity.** The oracle is the tmux source
   ([`../reference/tmux-source-map.md`](../reference/tmux-source-map.md); clone at
   `~/Documents/chakrit/tmux`) and real `tmux -C` captures — never memory or guesswork.
   Strict-produce for the pinned tmux; liberally accept the stream (unknown `%`-lines →
   `Notification::Unknown`, never a panic). Where the impl corrects the spec or tmux
   surprises, record it in `../reference/tmux-divergences.md`.
2. **Make illegal states unrepresentable.** Sum types over stringly-typed flags; newtypes
   for ids (`PaneId`/`WindowId`/`SessionId`); total functions over partial; `Result`/`Option`
   over hidden failure; exhaustive matches with no catch-all `_` that silently swallows a new
   variant. The sans-IO core stays pure and runtime-free; runtime lives only in the
   feature-gated drivers. `general-coding`/`rust-coding` are hard blockers, not nits.

## Cadence (repeat)

1. Run **2–3 implementation slices** (one commit each).
2. **Phase A — code-quality audit** (the batch since the last audit).
3. **Phase B — architecture / refactor audit** (the whole crate).
4. Fold all findings into [`../roadmap.md`](../roadmap.md) as ranked fix-slices. Fix-slices
   count as implementation slices next round.
5. Go to 1.

Audits are mandatory at the 2–3-slice mark, run **A then B sequentially** (both edit the
roadmap; parallel would collide). Don't let them stall forward motion; don't skip them.

Periodic, not every cycle:

- **Transcript/fixture organization** — when `tests/fixtures/` grows unwieldy, spend a slice
  regrouping and deduping. Phase B flags when due.
- **Roadmap hygiene** — distill accumulated audit findings back to the live roadmap; history
  lives in git and the breadcrumb.
- **Retrospective** — every few audit cycles, review what broke operationally (lost work,
  transient API errors, peer-instruction mishaps) and record each with its guard in
  `../reference/failure-modes.md`.

## Slice (per unit)

Full workflow in fresh context: plan → tests-first → implement → verify → commit → update
docs. Verify gate: `cargo test` + `cargo clippy --all-targets --all-features` +
`cargo fmt --check` (all clean); add transcript replay and the live-tmux integration script
as those land. Commit on the current branch. Standing duties:

- **Tests first-class.** Pin edges, not just the happy path — octal-escape cases (`\134`,
  DEL/high-byte passthrough, UTF-8 split across reads), sparse/non-contiguous command
  numbers, notifications interleaved around reply blocks, layout round-trips. Prefer
  fixtures captured from real `tmux -C` over hand-written wire bytes.
- **Keep docs current as a restore point.** Update [`../roadmap.md`](../roadmap.md), the
  relevant decision/note, and the session-resume breadcrumb as the slice lands — not batched
  at the end. A slice is not done until its doc trail is written.
- **Commit at checkpoints, not only at the end.** A crash or transient API error loses all
  uncommitted work; commit at natural internal seams on a long slice.

## Phase A — code-quality audit (the batch since the last audit)

- **Protocol fidelity** — behavior matches the source map / a real capture; the wire format
  is right, not merely plausible. Unknown lines tolerated, not misparsed.
- **Illegal-states-unrepresentable (check FIRST)** — tightest type that fits; flag loose
  `String`/`u*`/`bool`/`Option` that should be a sum type, a newtype, or that should carry
  distinct semantics; flag any record admitting a nonsense combination; flag catch-all `_`
  arms that could swallow a new `Notification` variant the code should handle.
- **DRY / reuse** — no duplicated logic that wants a named helper.
- **Test strength** — edges pinned, not smoke; new wire-format coverage prefers real
  captures.
- **Skill compliance** — `general-coding`/`rust-coding` (hard blockers); naming/readability.
- **Doc accuracy** — roadmap / spec / decisions match the code.

Output: fold findings into [`../roadmap.md`](../roadmap.md) as fix-slices. Apply only
low-risk fixes inline; if you do, re-run the full verify gate and commit.

## Phase B — architecture / refactor / cleanup audit (the whole crate)

- **Type-system leverage** — across modules, loose types carrying invariants the type system
  could enforce; wildcard matches hiding cases; partial functions reducible to total.
- **Sans-IO boundary** — the core (`Parser`, correlation, `Layout`, `decode_output`) stays
  pure and runtime-free; runtime and I/O live only in feature-gated drivers; no driver
  concern leaks into the core; no cross-module DRY violation.
- **Dependency / compile-time budget** — minimal deps is a stated goal; flag dep creep or a
  heavy crate where a hand-rolled piece fits. Report `cargo build` time regressions.
- **Refactor / cleanup** — dead code, duplication across modules, misplaced functions,
  over-engineering.
- **Test/fixture health** — coverage gaps at the seams; oversized test modules; fixture
  debt.

Output: fold into [`../roadmap.md`](../roadmap.md) as ranked architecture fix-slices; large
refactors become their own slices. Apply only low-risk cleanups inline (re-verify + commit).

## Blockers — don't stall, surface

When work needs a human — ambiguous spec, a judgment call you can't safely default, or a
safety-envelope boundary (push/publish/dep-install/global mutation; see `CLAUDE.md`) — don't
cross it and don't wait on it. Surface it to the driver (hangar) over `ace-connect`
(`STUCK`/`ASK`), and for unattended stretches append a blocker to `.afk.log`: **what**
stopped, **why** it needs a human, **what you'd do** (so a one-word reply unblocks it). Then
pick up the next unblocked work.

## Releases (local only — CI/GitHub Actions BANNED)

Pre-1.0, milestone-based (not on a clock), cut from current `main` and released in step with
hangar's needs and the pinned tmux version. Mechanism: a local `scripts/release.sh` —
`cargo build` + checksum + `gh release create` + `cargo publish`. No CI, ever. Requires a
clean tree and the user's go-ahead — publish and push are outside the autonomous envelope.
