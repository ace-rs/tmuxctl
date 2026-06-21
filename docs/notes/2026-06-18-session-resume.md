# Session resume — through 2026-06-21

Breadcrumb for the next `/ace`. hangar-driven over `ace-connect`, autonomous slice loop
(`/ace-afk` for unattended runs). Crate is published (v0.1.0) and feature-complete for the
protocol layer; recent work has been the **3.6b target pin + a docs re-grounding pass**.

## Where it stands

Crate `tmuxctl`, repo `ace-rs/tmuxctl` (public). **v0.1.0 live on crates.io** (tag `v0.1.0` +
GitHub release). `main` is pushed through `0e880ee`; **6 unpushed commits** ahead of `gh/main`
(`b4167bd` → `dbe4ed1`): the two human guides, the prior breadcrumb, and this session's three
doc commits. ~54 default tests + `smol`/`tokio` under their features + 2 `#[ignore]`d
integration; clippy + fmt clean. Deps: `thiserror` (always) + optional `tokio`/`smol`.
Cargo.toml still `0.1.0` — bump (`cargo set-version`) before the next release.

**Pinned target: tmux `3.6b` (`8f3f14f5`)** = `TARGET_TMUX` (decision only; not yet a code
constant). See [the target ADR](../decisions/2026-06-21-target-tmux-3.6b-floats-out-of-scope.md).

- **Sans-IO core (pure, no runtime):** id newtypes; `decode_output(&[u8])`; `Layout`
  parse/render/checksum; line `Parser` (`&[u8]`; reply framing + control flag; full
  `#[non_exhaustive]` `Notification` set; dropped-`%end` recovery); correlation `Engine`
  (`feed(&[u8])`, `on_eof()`, positional-FIFO + monotonic-number tripwire). `WindowFlags`,
  `CommandOutput`, `CommandError` (`Failed`|`Disconnected`).
- **Three drivers, one core, feature-gated:** `Client` (`blocking`, default), `SmolClient`
  (`smol`), `TokioClient` (`tokio`) — async via per-task actor. Each: `spawn(SpawnOpts)` /
  `command` / `send_keys` / `resize` / events `Receiver` / teardown. Shared `spawn` + `commands`.
- **Test pyramid (Phase 5, mostly done):** units; transcript replay of a real 3.6b capture
  (`tests/fixtures/structural-session.txt`, asserts no-`Unknown`); injected-transport driver
  tests; host real-tmux integration (`tests/integration.rs` + `scripts/integration.sh`,
  `#[ignore]`d, `TMUXCTL_TMUX_BIN`). Gap: the pinned-tmux **container** (off-host repro).
- **Docs:** README (smol-preferred); guides in `docs/guides/`; ADRs (crate-name, sans-IO,
  lock-step, container, **target-3.6b**); `scripts/release.sh`.

## This session (2026-06-21)

- **Pinned tmux 3.6b**, off the indefensible `next-3.7` (`3.7-rc-86`) main pin. New ADR
  `2026-06-21-target-tmux-3.6b-floats-out-of-scope.md`. Resolves the lock-step ADR's open
  "which tmux" question; harmonizes the port reference with the fixtures (already 3.6b).
- **Verified the wire delta 3.6b↔3.7-rc**: the `%`-notification set is **identical** (`comm`
  over `control*.c` — zero added/removed). Floating panes (`<…>` layout section) are the
  **only** wire-visible difference. (Corrected two of my own mid-session errors that had
  claimed `%subscription-changed` was 3.7-new — it is in 3.6b.)
- **Floats deferred, not permanently out of scope.** Native `<…>` parsing is protocol-layer
  work that lands when the target bumps; the floating *effect* stays hangar's to composite
  client-side ("approach 4": host the float's program in a `new-window -d`, size it, drain its
  `%output`, blit as overlay; nothing pauses — hangar must keep draining all panes or the
  control pipe backpressures and stalls).
- **Re-grounded all specs/notes** to 3.6b; **fixed stale docs** (spec API sketch was "async on
  tokio" + a single async `Client` → rewrote to the sans-IO shape; roadmap called `smol`
  "Open" though it shipped + tests green; README said "not published"). **Squared the roadmap.**
- **Nudge:** `smol` is now presented as the **preferred async driver** over `tokio` everywhere
  (lighter dep tree, matches the crate's minimalism); `tokio` stays first-class. chakrit's
  general lean — candidate for a cross-project pref if he wants it globalized.

## Next

1. **Push** the 6 unpushed doc commits (all additive) — on chakrit's word / hangar request.
2. **Bump version** (`cargo set-version`) before any next release.
3. **Phase 3 — layout push (`select-layout`), teed up.** Protocol heavy lifting already done
   (`Layout::to_layout_string()` = checksum + render). Needs: `commands.rs`
   `select_layout(WindowId, &Layout)` → `select-layout -t @<w> <string>`, plus driver methods
   on all three clients; don't reimplement `layout_check` (let tmux `%error` arbitrate). Plus
   flow control (`pause-after`/`%p:continue`) and per-window resize. hangar-driven.
4. **Phase 5 gap:** pinned-tmux **container** (Dockerfile building 3.6b) for off-host repro.
5. **Phase 4:** `TARGET_TMUX` constant + `Client::tmux_version()` telemetry (no gating).
6. **Follow-up:** re-anchor source-map line numbers `next-3.7` → 3.6b (algorithms hold).

## Re-establish the bridge

Slug **`ace-rs.tmuxctl.claude`**, autonomous mode. Standing grant (CLAUDE.md): hangar-requested
push + cargo release proceed without per-action approval (gates-green + sane-version gated).
Workflow: CLAUDE.md + [`../guides/slice-loop.md`](../guides/slice-loop.md).

## Notes / divergences

- **Version-delta check:** compare the full symbol set (`comm` over `%`-strings in
  `control*.c`), never infer "new" from `git diff` +/− lines — refactored lines read as added
  and mislead. 3.6b↔3.7 notification set is identical; `<…>` floats are the only wire delta.
- Correlation is **positional FIFO** (tmux serial), control replies (`flags != 0`) only; the
  parsed `number` is a monotonic `debug_assert` desync tripwire.
- Commit messages: **no backticks in `git commit -m`** (shell mangles them) — use `-F -` heredoc.
- Async drivers don't reuse `blocking`'s `Mutex<Shared>` (held across a write) — per-task actor.
- `SpawnOpts` is `#[non_exhaustive]` → external crates must use the builder, not a literal.
- 3.7 floating-pane `<…>` sections **deferred, not a gap**: out of the 3.6b target. See the
  target ADR.
