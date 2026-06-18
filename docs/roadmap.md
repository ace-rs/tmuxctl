# tmuxctl roadmap

Sequencing from the current pre-`Client` state to a published crate that covers the entire
tmux control-mode signal surface, then to the broader tmux-management utilities layered on
top. Living doc — update as slices land. The contract it builds toward is
[`spec/overview.md`](spec/overview.md); the wire details are in
[`reference/tmux-source-map.md`](reference/tmux-source-map.md).

## Where it stands

Pure, synchronous, sans-IO core, tested (35 tests, clippy+fmt clean), one dep (`thiserror`):

- Wire ids (`PaneId`/`WindowId`/`SessionId`), `decode_output` (octal unescape),
  `Layout::parse`/`render`/`checksum` round-trip.
- Incremental line `Parser`: `%begin`…`%end`/`%error` reply framing carrying the control
  flag, plus the **complete** typed `Notification` set (`#[non_exhaustive]`).
- Sans-IO correlation `Engine`: `register_command()`→`CommandId`, `on_line()`→`Incoming`,
  FIFO correlation, server-internal replies skipped. `CommandOutput`/`CommandError`.

Next: the byte→line framing + `&[u8]` seam and the EOF/teardown seam (audit fix-slices
below, both pre-driver), then the `blocking` driver. Not started: drivers, version guard,
transcript regression net, publishing.

## Audit 1 — fix-slices (2026-06-18)

First two-phase audit (code-quality + architecture). Ranked; top two are core-shape
decisions that must land **before** the `blocking` driver, and both touch a hangar-pinned
surface (coordinating before implementing).

1. **Byte→line framing + `&[u8]` line type (BLOCKER).** `%output` passes bytes ≥0x80 raw,
   so an output line is not valid UTF-8 — but `Parser::push`/`decode_output` take `&str`.
   Nothing frames the raw stream into lines across reads either. Add a pure
   `Engine::feed(&[u8])` that frames on `\n`, buffers the partial tail, and hands byte-lines
   to a `&[u8]`-taking parse path; `decode_output` becomes `&[u8] -> Vec<u8>` (text fields
   still `str`-parsed). **Changes the pinned `decode_output` signature — hangar sign-off
   first.** Unblocks the spec's "UTF-8 split across chunk boundaries" test.
2. **EOF/teardown seam.** Pending commands at pipe-EOF never resolve → a blocking
   `command()` would hang forever. Add `Engine::on_eof()` that drains the FIFO, resolving
   each waiter as a disconnect error. Decide the seam before the driver.
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

## Phase 0 — Complete notification coverage — DONE

Landed (`e45a6b9`, `5cf4d77`): the full notification set + `LayoutChange` carrying
`visible_layout`/`flags`, and the reply control-flag. Caveat: `%client-detached`,
`%paste-buffer-changed`, `%paste-buffer-deleted` exist in tmux but are intentionally left to
`Notification::Unknown` for now (not in hangar's pinned set) — "complete" means the pinned
set, not every tmux line. `Layout` does not yet parse 3.7 floating-pane `<…>` sections —
tracked gap, add when targeting that.

## Phase 1 — Runtime decision (RESOLVED)

Resolved 2026-06-18 (chakrit): **sans-IO core, no runtime; feature-gated drivers.** The
core (Parser + reply-correlation state machine) is pure and synchronous; runtime support is
feature-gated drivers (`blocking` — hangar's choice — plus `tokio`, `smol`) that own the
process and pump bytes through the core. No mandatory async dep. See
[`decisions/2026-06-18-sans-io-core-feature-gated-drivers.md`](decisions/2026-06-18-sans-io-core-feature-gated-drivers.md).
Supersedes the spec's "Async on tokio" sketch.

## Phase 2 — `Client` (sans-IO core + `blocking` driver)

First the pure correlation core, then the `blocking` reader-thread driver around it.
`tokio`/`smol` drivers follow. Spawn `tmux -C` (**not** `-CC`) over separate stdin/stdout
pipes; pump stdout lines through `parser::Parser`.

- **Reply correlation (pure core):** a state machine, not a runtime primitive — register a
  command, match `%begin`…`%end`/`%error` back by command-number, resolve. Numbers are
  monotonic but **sparse** and **process-global** — never assume start-at-0 or +1. Drivers
  layer ergonomics on top (`blocking`: `command() -> Result`; async drivers: `async fn`).
- **Event surface:** the driver exposes the async `Notification`s (iterator for `blocking`,
  stream for async drivers).
- **Teardown:** detach on an explicit empty-line write; treat pipe EOF as session end (the
  `%exit` gotcha — it comes from the client process, not the server emitter).
- **Errors:** `%error` blocks resolve their future as `Err(CommandError)` carrying the
  output lines.

## Phase 3 — Typed command helpers

Thin, typed wrappers over a raw `command(&str)` escape hatch (which stays primary):

- `send_keys_literal` (`send-keys -l`/`-H` for raw bytes/control sequences).
- `resize` (`refresh-client -C`), per-window `@w:<wxh>` form.
- Flow control (`refresh-client -f pause-after=`, `-A '%p:continue'`).
- Layout push (`select-layout` with a regenerated checksum).
- Open question: how much command surface to type vs. leaving raw primary — type the four
  above, defer the rest.

## Phase 4 — Version detection & gating

Detect the spawned tmux version on handshake; gate the newer signals
(`%pause`/`%continue`/`%extended-output`, `%pane-mode-changed`, `%subscription-changed`, the
extra `%layout-change` args). Keep tolerate-and-log for unknown `%`-lines regardless.

## Phase 5 — Regression net & integration

- **Transcript record/replay** (primary net): capture real `tmux -C` byte streams, replay
  through the parser, assert the `Notification` stream. Pairs with the `smoke` skill for
  golden-file snapshots.
- **Live integration** via a local `scripts/*.sh` (no GitHub Actions): spawn real tmux,
  create+split a window, send keys, assert events + layout.

## Phase 6 — Publishing

README usage section, `scripts/release.sh` (build, checksum, `gh release create`, `cargo
publish`), confirm dual LICENSE files. Semver from `0.x`. Open question: publish before or
alongside the first consumer release.

## Beyond the protocol layer — tmux management utilities

Once the control-mode surface is fully covered and published, layer higher-level tmux
management on top (separate crate(s) or feature-gated modules; scope TBD with chakrit). The
protocol crate stays pure and consumer-agnostic; utilities depend on it, never the reverse.
Candidates to scope when we get there: session/window orchestration, layout presets,
persistence/restore, multi-session supervision. Defer concrete design until Phase 6 lands.

## Critical path

Phase 0 (now) → Phase 1 (decision) → Phase 2 → 3 → 4 → 5 → 6 → utilities. Phases 0 and the
regression fixtures (part of 5) can proceed in parallel with the Phase 1 decision since both
are pure and runtime-free.
