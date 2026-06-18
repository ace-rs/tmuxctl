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

## Next task (unblocked, pure — next slices)

1. **#3 desync tripwire.** Correlation is positional FIFO (sound). Track the parsed reply
   `number` as a strictly-increasing assertion to catch a dropped/reordered block instead of
   silently mis-correlating. Amend `docs/spec/overview.md` ("correlate by number") to bless
   positional correlation. Pure, no pinned-surface change.
2. **Driver Slice B.** `Client::spawn` (real `tmux -C`, **not** `-CC`; add a
   `child: Option<Child>` field and reap it in `Drop` — else zombie) + typed helpers
   `send_keys` (`-l`/`-H`) and `resize` (`refresh-client -C`). `SpawnOpts` lives in the
   driver, not the core. Spawn's integration test needs real tmux → gated/deferred (depends
   on the open container test-strategy decision); the helpers are testable over the fake
   transport (assert the bytes written).

Then: version guard (lock-step), transcript regression net (Phase 5), publishing.

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
