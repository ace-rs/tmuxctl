# `tmuxctl` — protocol contract

**Status:** Draft (canonical copy; supersedes hangar's `docs/spec/tmux-control-crate.md`,
which now points here).
**Kind:** Standalone, separately-publishable Rust crate (not part of any consumer binary).

A bidirectional **tmux control-mode client**: spawn `tmux -C`, parse the `%`-prefixed
notification stream, correlate command replies, decode pane output, and model tmux's layout
tree. Protocol layer **only** — no terminal emulation, no UI. A consumer (e.g. hangar)
drives it; so could anyone building a Rust tmux front-end.

Each wire detail below is backed by the tmux C source. See
[`../reference/tmux-source-map.md`](../reference/tmux-source-map.md) for the exact functions
and format strings (clone at `~/Documents/chakrit/tmux`, version next-3.7).

## Why a separate crate

- **Clean seam.** Protocol (this crate) vs. terminal emulation (`vt100`/`avt`, the consumer)
  vs. UI (ratatui, the consumer). Each independently testable and replaceable.
- **Nothing to adopt.** A 2026 survey found no reusable Rust control-mode client:
  `tmux_interface`'s `control_mode` module is "(unimplemented, draft)"; `tmuxpulse`/`tmuxcc`
  are single-purpose binaries, the best of them read-only, lacking reply correlation and
  output unescaping.
- **Decouples timelines.** A consumer's first phase may depend on this crate, but the crate
  depends on no consumer — developed and published in parallel.

## Scope

**In:**

- Spawn and supervise a `tmux -C` control session over separate stdin/stdout pipes.
- Line-framed parser → a typed `Notification` stream.
- Command send with **reply correlation** (`%begin`/`%end`/`%error` matched by
  command-number) returning per-command futures.
- **Octal-decode** `%output` / `%extended-output` payloads to raw bytes.
- Parse tmux **layout strings** into a typed tree; regenerate + checksum them.
- Typed helpers for input (`send-keys`), resize (`refresh-client -C`), and flow control,
  plus a raw-command escape hatch.
- tmux version detection; tolerate-and-log unknown `%`-lines.

