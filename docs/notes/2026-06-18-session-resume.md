# Session resume — through 2026-06-24

Breadcrumb for the next `/ace`. hangar-driven over `ace-connect`, autonomous slice loop
(`/ace-afk` for unattended runs). Crate is published (v0.1.0) and feature-complete for the
protocol layer. Latest work (2026-06-24) landed **per-window `resize_window`** (hangar ASK) and
a **middle-layer consistency audit** (route `send_keys` through `PaneId` Display), and settled
the **typed-command-surface strategy** (demand-driven, not blanket — see roadmap Phase 3).

## Where it stands

Crate `tmuxctl`, repo `ace-rs/tmuxctl` (public). **v0.1.0 live on crates.io** (tag `v0.1.0` +
GitHub release). `gh/main` @ **`5c7f09d`**; **local `main` is 3 commits ahead and UNPUSHED**
(`a5b1f77`, `f627ebe`, `11853c8` — this session; no push grant was invoked). **64 default lib
tests** + 2 `#[ignore]`d integration + 1 transcript; clippy `--all-features` + fmt + `cargo doc`
clean. Deps: `thiserror` (always) + optional `tokio`/`smol`. **Cargo.toml still `0.1.0`.**

**Release stance:** **2 features** accumulated since v0.1.0 (`select_layout`, `resize_window`).
Per chakrit's pre-v1 policy (new feature = patch bump), next release is **0.1.1** — *held*
until worth it or on hangar request (hangar consumes tmuxctl as a sibling repo, no urgent
crates.io need). Flow: `cargo set-version 0.1.1` → commit → tag `v0.1.1` → `scripts/release.sh`.

**Pinned target: tmux `3.6b`** — identified by version + the immutable `3.6b` release tag,
**never a commit SHA**. See [the target ADR](../decisions/2026-06-21-target-tmux-3.6b-floats-out-of-scope.md).

- **Sans-IO core (pure, no runtime):** id newtypes (sigil-aware `Display`); `decode_output`;
  `Layout` parse/render/checksum; line `Parser` (`&[u8]`; reply framing + control flag; full
  `#[non_exhaustive]` `Notification` set; dropped-`%end` recovery); correlation `Engine`
  (`feed`/`on_eof`, positional-FIFO + monotonic-number tripwire). `WindowFlags`,
  `CommandOutput`, `CommandError` (`Failed`|`Disconnected`).
- **Three drivers, one core, feature-gated:** `Client` (`blocking`, default), `SmolClient`
  (`smol`), `TokioClient` (`tokio`) — async via per-task actor. Each exposes the identical
  helper set: `command` / `send_keys` / `resize` / **`resize_window`** / `select_layout` /
  events `Receiver` / `spawn` / teardown. Shared `spawn.rs` + `commands.rs`. Parity verified
  this session.
- **Test pyramid (Phase 5, mostly done):** units; transcript replay of a real 3.6b capture
  (`tests/fixtures/structural-session.txt`, asserts no-`Unknown`); injected-transport driver
  tests; host real-tmux integration (`#[ignore]`d, `TMUXCTL_TMUX_BIN`). Gap: pinned-tmux
  **container** (off-host repro).
- **Docs:** README; guides in `docs/guides/`; ADRs; `docs/spec/overview.md` (the contract);
  `docs/reference/tmux-source-map.md`; `scripts/release.sh`.

## This session (2026-06-24, hangar-driven)

- **Slice — `resize_window` (Phase 3, hangar ASK):** `commands::resize_window(WindowId, cols,
  rows)` → `refresh-client -C @<w>:<cols>x<rows>` on all three drivers (`a5b1f77`; roadmap SHA
  stamp `f627ebe`). The `@%u:%ux%u` form at **cmd-refresh-client.c:90** (3.6b); layers over the
  global `resize`; bounds (`WINDOW_MINIMUM..=WINDOW_MAXIMUM`) are tmux's call → `%error` (parity
  with `resize`, no client-side check). Tests: `commands` unit + blocking-driver integration;
  async drivers kept their one-representative (`send_keys`) helper-test convention.
- **Design Q — "should we support *all* tmux commands?":** settled **no, demand-driven**. Raw
  `command(&str)` is the exhaustive surface; typed helpers are curated sugar for hot commands,
  added per consumer request; a wrapper buys ergonomics not validation. Now firmed in roadmap
  Phase 3 ("Settled 2026-06-24").
