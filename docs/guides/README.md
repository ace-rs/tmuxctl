# guides

How-tos for **using** the crate:

- [getting-started.md](getting-started.md) — install, the driver/feature choice, basic
  blocking + async + sans-IO usage. **Start here.**
- [tmux-concepts.md](tmux-concepts.md) — tmux's control-mode model (sessions/windows/panes,
  notifications, commands, layouts) mapped onto tmuxctl's types.
- [cookbook.md](cookbook.md) — worked use-cases (mirror a session, drive tmux, push a layout,
  test offline) and an FAQ.
- [reference.md](reference.md) — pointers: API docs, the protocol spec, the tmux source map,
  the ADRs, and companion crates.

How-to for **developing** the crate:

- [slice-loop.md](slice-loop.md) — the standing autonomous workflow: the 2–3-slice +
  two-phase-audit cadence, the per-slice anatomy, and the blocker/release rules.
