# Session resume — 2026-06-18/19

Breadcrumb for the next `/ace`. hangar-driven over `ace-connect`, autonomous slice loop
(`/ace-afk` for unattended runs). Two audit cycles done; crate is feature-complete and
release-ready.

## Where it stands

Crate `tmuxctl`, repo `ace-rs/tmuxctl` (public). `main` pushed through `fca5801`; many
commits since are **local/unpushed** (additive — don't touch hangar's pinned blocking
surface). 52 default tests + 3 `--all-features` + 2 `#[ignore]`d integration; clippy + fmt
clean. Deps: `thiserror` (always) + optional `tokio`/`smol` behind features.

- **Sans-IO core (pure, no runtime):** id newtypes; `decode_output(&[u8])`; `Layout`
  parse/render/checksum; line `Parser` (`&[u8]`; reply framing + control flag; full
  `#[non_exhaustive]` `Notification` set; dropped-`%end` recovery; trailing-token guard);
  correlation `Engine` (`feed(&[u8])` framer, `on_eof()`, positional-FIFO + monotonic-number
  tripwire). `WindowFlags`, `CommandOutput`, `CommandError` (`Failed`|`Disconnected`).
- **Three drivers**, one core, feature-gated: `Client` (`blocking`, default, std threads),
  `TokioClient` (`tokio`), `SmolClient` (`smol`) — async via the actor pattern. Each:
  `spawn(SpawnOpts)` / `command` / `send_keys` / `resize` / events `Receiver` / teardown.
  Shared `spawn` (SpawnOpts + builder + argv) and `commands` (command strings) modules.
- **Test pyramid:** units; transcript replay of a real tmux 3.6b capture
  (`tests/fixtures/structural-session.txt`, asserts no-`Unknown`); injected-transport driver
  tests; live integration (`tests/integration.rs` + `scripts/integration.sh`, `#[ignore]`d,
  keyed off `TMUXCTL_TMUX_BIN`, verified green vs tmux 3.6b).
- **Docs:** README with usage; ADRs (crate-name, sans-IO, lock-step, container test-strategy);
  `scripts/release.sh` (dry-run verified, `--execute` publishes).

## Next — needs chakrit/hangar decisions (not autonomously unblocked)

1. **Push** the local commits (≈9 since `fca5801`). Authorized on hangar request per the
   grant; hangar hasn't requested a new push yet. Say "push" or have hangar request it.
2. **Publish 0.1.0** to crates.io — `scripts/release.sh --execute`. Authorized on hangar
   request; hangar pins by git rev today and hasn't asked for a crates.io version.
3. **Which tmux to pin** (`TARGET_TMUX` SHA) + the **container Dockerfile** that builds it —
   needed to make integration reproducible beyond the host's tmux 3.6b. The container ADR
   specifies the shape; the version/base choice is yours.
4. **More typed helpers?** (per-window resize, flow control, layout push) — hangar-driven;
   build when a consumer needs them.

## Low-value / deferred (autonomous-OK but minor)

- `Client::tmux_version()` telemetry (lock-step ADR); `parse_subscription` id-header capture.

## Re-establish the bridge

Slug **`ace-rs.tmuxctl.claude`**, autonomous mode. Standing grant (CLAUDE.md): hangar-requested
push + cargo release proceed without per-action approval (gates-green + sane-version gated).
Workflow: CLAUDE.md + [`../guides/slice-loop.md`](../guides/slice-loop.md).

## Notes / divergences

- Correlation is **positional FIFO** (sound; tmux serial), control replies (`flags != 0`)
  only; the parsed `number` is a monotonic `debug_assert` desync tripwire.
- Commit messages: **no backticks in `git commit -m`** (shell command-substitution mangles
  them — hit twice).
- Async drivers don't reuse `blocking`'s `Mutex<Shared>` (held across a write) — they use a
  per-task actor (`select!` / `smol::future::or`).
- `SpawnOpts` is `#[non_exhaustive]` → external crates must use the builder, not a literal.
- 3.7 floating-pane `<…>` layout sections not parsed yet (tracked gap).