**Out (the consumer's job):**

- Terminal/VT emulation of decoded pane bytes (`vt100`/`avt`).
- Rendering, layout *placement*, keybindings, any UI.
- Spawning the pane processes — **tmux** does that, not this crate, not the consumer.

## Transport & handshake

- Spawn **`tmux -C`** (control mode) with separate stdin/stdout pipes — **not `-CC`**. `-C`
  with pipes is the programmatic form; `-CC` additionally disables canonical mode and emits
  a `\033P1000p` DSC for a *real terminal* to detect, which a piped host doesn't want.
- The session **detaches on an empty input line** (control.c handles a bare newline as
  detach) — never emit a stray newline.
- Startup attaches or creates a session per the spawn command (e.g.
  `tmux -C new-session -A -s <name>` or `tmux -C attach`).

## Reply framing & correlation

Every command sent produces exactly one reply block:

```
%begin <timestamp> <command-number> <flags>
…output lines…
%end   <timestamp> <command-number> <flags>     (success)
%error <timestamp> <command-number> <flags>     (failure)
```

- `timestamp` = seconds since epoch; `command-number` = unique, monotonically increasing;
  `flags` currently always `1`. Emitted by `cmdq_guard` (cmd-queue.c).
- **Correlate by `command-number`.** Keep a FIFO of issued commands, each with a oneshot. On
  `%begin`, capture the number and buffer lines until the matching `%end`/`%error`, then
  resolve. Asynchronous `%`-notifications interleave freely between a command and its reply —
  only the numbering disambiguates. Never assume a reply immediately follows its command.
- **Gotcha:** the command-number counter is **process-global** in tmux (`static u_int number`
  in `cmdq_next`), so the numbers a single control client observes are monotonic but
  **sparse** — do not assume they start at 0 or increment by 1.

## Pane output & escaping

```
%output %<pane-id> <data>
```

In `<data>`, every byte `< 0x20` **and** the backslash are replaced by a 3-digit **octal
escape** `\ooo` (so `\` → `\134`). DEL (`0x7f`) and bytes `>= 0x80` pass through **raw**
(the escaping loop is `byte < 0x20 || byte == '\\'`). **Decode `\NNN` back to the raw byte
before handing bytes to a VT emulator** — skipping this corrupts the screen. Under flow
control the form is `%extended-output %<pane-id> <ms-behind> : <data>` (same escaping).

UTF-8: pane content is raw bytes and multi-byte sequences can straddle reads — accumulate
bytes and decode at the emulator, never at the line reader. (`decode_output` preserves
`>= 0x80` bytes verbatim for exactly this reason.)

## Notification set

| Notification                                                  | Meaning                                  |
|---------------------------------------------------------------|------------------------------------------|
| `%output %<pane> <data>`                                      | Pane output (octal-escaped).             |
| `%extended-output %<pane> <ms-behind> : <data>`              | Pane output under flow control.          |
| `%layout-change @<win> <layout> [<visible-layout> <flags>]`  | Window layout changed.                   |
| `%window-add @<win>`                                          | Window created in the attached session.  |
| `%window-close @<win>`                                        | Window closed.                           |
| `%window-renamed @<win> <name>`                              | Window renamed.                          |
| `%unlinked-window-add/-close/-renamed @<win>`                | Same, for windows in *other* sessions.   |
| `%window-pane-changed @<win> %<pane>`                        | Active pane of a window changed.         |
| `%pane-mode-changed %<pane>`                                 | Pane entered/left a mode (copy, etc.).   |
| `%session-changed $<sess> <name>`                            | Attached session changed.                |
| `%session-renamed` / `%session-window-changed`               | Session renamed / its active window.     |
| `%sessions-changed`                                          | A session was created or destroyed.      |
| `%client-session-changed <client> $<sess> <name>`           | A client's session changed.              |
| `%pause %<pane>` / `%continue %<pane>`                       | Flow-control pause / resume.             |
| `%subscription-changed <name> …`                            | A format subscription pushed a value.    |
| `%exit [<reason>]`                                          | Control session ending (optional reason).|
| _unknown_ `%…`                                              | Log and skip — forward-compat.           |

> **Gotcha:** `%exit` is emitted by the tmux **client** process (client.c), not the server's
> control emitter. A direct-protocol Rust client talking to a remote/server tmux may not
> receive it that way — detect session teardown from the pipe/EOF too.
>
> **Gotcha:** `%layout-change` carries *both* `window_layout` and `window_visible_layout`;
> the two diverge when a pane is zoomed. Track which one you act on.

## Layout strings

`%layout-change` and the `window_layout` format use: `CHECKSUM,WxH,x,y<tree>`

- `CHECKSUM` — 4 hex digits over everything after the leading `CHECKSUM,`. Algorithm
  (`layout_checksum`, layout-custom.c): for each char `c`,
  `csum = (csum >> 1) + ((csum & 1) << 15); csum += c;`. Recompute when *generating* a layout
  to push via `select-layout`. Implemented as `layout::checksum`.
- **Leaf** (pane): `WxH,x,y,<pane-id>`.
- **Container**: `WxH,x,y` followed by children in `{…}` for a **left-right (horizontal)**
  split or `[…]` for a **top-bottom (vertical)** split; children comma-separated. A border
  consumes one row/column between children (the `+1` accounting in `layout_check`).
- Example: `bb62,159x48,0,0{79x48,0,0,79x48,80,0}` → a 159×48 window split into two
  side-by-side 79-wide panes.

## Input, resize, flow control

- **Keys:** `send-keys -t %<pane> …`. `-l` sends literal UTF-8 (no key-name lookup); `-H`
  takes hex ASCII byte values; `-K` sends key presses to the client. Use `-l`/`-H` to inject
  raw bytes/control sequences safely.
- **Resize:** `refresh-client -C <wxh>` for the control client's own size, or
  `refresh-client -C @<win>:<wxh>` to set a specific window's size for this client.
- **Flow control:** enable with `refresh-client -f pause-after=<seconds>` (pane emits
  `%pause` once that far behind); resume with `refresh-client -A '%<pane>:continue'`.
- **Subscriptions:** `refresh-client -B <name>:<type>:<format>` → `%subscription-changed`
  pushes — a polling-free way to watch arbitrary tmux formats.

## API sketch (Rust)

Async on **tokio**; minimal dependency tree (tokio, `bytes`; hand-rolled line parser over a
framework, to keep compile time and footprint small). Names indicative, not final; the
in-tree types (`PaneId`, `Notification`, `Layout`) already follow this shape.

```rust
pub struct PaneId(pub u32);     // %<n>
pub struct WindowId(pub u32);   // @<n>
pub struct SessionId(pub u32);  // $<n>

pub struct Client { /* child process + writer + pending-command queue */ }

impl Client {
    pub async fn spawn(args: SpawnOpts) -> Result<Client>;
    pub fn events(&self) -> impl Stream<Item = Notification>;
    pub async fn command(&self, cmd: &str) -> Result<CommandOutput, CommandError>;
    // typed helpers over `command`:
    pub async fn send_keys_literal(&self, pane: PaneId, bytes: &[u8]) -> Result<()>;
    pub async fn resize(&self, win: WindowId, cols: u16, rows: u16) -> Result<()>;
    pub async fn detach(self) -> Result<()>;   // empty-line teardown
}

pub fn decode_output(escaped: &str) -> Vec<u8>;   // \ooo octal → raw bytes
```

## Implementation guidance

- **Port the protocol from iTerm2** — `TmuxGateway` (the command queue + `%begin/%end`
  correlation + octal decode), `TmuxController`, `TmuxLayoutParser`. The only complete,
  maintained `-CC` client in existence. Cross-check against the
  [tmux Control-Mode wiki](https://github.com/tmux/tmux/wiki/Control-Mode) and the source map.
- Use `tmuxpulse`'s `src/mux/tmux/control.rs` as a Rust-idiom head-start for the async line
  reader → event enum, but **add** the reply correlation and output unescaping it omits.
  `robber-m/C-Tmux-Control-Mode` is a compact secondary reference.
- **Version-gate.** Detect the spawned tmux version; flow control
  (`%pause`/`%continue`/`%extended-output`), `%pane-mode-changed`, `%subscription-changed`,
  and the extra `%layout-change` args are newer (tmux ≥ 2.x–3.x). Tolerate and log unknown
  `%`-lines.

## Testing

- **Transcript record/replay:** capture real `tmux -C` sessions, replay the bytes through the
  parser, assert the `Notification` stream. The primary regression net — pairs well with
  golden-file snapshotting (the `smoke` skill).
- **Layout round-trip:** `parse → render → checksum` must equal tmux's own output.
- **Output unescape units:** control bytes, the backslash case (`\134`), DEL/high bytes
  passing through, and UTF-8 sequences split across chunk boundaries.
- **Live integration:** a local `scripts/` test spawns real tmux, creates a window, splits
  it, sends keys, and asserts the resulting events + layout. (No GitHub Actions — house
  convention; integration runs locally.)

## Publishing

- Crate name **`tmuxctl`** (confirmed available on crates.io). Semver from `0.x`.
- Dual MIT/Apache-2.0. See
  [`../decisions/2026-06-18-crate-name-license-and-shape.md`](../decisions/2026-06-18-crate-name-license-and-shape.md).
- A consumer depends on it by path during co-development, then by version once published.
  Release via a local script, not CI.

## Open questions

- Whether to publish before or alongside the first consumer release.
- Runtime-agnostic core (expose `AsyncRead`/`AsyncWrite`) vs. tokio-only.
- How much of tmux's command surface to type vs. leaving the raw escape hatch primary.
- Whether to expose the format/subscription system as first-class or leave it to raw
  `refresh-client -B`.
- Reconnect/resilience: behavior when the tmux server dies vs. when the control session is
  merely detached.
