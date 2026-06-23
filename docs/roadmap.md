# tmuxctl roadmap

Sequencing from the published **v0.1.0** protocol crate toward complete tmux control-mode
coverage, then to the broader tmux-management utilities layered on top. Living doc — update as
slices land. The contract it builds toward is
[`spec/overview.md`](spec/overview.md); the wire details are in
[`reference/tmux-source-map.md`](reference/tmux-source-map.md).

## Where it stands

**Published v0.1.0** — an end-to-end client across three runtime drivers over one sans-IO core
(~54 tests on the default build, clippy+fmt clean; one default dep, `thiserror`, with `tokio`
and `smol` behind optional features):

- **Pure sans-IO core:** id newtypes, `decode_output(&[u8])`, `Layout` parse/render/checksum;
  the line `Parser` (`&[u8]`, reply framing + control flag, full `#[non_exhaustive]`
  `Notification` set, dropped-`%end` recovery); the correlation `Engine` (`feed(&[u8])`
  framer, `on_eof()`, positional-FIFO correlation with a monotonic-number tripwire).
  `WindowFlags`, `CommandOutput`, `CommandError`.
- **Three drivers over the core:** `blocking` `Client` (default, std-only) plus async
  `SmolClient` / `TokioClient` (per-task actor; no lock held across `.await`). All tested over
  an injected transport (`UnixStream` / in-memory duplex) — no real tmux needed.
- **Test pyramid (Phase 5) substantially in place:** pure units; transcript replay of a real
  3.6b capture through `Engine::feed`; injected-transport driver tests; `#[ignore]`d real-tmux
  integration (`scripts/integration.sh`, host tmux). Shipped via `scripts/release.sh`.

Next (all unblocked unless noted): the **pinned-tmux container** for reproducible integration
(the one remaining Phase 5 gap); the **`TARGET_TMUX` constant + version telemetry** (Phase 4);
**more typed command helpers** — flow control, subscriptions (Phase 3; layout push and
per-window resize have landed).

## Audit 1 — fix-slices (2026-06-18)

First two-phase audit (code-quality + architecture). Ranked; top two are core-shape
decisions that must land **before** the `blocking` driver, and both touch a hangar-pinned
surface (coordinating before implementing).

