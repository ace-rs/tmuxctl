# tmux concepts, mapped to tmuxctl

This explains the slice of tmux that control mode exposes, and how each piece appears in
tmuxctl. For the exact wire formats see [the protocol spec](../spec/overview.md); for the
tmux C source behind them see [the source map](../reference/tmux-source-map.md).

## Control mode (`tmux -C`)

tmux normally draws a UI to a terminal. **Control mode** instead makes tmux speak a
line-based text protocol over stdin/stdout: you send commands, tmux streams back
`%`-prefixed notifications and `%begin`/`%end` reply blocks. That's the entire surface
tmuxctl speaks — it spawns `tmux -C` and translates the stream into Rust types.

tmuxctl always uses `-C`, never `-CC`. `-CC` additionally emits a DSC wrapper meant for a
*real terminal* to detect; a programmatic host doesn't want it.

## The object model → ids

A tmux **server** owns **sessions**; a session has **windows**; a window is a tree of
**panes**. Each gets a server-unique id with a sigil, which tmuxctl wraps in a newtype:

| tmux entity | wire sigil | tmuxctl type | example     |
|-------------|------------|--------------|-------------|
| session     | `$`        | `SessionId`  | `$0`        |
| window      | `@`        | `WindowId`   | `@3`        |
| pane        | `%`        | `PaneId`     | `%2`        |

These ids are stable for the entity's lifetime and are how every command and notification
refers to things. (Note `%` is also the notification prefix — context disambiguates.)

## Notifications → `Notification`

Everything that happens asynchronously — output, structural changes, session switches —
arrives as a `%`-line, parsed into a `Notification`. The set tmuxctl models:

| Wire line                                  | `Notification` variant          |
|--------------------------------------------|---------------------------------|
| `%output %p <data>`                        | `Output { pane, bytes }`        |
| `%extended-output %p <ms> : <data>`        | `ExtendedOutput { pane, ms_behind, bytes }` |
| `%layout-change @w <layout> <vis> <flags>` | `LayoutChange { window, layout, visible_layout, flags }` |
| `%window-add` / `-close` / `-renamed`      | `WindowAdd` / `WindowClose` / `WindowRenamed` |
| `%unlinked-window-add` / `-close` / `-renamed` | `UnlinkedWindow*` (windows in other sessions) |
| `%window-pane-changed @w %p`               | `WindowPaneChanged { window, pane }` |
| `%session-changed` / `%session-renamed`    | `SessionChanged` / `SessionRenamed` |
| `%session-window-changed $s @w`            | `SessionWindowChanged { session, window }` |
| `%client-session-changed <c> $s <name>`    | `ClientSessionChanged { client, session, name }` |
| `%sessions-changed`                        | `SessionsChanged`               |
| `%pane-mode-changed %p`                    | `PaneModeChanged`               |
| `%pause %p` / `%continue %p`               | `Pause` / `Continue`            |
| `%subscription-changed …`                  | `SubscriptionChanged { name, value }` |
| `%exit [reason]`                           | `Exit`                          |
| anything else                              | `Unknown(String)` (logged, never fatal) |

`Notification` is `#[non_exhaustive]`: tmux's surface grows across versions, so always keep a
catch-all arm.

## Commands and replies → `command`

You send a command as a line; tmux frames its reply:

```
%begin <ts> <number> <flags>
…output lines…
%end   <ts> <number> <flags>     (success)   →  Ok(CommandOutput { lines })
%error <ts> <number> <flags>     (failure)   →  Err(CommandError::Failed { lines })
```

`Client::command` (and the async equivalents) blocks until the matching reply arrives and
returns it. **Correlation is positional (FIFO):** tmux runs its command queue serially, so
reply blocks come back in send order; tmuxctl matches the next *control* reply to the oldest
outstanding command. The `flags` field distinguishes replies to *your* commands (`1`) from
server-internal command output echoed to you (`0`) — the latter never consumes your queue.
The command-number is a monotonic sanity check, not the correlation key.

## Pane output and escaping → `Notification::Output`

`%output` carries a pane's new bytes, with control bytes (`< 0x20`) and backslash
octal-escaped (`\ooo`) and everything else — including bytes `>= 0x80` — passed through raw.
tmuxctl decodes the escapes for you, so `Output.bytes` is the **raw byte stream** the pane
produced. It is *not* guaranteed UTF-8; hand it straight to a VT emulator, which is the thing
that understands cursor moves, colors, and multi-byte sequences. Under flow control the same
data arrives as `ExtendedOutput` with a "milliseconds behind" measure.

## Layouts → `Layout`

A window's pane arrangement is a tree, serialized as `checksum,WxH,x,y<tree>`. tmuxctl parses
it into `Layout`:

- `Layout::Leaf` — a single pane (`WxH,x,y,pane-id`).
- `Layout::SplitH` — a left-right split (`{…}`), children side by side.
- `Layout::SplitV` — a top-bottom split (`[…]`), children stacked.

`Layout::parse` verifies the checksum; `render` + `to_layout_string` regenerate it (with a
fresh checksum) so you can push a layout back via `select-layout`. `%layout-change` also
carries a `visible_layout` (what's shown — diverges from `layout` under zoom) and
`WindowFlags` (current/zoomed/bell/etc.). A border consumes one row/column between children.

## Flow control and subscriptions

For high-output panes, `refresh-client -f pause-after=<secs>` switches `%output` to
`%extended-output` and emits `%pause`/`%continue` (`Pause`/`Continue`) as a pane falls behind
and recovers. `refresh-client -B` registers format **subscriptions** that push
`%subscription-changed` (`SubscriptionChanged`) — a poll-free way to watch arbitrary tmux
formats.

## Versioning

tmuxctl is **lock-step** to one pinned tmux: it produces commands strictly for that version
and accepts the stream liberally (unknown lines → `Unknown`, never fatal). It does not gate
behavior by version — tmux's own client/server protocol check is the compatibility arbiter.
See [the lock-step ADR](../decisions/2026-06-18-lock-step-tmux-and-robustness.md).
