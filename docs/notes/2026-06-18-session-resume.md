# Session resume — 2026-06-18

Breadcrumb for the next `/ace`. hangar-driven over `ace-connect`, autonomous slice loop
(`/ace-afk` for unattended runs). Two audit cycles done.

## Where it stands

Crate `tmuxctl`, repo `ace-rs/tmuxctl` (public, `gh` remote, `main` pushed — **push pending**,
nothing pushed since the initial scaffold). 47 tests, clippy + fmt clean, one dep
(`thiserror`). The sans-IO core + the first driver are in place:

- **Core (pure, no runtime):** id newtypes; `decode_output(&[u8])`; `Layout`
  parse/render/checksum; the line `Parser` (`&[u8]`, reply framing + control flag, full
  `Notification` set, recovers from a dropped `%end`); the correlation `Engine`
  (`feed(&[u8])` framer, `on_eof()`, FIFO correlation). `WindowFlags`, `CommandOutput`,
  `CommandError` (`Failed`|`Disconnected`). `Error` is `Layout`-only, `#[non_exhaustive]`.
- **`blocking` driver (default feature, std-only):** `Client::with_transport` — reader
  thread over `Engine::feed`, `command()` blocking on a per-command channel, events as a
  `Receiver`, EOF→`Disconnected` teardown, poison-tolerant. Unit-tested over a `UnixStream`
  pair (no real tmux).

Two audits passed. Audit 2 concurrency verdict: **`command()` cannot hang**. All audit
findings resolved except the two below.

## Next task

The blocking client is complete end-to-end (spawn/command/send_keys/resize/events/teardown).
Audit 2 fully resolved (incl. #3 desync tripwire `31389a4` and Slice B `8355314`). Candidates
next, in rough order:

1. **Real-tmux integration tests** — the one untested seam is `Client::spawn` (and the live
   round-trip). **Gated on chakrit's container test-strategy decision** (build a pinned tmux,
   replay; integration doubles as transcript-fixture generator). Until then, `spawn` is
   code-complete but smoke-untested.
2. **Transcript regression net** (Phase 5) — `Engine::feed(&[u8]) -> Vec<Incoming>` is the
   right replay seam; pairs with the `smoke` skill.
3. **`smol` driver** — `tokio` is DONE (`TokioClient`, `2bbc518`, actor pattern, behind the
   `tokio` feature). `smol` mirrors it on `async-process`/`futures-lite`/`async-channel`; same
   actor shape. Reuse the shared `spawn`/`commands` modules. Don't copy `blocking`'s
   `Mutex<Shared>` (held across a write — won't survive `.await`).
4. **Version guard** (lock-step ADR — write the `TARGET_TMUX` pin) and **publishing**.

Audit 3 is due after ~2–3 more feature slices.

## Open decisions (chakrit's — not blocking driver work)

- **Lock-step + robustness ADR** (strict-produce one pinned tmux, liberal-accept, tmux is
  the compat arbiter) — discussed, not yet written. The test-strategy synthesis (container
  builds a pinned tmux; integration doubles as transcript-fixture generator) is unratified.

## Re-establish the bridge

Slug **`ace-rs.tmuxctl.claude`**, autonomous mode. On resume: re-bind the listener, `send.sh`
hangar (`ace-rs.hangar.claude`) a `CTX` that the slug is live. Autonomous workflow (grant +
safety envelope + 2–3-slice/audit cadence) is in `CLAUDE.md` +
[`../guides/slice-loop.md`](../guides/slice-loop.md).

## Notes / divergences

- Correlation is **positional (FIFO)**, not by command-number — sound (tmux runs the queue
  serially); only control replies (`flags != 0`) pop the FIFO. #3 adds the number tripwire.
- Commit messages: **no backticks in `git commit -m`** — the shell runs them as command
  substitution and mangles the message (hit twice; amended both).
- Layout leaves carry bare pane numbers; 3.7 floating-pane `<…>` sections not parsed yet.
- Async-driver note (future `tokio`/`smol`): `blocking`'s single `Mutex<Shared>` is held
  across `write_all` — won't survive `.await`; don't reuse `Shared` verbatim.
