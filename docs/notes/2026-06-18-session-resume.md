# Session resume — through 2026-06-21

Breadcrumb for the next `/ace`. hangar-driven over `ace-connect`, autonomous slice loop
(`/ace-afk` for unattended runs). Crate is published (v0.1.0) and feature-complete for the
protocol layer; recent work (2026-06-21 afk) landed the **`select-layout` push helper** and the
**`TARGET_TMUX` pin constant**, alongside heavy **hangar protocol support** over the bridge.

## Where it stands

Crate `tmuxctl`, repo `ace-rs/tmuxctl` (public). **v0.1.0 live on crates.io** (tag `v0.1.0` +
GitHub release). `gh/main` pushed through `5995543` (select-layout + its roadmap). **2 unpushed
commits** ahead: `52f1f67` (`TARGET_TMUX` const + crate-status refresh) and `fe8556f` (its
roadmap) — push next. **60 default+feature lib tests** + 2 `#[ignore]`d integration + 1
transcript, clippy `--all-features` + fmt + `cargo doc` all clean. Deps: `thiserror` (always) +
optional `tokio`/`smol`. Cargo.toml still `0.1.0`.

**Release stance:** 2 features accumulated since v0.1.0 (`select_layout`, `TARGET_TMUX`). Per
chakrit's pre-v1 policy (new feature = patch bump), the next release is **0.1.1** — *held* to
avoid spinning versions on two small features; cut it when more accumulates or hangar requests
(hangar consumes tmuxctl as a sibling repo, so no urgent crates.io need). Flow: `cargo
set-version 0.1.1` → commit → tag `v0.1.1` → `scripts/release.sh`.

**Pinned target: tmux `3.6b` (`8f3f14f5`)** = `TARGET_TMUX` (decision only; not yet a code
constant). See [the target ADR](../decisions/2026-06-21-target-tmux-3.6b-floats-out-of-scope.md).

- **Sans-IO core (pure, no runtime):** id newtypes; `decode_output(&[u8])`; `Layout`
  parse/render/checksum; line `Parser` (`&[u8]`; reply framing + control flag; full
  `#[non_exhaustive]` `Notification` set; dropped-`%end` recovery); correlation `Engine`
  (`feed(&[u8])`, `on_eof()`, positional-FIFO + monotonic-number tripwire). `WindowFlags`,
  `CommandOutput`, `CommandError` (`Failed`|`Disconnected`).
- **Three drivers, one core, feature-gated:** `Client` (`blocking`, default), `SmolClient`
  (`smol`), `TokioClient` (`tokio`) — async via per-task actor. Each: `spawn(SpawnOpts)` /
  `command` / `send_keys` / `resize` / `select_layout(WindowId, &Layout)` / events `Receiver` /
  teardown. Shared `spawn` + `commands`. Crate-root `TARGET_TMUX`/`TARGET_TMUX_COMMIT` consts.
- **Test pyramid (Phase 5, mostly done):** units; transcript replay of a real 3.6b capture
  (`tests/fixtures/structural-session.txt`, asserts no-`Unknown`); injected-transport driver
  tests; host real-tmux integration (`tests/integration.rs` + `scripts/integration.sh`,
  `#[ignore]`d, `TMUXCTL_TMUX_BIN`). Gap: the pinned-tmux **container** (off-host repro).
- **Docs:** README (smol-preferred); guides in `docs/guides/`; ADRs (crate-name, sans-IO,
  lock-step, container, **target-3.6b**); `scripts/release.sh`.

## This session (2026-06-21, afk + hangar)