- **Middle-layer audit (typed primitives + command builders):**
  - **Fix (`11853c8`):** `send_keys` was the lone builder hardcoding `%{}`+`.0`; now renders via
    `{pane}` `Display`, so the `%` sigil has one render home (`ids.rs`). Output byte-identical;
    existing test unchanged. (Resolves the long-standing audit nit — note: `resize` carries no
    id, so only `send_keys` needed migrating.)
  - **Doc guard (`11853c8`):** `layout.rs` write_tree now comments why the trailing pane id
    must stay bare `.0` — layout strings carry **no** sigil, so `{pane}` Display (`%N`) would
    corrupt them. Guards a future "consistency" sweep (which this audit nearly tripped on).
  - **Left as-is (rationale in commit):** parse-side sigils (`pane_id`/`window_id`/`session_id`
    in `parser.rs`) — three co-located one-liners over immutable tmux constants; centralizing
    via a `const SIGIL` is churn in the most-tested code for a modest single-source win.
  - **Clean, no action:** driver parity; notification typing (every id field newtyped, `client`
    correctly `String`).
- **hangar exchange:** ACK'd `resize_window` ("exactly the ask"). Reset form (`-C @<w>:` no-size
  → clears the override, **cmd-refresh-client.c:105**) **NOT built** — hangar's YAGNI call
  (window-arranger not built yet; they'll ping when they need to drop overrides). hangar will
  bump its Hangar tmuxctl pin from `fca5801` when wiring.

## Next

1. **Push** — local `main` is 3 commits ahead of `gh/main` (`5c7f09d`), unpushed. Push on
   chakrit's say-so or a hangar request (standing grant covers hangar-requested push + release).
2. **Release 0.1.1?** — 2 features since v0.1.0 (`select_layout`, `resize_window`). Cut when
   worth it / on hangar request.
3. **Phase 5 container — main remaining unblocked planned work; pick up fresh** (not in a
   bloated context). Dockerfile building tmux **3.6b** from the release tag/tarball (not a SHA)
   for off-host integration repro, plus the fixture-generator loop.
4. **Source-map line-number re-anchor** `next-3.7` → 3.6b: the local clone
   `~/Documents/chakrit/tmux` is checked out at the **`3.6b` tag** (verify `git -C … describe
   --tags` → `3.6b`). Re-anchor source-map line numbers; algorithms/format strings already hold,
   only line numbers + the `<…>` float section drift. (Restore with `git checkout master` there
   when chakrit wants it back.)
5. **Deferred until a consumer needs them:** flow control (`pause-after` / `%p:continue`),
   `tmux_version()` telemetry (`display-message -p '#{version}'`), and the **`resize_window`
   reset form** (`-C @<w>:` no-size, src:105 — hangar will ping).

## Re-establish the bridge

Slug **`ace-rs.tmuxctl.claude`**, autonomous mode. Standing grant (CLAUDE.md): hangar-requested
push + cargo release proceed without per-action approval (gates-green + sane-version gated).
Workflow: CLAUDE.md + [`../guides/slice-loop.md`](../guides/slice-loop.md).

## Notes / divergences

- **Commit messages under the `lowfat` hook:** backticks in `git commit -m` get mangled **and**
  `-F -` heredoc-over-stdin gets **swallowed** (empty-message abort, hit twice this session).
  Write the message to a temp file and `git commit -F /tmp/msg.txt`.
- **Verify-loop gotcha:** `cargo` under `lowfat` buffers output until exit (empty log mid-run ≠
  hung); a piped `cargo … | tail` reports the *pipe's* exit, not cargo's. Run the gate to a file
  with a sentinel: `cargo … > /tmp/x 2>&1; echo "exit=$?" >> /tmp/x`, then read it. **Never**
  spawn concurrent `cargo` (build-dir lock contention/stall). A mismatched `fake_tmux_expecting`
  **hangs** the blocking test forever (fake thread panics before writing `%end`) — when a driver
  test hangs, suspect a wire-string mismatch and read the assertion diff.
- **Version-delta check:** compare the full symbol set (`comm` over `%`-strings in `control*.c`),
  never infer "new" from `git diff` +/− lines. 3.6b↔3.7 notification set identical; `<…>` floats
  are the only wire delta (deferred, out of 3.6b target).
- **Id newtype `Display` is the command/notification wire form** (`%N`/`@N`/`$N`) — but layout
  strings encode pane ids **bare** (no sigil), so `layout.rs` correctly uses `.0` there. The
  parser inverts the sigil per-id-type in `parser.rs` (str path + a `pane_id_bytes` byte path for
  the non-UTF8 `%output` payload).
- Correlation is **positional FIFO** (tmux serial), control replies (`flags != 0`) only; parsed
  `number` is a monotonic `debug_assert` desync tripwire.
- Async drivers don't reuse `blocking`'s `Mutex<Shared>` (held across a write) — per-task actor.
- `SpawnOpts` is `#[non_exhaustive]` → external crates must use the builder, not a literal.
