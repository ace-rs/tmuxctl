# Getting started

`tmuxctl` is a **protocol-layer** tmux control-mode (`tmux -C`) client: it spawns a control
session, parses tmux's `%`-notification stream into typed events, correlates command replies,
decodes pane output to raw bytes, and models the layout tree. It does **not** emulate a
terminal or render anything — that's your job (pair it with a VT emulator like `vt100`/`avt`
and a renderer like `ratatui`).

New to tmux control mode? Read [tmux-concepts.md](tmux-concepts.md) first; it maps tmux's
model onto the types used below.

## Install

```sh
cargo add tmuxctl
```

Runtime support is feature-gated. The default is `blocking` (std threads, no extra deps):

| Feature    | Driver        | Pulls    | When                                       |
|------------|---------------|----------|--------------------------------------------|
| `blocking` | `Client`      | —        | default; threads, simplest                 |
| `smol`     | `SmolClient`  | `smol`   | async — **preferred** (lighter deps)       |
| `tokio`    | `TokioClient` | `tokio`  | async — you're already on a tokio runtime  |

```sh
cargo add tmuxctl --no-default-features --features smol
```

The pure protocol core (parser, correlation engine, layout, decode) is always available with
zero runtime dependency — see [Sans-IO core](#sans-io-core).

## Basic usage (blocking)

Spawn a control session, react to notifications on one thread, and issue commands from
another:

```rust
use tmuxctl::{Client, Notification, PaneId, SpawnOpts};

let mut client = Client::spawn(SpawnOpts::new().session("work"))?;

// Notifications arrive asynchronously; drain them on their own thread.
let events = client.events().unwrap(); // returns the Receiver once
std::thread::spawn(move || {
    for note in events {
        match note {
            Notification::Output { pane, bytes } => {
                // `bytes` is already octal-decoded — feed it to a VT emulator
            }
            Notification::LayoutChange { window, layout, .. } => { /* re-render */ }
            _ => {}
        }
    }
});

// Commands block until tmux's reply is correlated back.
let windows = client.command("list-windows -F '#{window_id}'")?;
client.send_keys(PaneId(0), b"echo hi\r")?;
client.resize(120, 40)?;
# Ok::<(), tmuxctl::CommandError>(())
```

`command` returns `Result<CommandOutput, CommandError>`: `Ok` carries the reply's output
lines, `Err(CommandError::Failed { lines })` is a tmux `%error`, and
`Err(CommandError::Disconnected)` means the session ended. The typed helpers (`send_keys`,
`resize`) are thin wrappers over `command` — for anything else, send the raw command string.

## Async drivers

`SmolClient` and `TokioClient` mirror the blocking API with `async fn` and an async-channel
event receiver. **`smol` is the recommended async driver** — a lighter dependency tree, in
keeping with tmuxctl's minimal footprint; reach for `TokioClient` when you're already on a tokio
runtime. Create and use them inside their runtime:

```rust,ignore
use tmuxctl::{SpawnOpts, SmolClient};

let mut client = SmolClient::spawn(SpawnOpts::new().session("work")).await?;
let mut events = client.events().unwrap(); // an async-channel Receiver
let out = client.command("list-windows").await?;
```

Internally each async driver runs one *owner task* that owns the engine and transport and
serializes command writes against the read loop — so no lock is held across an `.await`.

## Sans-IO core

The drivers are thin shells over a pure, runtime-free core. Drive it yourself when you have a
custom transport, want to test without a process, or replay a captured session:

```rust
use tmuxctl::{Engine, Incoming};

let mut engine = Engine::new();
for incoming in engine.feed(&raw_tmux_bytes) {
    match incoming {
        Incoming::Notification(note) => { /* async event */ }
        Incoming::Reply { id, result } => { /* a registered command completed */ }
    }
}
```

`Engine::feed(&[u8])` frames the raw byte stream on newlines (buffering partial lines across
calls) and returns correlated outcomes. Register outgoing commands with
`Engine::register_command` to correlate their replies; call `Engine::on_eof` when the stream
ends to fail any outstanding commands.

## A few essentials

- **Output is raw bytes, not text.** `Notification::Output.bytes` can contain any byte
  (including invalid UTF-8); decode it at the VT emulator, never with `String::from_utf8`.
- **`-C`, never `-CC`.** tmuxctl always uses plain control mode; the `-CC` terminal wrapper is
  not what a programmatic host wants.
- **tmux spawns the pane processes**, not tmuxctl and not you. You drive and observe; tmux
  runs the shells.

Next: [tmux-concepts.md](tmux-concepts.md) for the model, [cookbook.md](cookbook.md) for
worked examples and an FAQ, [reference.md](reference.md) for the protocol spec and API docs.
