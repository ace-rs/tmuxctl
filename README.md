# tmuxctl

A bidirectional tmux **control-mode** (`tmux -C`) client for Rust: spawn a control
session, parse the `%`-prefixed notification stream into typed events, correlate command
replies by command-number, octal-decode pane output to raw bytes, and model tmux's layout
tree.

It is a **protocol layer only** — no terminal emulation, no rendering, no UI. Those belong
to the consumer (a VT emulator like `vt100`/`avt`, a renderer like `ratatui`). The crate
exists because a 2026 survey found no reusable Rust control-mode client with reply
correlation and output unescaping; `tmuxctl` fills that gap on its own release cadence.

> **Status: pre-implementation.** The wire vocabulary (`PaneId`/`WindowId`/`SessionId`,
> `Notification`, `Layout`) and the fully-specified pure helpers (`decode_output`,
> `layout::checksum`) are in place and tested. The async `Client` and the line parser are
> the next slices.

## Scope

**In:** spawn/supervise `tmux -C`; line-framed parser → typed `Notification` stream;
command send with `%begin`/`%end`/`%error` reply correlation; octal-decode of
`%output`/`%extended-output`; layout-string parse + render + checksum; typed helpers for
`send-keys`, `refresh-client` resize and flow control; version detection; tolerate-and-log
unknown `%`-lines.

**Out (the consumer's job):** VT emulation of decoded bytes, rendering and layout
*placement*, keybindings, spawning pane processes (tmux does that), anything app-specific.

## Documentation

- [`docs/spec/overview.md`](docs/spec/overview.md) — the protocol contract: transport,
  reply framing, output escaping, the notification set, layout strings, the API sketch.
- [`docs/reference/tmux-source-map.md`](docs/reference/tmux-source-map.md) — a navigable
  map of the tmux C source (control mode, layout, escaping) that backs each wire detail.
- [`docs/decisions/`](docs/decisions/) — dated ADRs (crate name, license, async stack).

## Development

```sh
cargo test                                       # unit + integration
cargo clippy --all-targets --all-features        # done-gate, must be clean
cargo fmt
```

Warnings are errors (`#![deny(warnings)]`); clippy is a separate done-gate. No CI service —
release and integration scripts live under `scripts/` and run locally.

## Relationship to hangar

`tmuxctl` is developed standalone but consumed by [hangar](https://github.com/ace-rs/hangar),
a local-first terminal multiplexer that puts tmux's server in the engine role. hangar
depends on this crate by path during co-development, then by version once published. The
crate has no dependency on hangar.

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your
option.
