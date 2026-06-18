# Lock-step tmux versioning + robustness (no version matrix)

**Date:** 2026-06-18
**Status:** Accepted (chakrit)

## Context

`tmuxctl` parses tmux's **control-mode wire format** — `%`-notifications, the `%begin/%end`
reply framing, the layout-string grammar + checksum, the octal output escaping, the
command-number semantics. None of these are a stable, documented API: they are tmux
*internals*, free to change at any release, with no compatibility promise.

Trying to support "every tmux version, past and future" is therefore unwinnable. A version
matrix multiplies maintenance N-fold for *false* confidence — you still cannot test future
versions, and you carry per-version branches almost nobody exercises. We depend on a moving
target; pretending there is a stable contract is the mistake.

## Decisions

**Lock-step to one pinned tmux version.** tmuxctl targets exactly one tmux, pinned by commit
SHA (a tag/branch moves). The crate version encodes which tmux it was validated against
(`tmuxctl x.y` ↔ tmux `<sha>`); a tmux bump that shifts the wire format is a tmuxctl release.
One target, one fixture set — never a matrix.

**Robustness principle (a disciplined Postel's law), with the verdict delegated to tmux:**

- **Produce strictly** — every command we *emit* (`send-keys`, `refresh-client`, regenerated
  layout strings) targets the one pinned tmux. We do not try to emit portable-across-versions
  output; that would be a write-side matrix.
- **Accept liberally** — the notification stream we *parse* tolerates drift: known lines
  parse strictly, unknown or evolved lines fall to `Notification::Unknown` and are logged,
  never fatal; extra trailing fields are ignored. **Bounded:** tolerate the *unrecognized*,
  never *reinterpret* a recognized line whose required shape changed — that drift must
  surface (the fixture diff and the command-number tripwire catch it), not be papered over.
- **Compatibility is tmux's verdict, not ours.** We do **not** implement a version-comparison
  gate. tmux's client and server already enforce their own `PROTOCOL_VERSION` match and the
  client errors (`protocol version mismatch …`) when they differ; we surface that verbatim.
  Version *detection* is telemetry only — expose the running version to the consumer; never
  branch on it.

## Consequences

- **No version-gating code.** This supersedes `spec/overview.md`'s "detect version, gate the
  newer signals" guidance — there is nothing to branch: the pinned target either has a signal
  in its format (we parse it unconditionally) or we don't target that version. Roadmap
  "Phase 4 — version detection & gating" collapses to "version *guard* + pin".
- **The only version-elasticity we keep** is the bounded liberal-accept (`Unknown` bucket) and
  the desync tripwire — graceful degradation, not adaptation. No `Error::TmuxVersionMismatch`
  and no refuse/warn knob: we accept and run, and tmux's own acceptance is the filter.
- **Users on a different tmux:** guaranteed only for the pinned version; other versions are
  best-effort (adjacent versions are mostly format-stable; `Unknown` absorbs the rest). The
  consumer can read the detected version and decide; tmuxctl itself never gates.
- **Bump runbook:** bump the pinned tmux ref → rebuild → regenerate fixtures → the `smoke`
  `CHANGED` diff over the fixtures *is* the upstream-wire-format changelog → patch the parser
  if needed → bump `tmuxctl` + the pinned ref → release. Low toil: tmux releases a few times
  a year.

## Open questions

- The exact pin mechanism in-repo (a `TARGET_TMUX` constant + the SHA) — settled when the
  integration harness lands (see the container test-strategy ADR).
- Whether to expose `Client::tmux_version()` now or when a consumer needs it.
