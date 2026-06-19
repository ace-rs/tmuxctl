# tmuxctl

A bidirectional tmux **control-mode** (`tmux -C`) client for Rust: spawn a control
session, parse the `%`-prefixed notification stream into typed events, correlate command
replies by command-number, octal-decode pane output to raw bytes, and model tmux's layout
tree.

It is a **protocol layer only** — no terminal emulation, no rendering, no UI. Those belong
to the consumer (a VT emulator like `vt100`/`avt`, a renderer like `ratatui`). The crate
exists because a 2026 survey found no reusable Rust control-mode client with reply
correlation and output unescaping; `tmuxctl` fills that gap on its own release cadence.

> **Status: working, pre-1.0.** The sans-IO core (line `Parser`, correlation `Engine`,
> `Layout`, `decode_output`, the full `Notification` set) and a usable `Client` are in place,
> with three runtime drivers (`blocking`, `tokio`, `smol`) over the one core. Tested by unit
> tests, transcript replay of a real `tmux -C` capture, and live integration against tmux.
> Not yet published to crates.io.

## Scope

**In:** spawn/supervise `tmux -C`; line-framed parser → typed `Notification` stream;
command send with `%begin`/`%end`/`%error` reply correlation; octal-decode of
`%output`/`%extended-output`; layout-string parse + render + checksum; typed helpers for
`send-keys` and `refresh-client` resize; tolerate-and-log unknown `%`-lines. Versioning is
**lock-step** to one pinned tmux (no per-version gating) — see the ADRs.

**Out (the consumer's job):** VT emulation of decoded bytes, rendering and layout
*placement*, keybindings, spawning pane processes (tmux does that), anything app-specific.

## Usage

Three runtime drivers wrap one sans-IO core; pick by Cargo feature.

| Feature    | Driver         | Notes                                            |
|------------|----------------|--------------------------------------------------|
| `blocking` | `Client`       | std threads, no extra deps. **Default.**         |
| `tokio`    | `TokioClient`  | async; opt-in (pulls `tokio`).                   |
| `smol`     | `SmolClient`   | async; opt-in (pulls `smol`).                    |

Blocking client — spawn a control session, run a command, react to notifications:

```rust
use tmuxctl::{Client, Notification, PaneId, SpawnOpts};

let mut client = Client::spawn(SpawnOpts::new().session("work"))?;

// Notifications stream on their own channel; drain on a thread.
let events = client.events().unwrap();
std::thread::spawn(move || {
    for note in events {
        if let Notification::Output { pane, bytes } = note {
            // feed `bytes` (already octal-decoded) to your VT emulator
        }
    }
});

// Commands block until tmux's reply is correlated.
let windows = client.command("list-windows -F '#{window_id}'")?;
client.send_keys(PaneId(0), b"echo hi\r")?;
client.resize(120, 40)?;
# Ok::<(), tmuxctl::CommandError>(())
```

The async drivers mirror this with `async fn` and an `mpsc`/`channel` events receiver.

Sans-IO core — drive the protocol yourself over any transport (this is what the drivers
wrap, and how the parser is tested without a process):

```rust
use tmuxctl::{Engine, Incoming};

let mut engine = Engine::new();
for incoming in engine.feed(&raw_tmux_bytes) {
    match incoming {
        Incoming::Notification(note) => { /* async event */ }
        Incoming::Reply { id, result } => { /* a command you registered completed */ }
    }
}
```

## Documentation

- [`docs/spec/overview.md`](docs/spec/overview.md) — the protocol contract: transport,
  reply framing, output escaping, the notification set, layout strings, the API sketch.
- [`docs/reference/tmux-source-map.md`](docs/reference/tmux-source-map.md) — a navigable
  map of the tmux C source (control mode, layout, escaping) that backs each wire detail.
- [`docs/decisions/`](docs/decisions/) — dated ADRs (crate name/license, sans-IO core +
  feature-gated drivers, lock-step tmux versioning, the container test strategy).
- [`docs/roadmap.md`](docs/roadmap.md) — what's done and what's next.

## Development

```sh
cargo test                                       # units + transcript replay (fast, no tmux)
cargo test --all-features                         # also the tokio + smol drivers
cargo clippy --all-targets --all-features        # done-gate, must be clean
cargo fmt
./scripts/integration.sh                          # live tmux round-trip (#[ignore]d otherwise)
```

Warnings are errors (`#![deny(warnings)]`); clippy is a separate done-gate. No CI service —
release and integration scripts live under `scripts/` and run locally. The live integration
suite is keyed off `TMUXCTL_TMUX_BIN`.

## Relationship to hangar

`tmuxctl` is developed standalone but consumed by [hangar](https://github.com/ace-rs/hangar),
a local-first terminal multiplexer that puts tmux's server in the engine role. hangar
depends on this crate by path during co-development, then by version once published. The
crate has no dependency on hangar.

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your
option.
