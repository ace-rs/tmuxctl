# Test strategy: pinned-tmux integration, container-produced, fixture-generating

**Date:** 2026-06-18
**Status:** Accepted (chakrit)

## Context

The pure core is unit-tested and the blocking driver is tested over an in-memory transport,
but nothing yet verifies the protocol against **real tmux** — and real tmux is the only oracle
for the parts a fake cannot reach (chiefly the *write* side: that our emitted commands are
actually accepted). We want that verification without an unwinnable version matrix (see the
lock-step ADR) and without GitHub Actions (house ban).

## Decisions

**A four-layer test pyramid, keyed by oracle and cost:**

| Layer | What | Needs tmux? | Cadence |
|-------|------|-------------|---------|
| 1 | Pure unit (parser, layout, decode, engine) | no | every `cargo test` |
| 2 | Transcript replay — snapshot the `Incoming`/`Notification` stream | no — bytes committed | every `cargo test` |
| 3 | Driver correlation/framing over an injected transport (`UnixStream` pair) | no | every `cargo test` |
| 4 | Containerized real-tmux integration | yes, pinned | pre-release / on tmux bump / when touching framer·correlation·command-emission |

Layers 1–3 exist today and gate every change. Layer 4 is the truth oracle, run deliberately.

**Decouple the pinned binary from the container.** The requirement is *a pinned tmux binary
at a known version*; the container is just one reproducible way to produce it. The integration
suite keys off **`TMUXCTL_TMUX_BIN`** (a path + version), never off Docker — so it runs against
a host-built, Nix-built, or container-built tmux alike. Integration tests are `#[ignore]`d
(and/or behind a feature) so default `cargo test` stays pure and fast.

**Integration does double duty: it is also the fixture generator.** The same harness that
asserts live also records the raw `tmux -C` byte stream to `tests/fixtures/`, committed. The
fast layer-2 replay suite then consumes those authentic, version-stamped goldens with no
process and no executor (`Engine::feed(&[u8]) -> Vec<Incoming>` is the replay seam). The
expensive container run refreshes goldens; the everyday suite stays cheap. Fixtures must come
from real captures — hand-written wire bytes would only encode our own assumptions.

**Single pinned tmux, not a matrix** (lock-step ADR): one tmux build, one fixture set.

**Per-test isolation (mandatory):** each integration test spawns its own server — unique
socket (`-L tmuxctl-test-…`), `-f /dev/null`, fixed size, `escape-time 0`, explicit `TERM` +
UTF-8 locale, `kill-server` teardown — so tests are parallel-safe and never touch a real
session.

**Reproducible pin:** tmux by **commit SHA** (+ pinned libevent/ncurses, base-image digest,
`--enable-utf8proc`). The container is built/run by a local **`scripts/integration.sh`** — no
CI, no Actions, ever. Integration is **not** an automatic gate; the cadence above is the
discipline.

## Consequences

- What layer 4 uniquely buys that fakes can't: the **write side** (commands tmux accepts),
  layout round-trip against `select-layout` *acceptance*, the byte→line framer under real
  chunking, and teardown/EOF semantics.
- `smoke` caveat (the snapshot net): `CHANGED` means "read the diff," not "broken" — a human/
  agent eyeballs whether a fixture diff is real wire drift or a regression; it is never
  auto-greened.
- Layer 4 + the bump runbook (lock-step ADR) are the same machinery: a tmux bump regenerates
  fixtures and the `CHANGED` diff is the drift alarm.
- Roadmap "Phase 5 — regression net & integration" is now specified by this ADR.

## Open questions

- `#[ignore]` + env-detect vs. a dedicated `integration` cargo feature for gating layer 4.
- Whether the container ships a Nix or a plain Debian-pinned toolchain for the tmux build.
