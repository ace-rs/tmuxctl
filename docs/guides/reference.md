# Reference

Pointers for going deeper — the API, the protocol contract, the tmux source behind it, and
the design record.

## API

- **docs.rs** — [`docs.rs/tmuxctl`](https://docs.rs/tmuxctl) — the rendered API for the
  published release. Or build it locally with all drivers:

  ```sh
  cargo doc --all-features --open
  ```

The public surface: `Client` / `SmolClient` / `TokioClient` (drivers), `SpawnOpts`, the
sans-IO `Engine` + `Incoming`, `Parser` + `Event` + `Reply`, `Notification` + `WindowFlags`,
`Layout`, `PaneId`/`WindowId`/`SessionId`, `CommandOutput`/`CommandError`, `decode_output`,
and `Error`/`Result`.

## Protocol contract

- [`../spec/overview.md`](../spec/overview.md) — the canonical protocol contract: transport
  and handshake, reply framing and correlation, output escaping, the full notification set,
  layout strings, the API sketch, and the testing/publishing plans. Read this before any
  non-trivial protocol work.
- [`../reference/tmux-source-map.md`](../reference/tmux-source-map.md) — a navigable map of
  the tmux C source (control.c, control-notify.c, layout-custom.c, cmd-queue.c) keyed to each
  wire detail: exact format strings, the layout checksum algorithm, the escaping loop.

## Design record (ADRs)

- [Crate name, license, and shape](../decisions/2026-06-18-crate-name-license-and-shape.md)
- [Sans-IO core + feature-gated drivers](../decisions/2026-06-18-sans-io-core-feature-gated-drivers.md)
- [Lock-step tmux versioning + robustness](../decisions/2026-06-18-lock-step-tmux-and-robustness.md)
- [Container test strategy](../decisions/2026-06-18-container-test-strategy.md)
- [`../roadmap.md`](../roadmap.md) — what's done and what's next.

## External

- [tmux Control-Mode wiki](https://github.com/tmux/tmux/wiki/Control-Mode) — the upstream
  description of the protocol.
- `man tmux` — the command reference (everything you can pass to `Client::command`).
- iTerm2's `TmuxGateway` / `TmuxController` / `TmuxLayoutParser` — the most complete existing
  control-mode client; tmuxctl's correlation and decoding port from the same ideas.

## Companion crates (the consumer's side)

tmuxctl is protocol-only; a full front-end pairs it with:

- a **VT emulator** — `vt100` or `avt` — to turn `Notification::Output` bytes into a screen
  grid.
- a **renderer** — e.g. `ratatui` — to draw that grid and the layout tree.
- [hangar](https://github.com/ace-rs/hangar) is the reference consumer (a local-first
  terminal multiplexer built on tmuxctl).
