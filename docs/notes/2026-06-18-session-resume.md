# Session resume — 2026-06-18

Breadcrumb for the next `/ace`. hangar-driven over `ace-connect`, autonomous slice loop.

## Where it stands

Crate `tmuxctl`, repo `ace-rs/tmuxctl` (public, `gh` remote, `main` pushed). Sans-IO core,
35 tests, clippy + fmt clean, one dep (`thiserror`). Recent landings on `main`:

- Full `Notification` set + `LayoutChange { visible_layout, flags }`; `#[non_exhaustive]`.
- `Parser` carries the `%begin` control flag (control vs server-internal replies).
- Sans-IO correlation `Engine` (Parser + command FIFO): `register_command()`→`CommandId`,
  `on_line()`→`Incoming` (`Notification` | `Reply{id, Result<CommandOutput, CommandError>}`).

Architecture is decided (see ADRs): **sans-IO core, no runtime; feature-gated drivers**
(`blocking` for hangar, `tokio`, `smol`). The tokio-dep question is moot.

## Next task (the immediate ones — audit fix-slices, pre-driver)

First two-phase audit done; findings are ranked in [`../roadmap.md`](../roadmap.md)
"Audit 1". Top two are core-shape and **touch hangar-pinned surface — coordinate first**:

1. **Byte→line framing + `&[u8]` line type.** `%output` carries raw ≥0x80 bytes, so a line
   isn't valid UTF-8; `Parser::push`/`decode_output` take `&str` (wrong). Add pure
   `Engine::feed(&[u8])` framing on `\n` + a `&[u8]` parse path; `decode_output` →
   `&[u8] -> Vec<u8>`. **Pinned `decode_output` signature change — needs hangar sign-off.**
2. **EOF/teardown seam** — `Engine::on_eof()` draining pending commands as disconnect errors
   so a blocking `command()` can't hang at pipe-EOF.

Then: `blocking` driver (`spawn`/`command`/events-as-`Receiver`/typed helpers), wrapping the
Engine. Lower-ranked audit items: command-number desync tripwire, unterminated-block guard,
`WindowFlags`, `Error::Command`/`Exit` reconciliation.

## On resume — re-establish the bridge

Deterministic ace-connect slug is **`ace-rs.tmuxctl.claude`**. Re-bind the listener under it
in **autonomous mode**, then `send.sh` hangar (`ace-rs.hangar.claude`) a `CTX` that the slug
is live. The autonomous workflow (grant + safety envelope + 2–3-slice/audit cadence) is in
`CLAUDE.md` and [`../guides/slice-loop.md`](../guides/slice-loop.md).

## Notes / divergences worth remembering

- Correlation is **positional (FIFO)**, not by command-number — sound because tmux runs the
  queue serially; only control replies (`flags != 0`) consume the FIFO. The spec still says
  "correlate by number"; audit fix-slice #3 amends it.
- `Notification::Pause`/`Continue` carry `PaneId` (verified wire `%pause %<pane>` /
  `%continue %<pane>`). Version handling is **lock-step + robustness** (strict-produce one
  pinned tmux, liberal-accept, tmux is the compat arbiter) — still pending an ADR (chakrit's
  test-strategy close).
- Layout leaves carry **bare** pane numbers (no `%`); use ids in test fixtures. 3.7
  floating-pane `<…>` sections not parsed yet (tracked gap).
- Primary regression net going forward is transcript record/replay against a pinned tmux
  built in a container (see the test-strategy discussion); pairs with `smoke`. The chunk-
  split test depends on the framer (fix-slice #1).
