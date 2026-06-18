# tmuxctl roadmap

Sequencing from the current pre-`Client` state to a published crate that covers the entire
tmux control-mode signal surface, then to the broader tmux-management utilities layered on
top. Living doc — update as slices land. The contract it builds toward is
[`spec/overview.md`](spec/overview.md); the wire details are in
[`reference/tmux-source-map.md`](reference/tmux-source-map.md).

## Where it stands

Pure, synchronous, dependency-free core is in place and tested (21 tests, clippy+fmt clean):

- Wire ids (`PaneId`/`WindowId`/`SessionId`), `decode_output` (octal unescape),
  `Layout::parse`/`render`/`checksum` round-trip.
- Incremental line `Parser`: `%begin`…`%end`/`%error` reply framing by command-number, plus
  a typed `Notification` for most async `%`-lines.

Not started: the async `Client` (spawn + writer + reply correlation + event stream), full
notification coverage, version gating, the transcript regression net, and publishing.

## Phase 0 — Complete notification coverage (no new deps, do now)

The parser silently routes several real `%`-lines to `Notification::Unknown`. Close the gap
while it's still pure code — no runtime decision blocks this.

| Wire line                                       | Status        | Work                                    |
|-------------------------------------------------|---------------|-----------------------------------------|
| `%layout-change @w <layout> <vis> <flags>`      | lossy         | capture `visible_layout` + flags (zoom) |
| `%window-pane-changed @w %p`                     | → `Unknown`   | add variant + parse                     |
| `%unlinked-window-add/-close/-renamed @w`        | → `Unknown`   | add variants (windows in other sessions)|
| `%session-renamed $s <name>`                     | → `Unknown`   | add variant + parse                     |
| `%session-window-changed $s @w`                  | → `Unknown`   | add variant + parse                     |
| `%client-session-changed <client> $s <name>`     | → `Unknown`   | add variant + parse                     |
| `%continue %p` payload                           | verify        | confirmed `%continue %<pane>` (ADR note)|

Each lands tests-first against fixture lines pulled from the source map. The
`%layout-change` fix changes the `LayoutChange` variant shape — do it before any consumer
pins the type.

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