Prior same-day session (3.6b pin + docs re-grounding) is captured in the target ADR and the
roadmap — not repeated here. This run was `/ace-afk` with chakrit's explicit grant to **commit,
push, and cut release** (pre-v1 versioning: new feature = patch, breaking = minor; don't spin).

- **Slice 1 — `select_layout` (Phase 3 layout push):** `commands::select_layout(WindowId,
  &Layout)` → `select-layout -t @<w> <checksummed-string>`, exposed on all three drivers
  (`7ecaf63`). Sends `to_layout_string()` (checksummed) form — `layout_parse` (layout-custom.c)
  **requires** the 4-hex checksum. No client-side validation: tmux arbitrates via `%error`.
  Loosened the `fake_tmux_expecting` test helper to take an owned `String`.
- **Slice 2 — `TARGET_TMUX` const + crate-status refresh** (`52f1f67`): `pub const TARGET_TMUX
  = "3.6b"` / `TARGET_TMUX_COMMIT = "8f3f14f5"` at the crate root; fixed the stale `# Status`
  doc (claimed "Early"/"async Client next slice" though all drivers shipped + published).
- **hangar protocol support over the bridge (priority):** answered 3 question-sets, all
  source-verified, full detail in `/tmp/*-answer-tmuxctl.md` + folded into hangar's own docs:
  (1) `%output` bytes are raw PTY post-octal-decode, feed `vt100::process` direct; (2) an escape
  seq **can split across consecutive `%output`** (tmux chunks by a byte budget, floor 32B —
  control.c:706) → one persistent parser per pane, never reset; (3) control-mode sizing — a
  control client is **ignored for sizing until it sends `refresh-client -C`** (resize.c:69, else
  `default-size` 80x24); `%layout-change` is the resize feedback (only on real change);
  `window-size manual` ignores `-C`; per-window form is `-C @<w>:WxH`.
- **Audit (2 slices):** A — `select_layout` uses `WindowId` Display (`@2`) while `send_keys`
  hardcodes `%{}`+`.0`; optional fix-slice = migrate `send_keys`/`resize` to Display (both
  valid, low priority). B — each command helper is triplicated across the 3 drivers; at ~6+
  helpers consider codegen, but the sync/async split + "don't DRY across modules" argue against
  it now. No violations.

## Next

1. **Push** `52f1f67` + `fe8556f` to `gh/main` (afk grant covers it; held only pending this
   save). Then consider **release 0.1.1** if the batch feels worth it (bump → tag → release.sh).
2. **Phase 5 container — the main remaining unblocked planned work; pick up fresh** (this save
   fired because context was heavy — don't start it bloated). Dockerfile building tmux **3.6b**
   (`8f3f14f5`) for off-host integration repro, plus the fixture-generator loop. `docker build`
   is borderline-but-OK under the afk envelope (local, reversible); writing the Dockerfile is
   plainly fine.
3. **Deferred until a consumer needs them** (Phase 3 defer-helpers stance): per-window resize
   (`refresh-client -C @<w>:WxH` — exact form verified this session), flow control
   (`pause-after` / `%p:continue`), `Client::tmux_version()` telemetry (`display-message -p
   '#{version}'`).
4. **Blocked — source-map line-number re-anchor** `next-3.7` → 3.6b: needs the 3.6b source. The
   local clone `~/Documents/chakrit/tmux` is at `3.7-rc-86`; read 3.6b blobs via `git show
   8f3f14f5:control.c` etc. — **do not `checkout`** chakrit's clone (outside-tree mutation).
   Algorithms/format strings hold; only line numbers + the `<…>` float section drift.
5. **Optional fix-slice (audit nit):** migrate `send_keys`/`resize` to `PaneId` Display for
   sigil consistency with `select_layout`. Low priority; both forms valid.

## Re-establish the bridge

Slug **`ace-rs.tmuxctl.claude`**, autonomous mode. Standing grant (CLAUDE.md): hangar-requested
push + cargo release proceed without per-action approval (gates-green + sane-version gated).
Workflow: CLAUDE.md + [`../guides/slice-loop.md`](../guides/slice-loop.md).

## Notes / divergences

- **Verify-loop gotcha (cost ~20 min this session):** `cargo` under the `lowfat` hook buffers
  output until the command *exits* — the log looks empty mid-run, so don't read it as "hung."
  A piped `cargo … | tail`/`| head` reports the **pipe's** exit code, not cargo's — misleading.
  Run the gate to a file with a sentinel: `cargo … > /tmp/x 2>&1; echo "exit=$?" >> /tmp/x`,
  then read the file. **Never** spawn concurrent `cargo` invocations — they contend on the
  build-dir lock and serialize/stall. And a mismatched `fake_tmux_expecting` **hangs** the
  blocking test forever (the fake thread panics before writing `%end`, so `client` blocks on the
  reply) — when a driver test hangs, suspect a wire-string mismatch, read the assertion diff.
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
