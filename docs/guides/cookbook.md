# Cookbook & FAQ

Worked use-cases and common questions. See [getting-started.md](getting-started.md) for setup
and [tmux-concepts.md](tmux-concepts.md) for the model.

## Use-cases

### Mirror a session into your own UI

The headline use-case: attach to tmux, render its panes yourself. Spawn a control client,
drain notifications, feed `Output` bytes to a VT emulator, and re-render on layout changes.

```rust,ignore
let mut client = Client::spawn(SpawnOpts::new().session("work"))?;
let events = client.events().unwrap();
for note in events {
    match note {
        Notification::Output { pane, bytes } => screen(pane).feed(&bytes), // vt100/avt
        Notification::LayoutChange { window, layout, .. } => relayout(window, &layout),
        Notification::WindowAdd(w) | Notification::WindowClose(w) => refresh_tabs(),
        Notification::Exit(_) => break,
        _ => {}
    }
}
```

tmuxctl gives you decoded bytes and a typed layout tree; the VT emulator turns bytes into a
screen grid, and your renderer draws it. tmuxctl does neither of those.

### Drive tmux programmatically

Issue commands and read their replies — automation, scripting, orchestration:

```rust,ignore
client.command("new-window -n build")?;
client.command("send-keys -t build 'cargo build' Enter")?;          // or client.send_keys(..)
let panes = client.command("list-panes -F '#{pane_id} #{pane_active}'")?;
for line in panes.lines { /* … */ }
```

`command` blocks until the reply is correlated. A tmux error (`%error`) comes back as
`Err(CommandError::Failed { lines })`, so you can branch on failure without parsing stderr.

### Push a layout back

Parse, transform, and re-apply a layout (the checksum is regenerated for you):

```rust,ignore
let layout = Layout::parse("a1b2,80x24,0,0,0")?;
client.command(&format!("select-layout '{}'", layout.to_layout_string()))?;
```

### Custom transport or offline testing

The sans-IO `Engine` needs no process or runtime. Feed it bytes from anywhere — a recorded
transcript, a fake pipe, a non-std transport — and assert the event stream:

```rust,ignore
let mut engine = Engine::new();
let events = engine.feed(b"%window-add @1\n%output %0 hi\\012\n");
// → [Incoming::Notification(WindowAdd(@1)), Incoming::Notification(Output { %0, "hi\n" })]
```

This is exactly how tmuxctl's own transcript tests replay real captures, and how a driver for
an unsupported runtime would wrap the core.

## FAQ

**Does tmuxctl render anything or emulate a terminal?**
No. It is the protocol layer only — parse the stream, decode output to bytes, model layouts.
Turning bytes into a screen is a VT emulator's job (`vt100`, `avt`); drawing is a renderer's
(`ratatui`). This separation is the whole point of the crate.

**Why `-C` and not `-CC`?**
`-CC` emits a DSC wrapper for a real terminal to detect and disables canonical mode — meant
for running tmux *inside* another terminal. A programmatic host wants plain `-C`. tmuxctl
always uses `-C`.

**Which driver should I use?**
`Client` (the default `blocking` feature) unless you're on an async runtime — then prefer
`SmolClient` (lighter dependency tree), or `TokioClient` if you're already on tokio. All three
wrap the same core; the protocol behavior is identical, and the default build pulls no async
runtime.

**Is `%output` data UTF-8?**
No. Pane output is arbitrary bytes (a program can emit anything), so `Output.bytes` may not
be valid UTF-8. Never `String::from_utf8` it — hand it to a VT emulator, which handles
multi-byte sequences (including ones split across reads).

**How are command replies matched to commands?**
Positionally (FIFO): tmux runs its command queue serially, so replies arrive in send order
and tmuxctl matches each to the oldest outstanding command. Server-internal command output
(flags `0`) is skipped so it can't desync the queue.

**What happens if a command fails, or tmux exits mid-command?**
A tmux `%error` → `Err(CommandError::Failed { lines })`. A session that ends (pipe EOF) →
`Err(CommandError::Disconnected)`, and the events receiver closes — a blocked `command` can't
hang past teardown.

**Which tmux versions work?**
tmuxctl is lock-step to one pinned tmux: guaranteed against that version, best-effort on
others (unknown `%`-lines degrade to `Notification::Unknown`, never a panic). It does not
gate features by version. See the
[lock-step ADR](../decisions/2026-06-18-lock-step-tmux-and-robustness.md).

**Does tmuxctl spawn the shells/programs in panes?**
No — tmux does. You spawn and drive *tmux*; tmux owns the pane processes. tmuxctl observes
and commands.

**Can I use it without an async runtime, or with a runtime it doesn't support?**
Yes. The sans-IO `Engine` (and `Parser`, `Layout`, `decode_output`) is pure and runtime-free.
Wrap it with your own read/write loop — that's all the `blocking`/`smol`/`tokio` drivers are.

**Is the client `Send`/thread-safe?**
The blocking `Client` runs its reader on an internal thread and hands you a `Receiver` to
drain wherever you like; `command` is callable while events stream. The async drivers run an
owner task and are driven from their runtime.