1. **Byte→line framing + `&[u8]` line type (BLOCKER). — DONE (`fa6483d`).** `decode_output`
   is now `&[u8] -> Vec<u8>`, `Parser::push` takes `&[u8]` (output decoded on the byte path,
   text lines `from_utf8_lossy`'d), and `Engine::feed(&[u8])` frames on `\n` buffering the
   partial tail. hangar-approved. The chunk-boundary-with-non-UTF-8 test now exists.
2. **EOF/teardown seam. — DONE (`d807525`).** `Engine::on_eof()` drains pending commands as
   `Err(CommandError::Disconnected)`; `CommandError` is now `Failed { lines } | Disconnected`.
   Driver signals EOF to event consumers by dropping the events `Sender`.
3. **Command-number desync tripwire.** Correlation is positional (FIFO) — sound, but the
   parsed `number` is dropped. Track it as a strictly-increasing assertion to catch a
   dropped/reordered block instead of silently mis-correlating; amend `spec/overview.md`
   (which says "correlate by number") to bless positional correlation.
4. **Unterminated-block guard.** A dropped `%end` makes `push_within_block` buffer the rest
   of the stream forever. Treat a top-level `%begin` mid-block (or a size bound) as a desync
   signal. (Related to #1's framer.)
5. **`WindowFlags` for `LayoutChange.flags`.** `Option<String>` is a stringly-typed leak of
   a known bitset (`*` current, `Z` zoomed, `!` bell, …). Parse to a hand-rolled
   `WindowFlags` (no new dep) at the boundary. Before a consumer pins `LayoutChange`.
6. **Reconcile `Error::Command`/`Error::Exit` vs `CommandError`/`Notification::Exit`.**
   Parallel structures over the same failures; the crate-`Error` variants may be pre-sans-IO
   leftovers. Decide when typed helpers land (they're the `Error::Command` consumer); drop
   if dead.

Nits (deferred): `parse_guard` accepts trailing tokens (tighten to reject); `parse_subscription`
discards the id header (document as intentional or capture); subscription `name` assumes no
spaces. Declined: restructuring `Reply.error: bool` into a sum type — the parser frames
blocks, command-semantics `Result` belongs to the engine; bool at the parser layer is correct
layering (rationale recorded here).

## Audit 2 — fix-slices (2026-06-18, post byte-refactor + blocking driver)

Concurrency verdict: **`command()` cannot hang** — every interleaving verified (the
`connected` flag + single-lock register/write/drain serialization rule out an orphaned
waiter; the events channel is unbounded so the lock is never held across a blocking send).
Sans-IO boundary re-confirmed clean (`--no-default-features` builds the pure core). N3 (a
claimed `decode_output` off-by-one) was a **false positive** — disproven by two agents + a
regression test (`b25ca60`).

- **A2-1. Blank lines inside reply blocks dropped (BLOCKER). — DONE (`5bff98a`).** `feed`'s
  empty-line skip ran beneath block buffering; moved the top-level-only skip into the parser.
- **A2-2. `command()` panicked on a poisoned lock. — DONE (`77f40c8`).** Now returns
  `Disconnected` if the reader thread panicked.
- **A2-3. `ExtendedOutput.ms_behind` `u32` → `u64`. — DONE (`9888f55`).** hangar-approved.
- **A2-4. `child: Option<Child>` on `Client`.** `spawn` must hold and reap the tmux child or
  it orphans a zombie. Lands with driver Slice B.
- **#4 unterminated-block guard. — DONE (`0580d79`).** A mid-block `%begin` flushes the
  truncated block as an error reply and resyncs, instead of buffering the stream forever.
- **#5 `WindowFlags`. — DONE (`9888f55`).** `LayoutChange.flags` is now `Option<WindowFlags>`
  covering the full tmux flag set, unmodeled chars retained. hangar-approved.
- **#6 dead `Error::Io`/`Command`/`Exit`. — DONE (`ee9511a`).** Dropped; `Error` is now
  `Layout`-only and `#[non_exhaustive]`. hangar-approved.
- **#3 desync tripwire. — DONE (`31389a4`).** The parsed reply `number` is now a
  strictly-increasing `debug_assert` (positional FIFO stays the correlation); spec amended to
  bless positional-not-numeric correlation.
- **A2-4 `child: Option<Child>`. — DONE (`8355314`).** `spawn` holds and reaps the tmux child
  in `Drop`.
- Nits: write-failure leaves an orphaned id in `pending` (harmless, `on_eof` drains it).

**Audit 2 fully resolved.** Audit 3 due after ~2–3 more feature slices.

Async-driver note (it informed the `tokio`/`smol` drivers that have since landed): `blocking`'s
single `Mutex<Shared>` is held across `writer.write_all`, which won't survive `.await` — an
async driver needs an async-aware mutex or a dedicated writer task, not a verbatim reuse of
`Shared`. Both async drivers use a per-task actor instead.

## Phase 0 — Complete notification coverage — DONE

Landed (`e45a6b9`, `5cf4d77`): the full notification set + `LayoutChange` carrying
`visible_layout`/`flags`, and the reply control-flag. Caveat: `%client-detached`,
`%paste-buffer-changed`, `%paste-buffer-deleted` exist in tmux but are intentionally left to
`Notification::Unknown` for now (not in hangar's pinned set) — "complete" means the pinned
set, not every tmux line. `Layout` is tiled-only (no `<…>` float sections) — **correct for the
pinned 3.6b target, which has no floating panes**, not a gap. Native float parsing is deferred for
now (3.6b has none; 3.7's are alpha) and lands in a future tmuxctl as the target bumps; the overlay
effect stays hangar's to composite client-side meanwhile. See
[the target ADR](decisions/2026-06-21-target-tmux-3.6b-floats-out-of-scope.md).

## Phase 1 — Runtime decision (RESOLVED)

Resolved 2026-06-18 (chakrit): **sans-IO core, no runtime; feature-gated drivers.** The
core (Parser + reply-correlation state machine) is pure and synchronous; runtime support is
feature-gated drivers (`blocking` — hangar's choice — plus `tokio`, `smol`) that own the
process and pump bytes through the core. No mandatory async dep. See
[`decisions/2026-06-18-sans-io-core-feature-gated-drivers.md`](decisions/2026-06-18-sans-io-core-feature-gated-drivers.md).
Supersedes the spec's "Async on tokio" sketch.

## Phase 2 — `Client` (sans-IO core + `blocking` driver) — DONE

Landed end-to-end: the pure correlation `Engine`, then the `blocking` `Client` (`4e6f9df`,
`2c6eba9`, `8355314`). `Client::spawn(SpawnOpts)` runs `tmux -C` (**not** `-CC`) over piped
stdin/stdout, holds/reaps the child; `command()` blocks on a per-command channel; events are
a `Receiver<Notification>`; teardown detaches on an empty line and treats EOF/`Disconnected`
as session end; `%error` → `Err(CommandError::Failed)`. Reply correlation is positional FIFO
with a monotonic-number tripwire.

The **`tokio` driver** (`TokioClient`) has landed too (`2bbc518`) behind the `tokio` feature —
actor pattern (owner task `select!`s commands vs. stdout; no lock across `.await`), tested
over `tokio::io::duplex`. Shared `SpawnOpts`/argv (`spawn.rs`) and command-string builders
(`commands.rs`) keep the drivers DRY. The **`smol` driver** (`SmolClient`) has **also landed**
behind the `smol` feature — the same actor pattern over `async-process`/`futures-lite`, tested
over an in-memory duplex (`cargo test --features smol` green). All three drivers now ship.

## Phase 3 — Typed command helpers (partial)

Thin, typed wrappers over a raw `command(&str)` escape hatch (which stays primary):

- **DONE (`8355314`):** `send_keys` (`send-keys -H` hex bytes) and `resize` (`refresh-client
  -C <cols>x<rows>`).
- **DONE (`7ecaf63`):** `select_layout` (`select-layout -t @<w>` with a regenerated checksum
  via `to_layout_string()`) on all three drivers; tmux arbitrates validity (`%error`).
- **DONE (hangar ASK, per-window control-mode sizing):** `resize_window` (`refresh-client -C
  @<w>:<cols>x<rows>`, the `@%u:%ux%u` form at cmd-refresh-client.c:90) on all three drivers;
  layers over the global `resize`, tmux bounds-checks (`%error`).
- Open: flow control (`refresh-client -f pause-after=`, `-A '%p:continue'`).
- Open question: how much command surface to type vs. leaving raw primary — typed the two
  high-use ones, defer the rest.

## Phase 4 — Version guard + pin (collapsed)

Per [the lock-step ADR](decisions/2026-06-18-lock-step-tmux-and-robustness.md), there is **no
version-gating**: target one pinned tmux (by version + release tag, **not** a commit SHA),
produce strictly, accept liberally, let tmux be the compat arbiter. **Pinned target resolved
(2026-06-21): tmux `3.6b`** (the `3.6b` release tag — see
[the target ADR](decisions/2026-06-21-target-tmux-3.6b-floats-out-of-scope.md)). No per-version
branches, no SHAs in code. A standalone `TARGET_TMUX` const was tried and reverted — it had no
consumer and duplicated the ADR's pin; the pin re-enters code only as a runtime **drift-check**
(`#{version}` vs the target) wired into the integration harness (Phase 5). **Follow-up
fix-slice:** re-anchor the source map's line numbers from `next-3.7` to 3.6b (algorithms/format
strings hold; only line numbers + the `<…>` float section drift).

## Phase 5 — Regression net & integration — MOSTLY DONE

The four-layer pyramid from
[the container test-strategy ADR](decisions/2026-06-18-container-test-strategy.md) is built
except its top rung:

- **Pure units** — core types, parser, engine, layout, decode. DONE.
- **Transcript replay** — `tests/fixtures/structural-session.txt` (a real 3.6b `tmux -C`
  capture) replayed through `Engine::feed`, asserting the exact `Notification` stream and that
  no line falls to `Unknown` (`tests/transcript.rs`). DONE. (Asserts in-Rust; the ADR's
  `smoke` golden-file flavor was not adopted — optional, revisit if fixtures multiply.)
- **Injected-transport driver** — `with_transport` over `UnixStream` / duplex for all three
  drivers. DONE.
- **Real-tmux integration** — `tests/integration.rs`: `#[ignore]`d, `TMUXCTL_TMUX_BIN`-keyed,
  per-test socket isolation, run via `scripts/integration.sh` (no Actions). DONE against the
  **host** tmux.

**Remaining gap:** the **pinned-tmux container** (a Dockerfile building 3.6b) so integration is
reproducible off-host, plus the fixture-generator loop that regenerates transcripts from it. A
couple more fixtures would also widen replay coverage.

## Phase 6 — Publishing — DONE

**v0.1.0 released** (`81eb4de`): README usage section, `scripts/release.sh` (gate → tag →
`gh release` → `cargo publish`, idempotent re-run), live on crates.io + a GitHub release at
tag `v0.1.0`. Bump the version (`cargo set-version`) before the next release. The
`blocking`/`smol`/`tokio` drivers have all landed since; the pinned-tmux container (Phase 5)
remains for a later release.

## Beyond the protocol layer — tmux management utilities

Once the control-mode surface is fully covered and published, layer higher-level tmux
management on top (separate crate(s) or feature-gated modules; scope TBD with chakrit). The
protocol crate stays pure and consumer-agnostic; utilities depend on it, never the reverse.
Candidates to scope when we get there: session/window orchestration, layout presets,
persistence/restore, multi-session supervision. Defer concrete design until Phase 6 lands.

## Critical path

Phases 0 → 1 → 2 → 6 are **DONE** (notification coverage, runtime decision, all three drivers,
published v0.1.0); Phase 5 is **mostly done** (pyramid built, container pending). Remaining, in
rough priority: finish **Phase 5** (pinned-tmux container) and **Phase 4** (`TARGET_TMUX`
constant + version telemetry) to make the lock-step net reproducible, then **Phase 3** typed
helpers (flow control, subscriptions — layout push and per-window resize landed) as consumers
need them, then the
**utilities** layer. None of the remaining work blocks the rest.
