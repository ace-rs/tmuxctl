# Session resume — 2026-06-18

Breadcrumb for the next `/ace`. Repo onboarded into ACE and first slices landed,
hangar-driven over `ace-connect`.

## Where it stands

Crate `tmuxctl` (repo `ace-rs/tmuxctl`, renamed from `tmux-rs`). Three commits on `main`:

1. `bb743fe` — scaffold: crate skeleton, docs tree, dual MIT/Apache-2.0, ACE config.
2. `8ed591a` — `Layout::parse`/`render`/`to_layout_string` + checksum round-trip.
3. `ef3a45b` — sync line `Parser` (framing + `%begin`/`%end`/`%error` reply blocks).

21 tests green, clippy + fmt clean, build ~0.3s. Public API is locked and was sent to
hangar (snapshot was an ephemeral `/tmp` file; the source of truth is the
re-exports in `src/lib.rs`).

## Next task (the immediate one)

**Async `Client`** — spawn `tmux -C` (NOT `-CC`) over separate stdin/stdout pipes, drive
`parser::Parser` over the stdout lines, and correlate `Event::Reply { number }` back to
issuing commands via a FIFO of oneshots. Typed helpers (`send_keys_literal`, `resize`,
`detach`) over a raw `command()`.

**Blocked on a decision (chakrit's):** the Client needs **tokio**, which is a dependency
add — deliberately not done autonomously. This also forces the spec's open question:
**runtime-agnostic core (expose `AsyncRead`/`AsyncWrite`) vs. tokio-only.** Resolve that
before writing the Client. Get chakrit's go-ahead on the dep.

## On resume — re-establish the bridge

The directory is now `tmuxctl`, so the deterministic ace-connect slug is
**`ace-rs.tmuxctl.claude`** (was `ace-rs.tmux-rs.claude`). Re-bind the listener under this
slug in **autonomous mode**, then `send.sh` hangar (`ace-rs.hangar.claude`) a `CTX` that the
slug is live so it can re-predict the peer.

## Notes / divergences worth remembering

- `Notification::Pause(PaneId)` / `Continue(PaneId)` carry a pane id — corrected from the
  spec's API sketch (which had `Continue` payload-less) against the verified wire
  `%pause %<pane>` / `%continue %<pane>`. Not yet version-gated; gate when the Client adds
  tmux-version detection.
- Layout leaves carry **bare** pane numbers (no `%`); the spec's `bb62` example omits pane
  ids, so it does not round-trip through `Layout::parse` — use ids in test fixtures.
- Primary regression net going forward is transcript record/replay (capture real `tmux -C`,
  replay bytes, assert the `Event` stream). Pairs with the `smoke` skill.
- `Cargo.toml` `repository` is now `github.com/ace-rs/tmuxctl`; the `gh` remote is wired and
  `main` is pushed.
