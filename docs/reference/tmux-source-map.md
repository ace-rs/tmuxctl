# tmux source map (control mode)

This is a navigation map of the tmux C source for engineers porting tmux's
**control-mode** protocol (`tmux -C`), the layout-string parser/checksum, and the
pane-output escaping into the `tmuxctl` Rust crate. It records the real function
names, exact format strings, and the verified algorithms — not a tutorial.

- **tmux clone:** `/Users/chakrit/Documents/chakrit/tmux`
- **Version (target):** `3.6b` — `TARGET_TMUX` = tag `8f3f14f5`, the pinned target (see
  [the target ADR](../decisions/2026-06-21-target-tmux-3.6b-floats-out-of-scope.md)). The line
  numbers below were captured against `next-3.7` (`3.7-rc-86-gc6b8ad6e`) and are **pending
  re-anchoring to 3.6b**; the algorithms and format strings hold across both — only line numbers
  and the `<…>` float section drift.
- **Spec cross-reference:** `docs/spec/overview.md` (the protocol spec this map feeds)

⚠️ Line numbers below are **hints, not contracts** — they drift across versions and
even across rc tags. Anchor on function/struct names and on the literal format
strings, which are stable, and re-grep if a line number misses.

Every format string here is quoted verbatim from the source. In C, `control_write`
takes a printf format, so a literal `%` is written `%%`. The strings below are shown
**as the C source writes them** (doubled `%%`); the byte actually emitted on the wire
has a single `%`. So `"%%output %%%u "` emits `%output %<id> `.


## 1. Control-mode emitter

The emitter has two layers: a generic line writer (`control.c`) and a set of
notification helpers (`control-notify.c`). Notifications are `%`-prefixed lines;
command replies are framed by `%begin/%end/%error` (see §2).

### control.c — line writer and flow control

| Function                       | Role                                                          |
| ------------------------------ | ------------------------------------------------------------- |
| `control_write(c, fmt, ...)`   | Queue/emit one notification line. printf-style.               |
| `control_vwrite(c, fmt, ap)`   | Immediate write path (used when no `%output` blocks pending). |
| `control_write_output(c, wp)`  | Enqueue a pane's new data as an output block.                 |
| `control_append_data(...)`     | Build the `%output` / `%extended-output` line + escaping.     |
| `control_write_pending(...)`   | Drain pane blocks into the write buffer under a byte limit.   |
| `control_start(c)`             | Set up bufferevents; emit the CONTROLCONTROL DCS opener.      |
| `control_pause_pane(c, wp)`    | Emit `%pause`.                                                |
| `control_continue_pane(c, wp)` | Emit `%continue`.                                             |

Key facts verified from source:

- `control_write` (control.c:412) is `void control_write(struct client *c, const
  char *fmt, ...)`. If the `all_blocks` queue is empty it writes immediately via
  `control_vwrite`; otherwise it stores the formatted line in a `control_block` so it
  stays ordered **behind** any pending `%output` (the block-ordering comment at
  control.c:29 explains why: an `%output` block holds up later notification lines
  until it is fully written, preserving stream order).
- `control_vwrite` (control.c:394) writes the line then a single `"\n"` —
  notifications are LF-terminated.
- The CONTROLCONTROL opener: `control_start` (control.c:771) writes the 7-byte DCS
  `"\033P1000p"` only when `CLIENT_CONTROLCONTROL` is set (i.e. `tmux -CC`). Plain
  `-C` does not emit it.

### control.c — `%output` / `%extended-output` and pause/continue

`control_append_data` (control.c:614) builds the line. The two format branches
(control.c:626–631):

```c
if (c->flags & CLIENT_CONTROL_PAUSEAFTER) {
        evbuffer_add_printf(message,
            "%%extended-output %%%u %llu : ", wp->id,
            (unsigned long long)age);
} else
        evbuffer_add_printf(message, "%%output %%%u ", wp->id);
```

So on the wire:

- normal: `%output %<pane-id> <escaped-bytes>`
- pause-after mode (`refresh-client -A …:pause` / `-f pause-after`):
  `%extended-output %<pane-id> <age-ms> : <escaped-bytes>`
  — note the literal ` : ` separator before the data and the `age` (ms behind) field.

`%pause` / `%continue` (control.c:389, 375): `"%%pause %%%u"` and `"%%continue
%%%u"` → `%pause %<pane-id>` / `%continue %<pane-id>`. `%pause` is also emitted
automatically by `control_check_age` (control.c:456) when a paused-mode client falls
`c->pause_age` ms behind.

### control-notify.c — notification helpers

`CONTROL_SHOULD_NOTIFY_CLIENT(c)` (control-notify.c:26) gates every notification:
client must be non-NULL, have `CLIENT_CONTROL`, and a live `control_state`. Most
helpers additionally require `c->session != NULL` and that the window is linked in
the client's session.

| Function (control-notify.c)             | Format string (C source)                                           | Wire output                                                  |
| --------------------------------------- | ------------------------------------------------------------------ | ----------------------------------------------------------- |
| `control_notify_pane_mode_changed`      | `"%%pane-mode-changed %%%u"`                                        | `%pane-mode-changed %<pane>`                                |
| `control_notify_window_layout_changed`  | template `"%layout-change #{window_id} #{window_layout} #{window_visible_layout} #{window_raw_flags}"` (format-expanded, **single `%`**) | `%layout-change @<win> <layout> <visible-layout> <flags>`   |
| `control_notify_window_pane_changed`    | `"%%window-pane-changed @%u %%%u"`                                  | `%window-pane-changed @<win> %<pane>`                       |
| `control_notify_window_unlinked`        | `"%%window-close @%u"` / `"%%unlinked-window-close @%u"`            | `%window-close @<win>` (linked) / `%unlinked-window-close`  |
| `control_notify_window_linked`          | `"%%window-add @%u"` / `"%%unlinked-window-add @%u"`                | `%window-add @<win>` / `%unlinked-window-add`               |
| `control_notify_window_renamed`         | `"%%window-renamed @%u %s"` / `"%%unlinked-window-renamed @%u %s"`  | `%window-renamed @<win> <name>` / `%unlinked-window-renamed`|
| `control_notify_client_session_changed` | `"%%session-changed $%u %s"` (self) / `"%%client-session-changed %s $%u %s"` (other) | `%session-changed $<sess> <name>` / `%client-session-changed <client> $<sess> <name>` |
| `control_notify_client_detached`        | `"%%client-detached %s"`                                            | `%client-detached <client>`                                 |
| `control_notify_session_renamed`        | `"%%session-renamed $%u %s"`                                        | `%session-renamed $<sess> <name>`                           |
| `control_notify_session_created`        | `"%%sessions-changed"`                                              | `%sessions-changed` (no args)                               |
| `control_notify_session_closed`         | `"%%sessions-changed"`                                              | `%sessions-changed` (no args)                               |
| `control_notify_session_window_changed` | `"%%session-window-changed $%u @%u"`                               | `%session-window-changed $<sess> @<win>`                    |
| `control_notify_paste_buffer_changed`   | `"%%paste-buffer-changed %s"`                                       | `%paste-buffer-changed <name>`                              |
| `control_notify_paste_buffer_deleted`   | `"%%paste-buffer-deleted %s"`                                       | `%paste-buffer-deleted <name>`                              |

Note: `%layout-change` carries **four** fields — `window_layout` (the full layout)
*and* `window_visible_layout` (what's actually shown, which differs when a pane is
zoomed) plus the raw window flags. Don't conflate the two layout fields.

### `%subscription-changed` (control.c, subscription machinery)

Emitted by the per-second subs timer (`control_check_subs_timer`, control.c:1042),
not by control-notify.c. Four shapes by subscription type:

- session: `"%%subscription-changed %s $%u - - - : %s"` (control.c:871) →
  `%subscription-changed <name> $<sess> - - - : <value>`
- pane / all-panes: `"%%subscription-changed %s $%u @%u %u %%%u : %s"`
  (control.c:918, 953) →
  `%subscription-changed <name> $<sess> @<win> <winidx> %<pane> : <value>`
- window / all-windows: `"%%subscription-changed %s $%u @%u %u - : %s"`
  (control.c:998, 1033) →
  `%subscription-changed <name> $<sess> @<win> <winidx> - : <value>`

The `-` placeholders fill unused id slots so the field count is constant per
subscription class. The ` : ` separates the fixed header from the format-expanded
value. Subscriptions are registered via `control_add_sub` / `control_remove_sub`
(control.c:1138, 1167), driven by `refresh-client -B` (§5).

### `%exit`

**Not** emitted by the server's control emitter. It is printed by the **client
process** in `client.c:423–427`:

```c
} else if (client_flags & CLIENT_CONTROL) {
        if (client_exitreason != CLIENT_EXIT_NONE)
                printf("%%exit %s\n", client_exit_message());
        else
                printf("%%exit\n");
```

So `%exit` (optionally `%exit <reason>`) is the last line, written by the local
`tmux -C` front-end on teardown, after `fflush`. For `-CC` it is followed by the DCS
terminator `"\033\\"` (client.c:438). A Rust client speaking the protocol directly
(no local `tmux -C` process) will not receive `%exit` from a remote tmux — it is a
client-side artifact. Treat its presence as "the front-end is exiting", and key your
own teardown off the transport closing.


## 2. Command queue / reply correlation (`%begin`/`%end`/`%error`)

Files: cmd-queue.c (numbering + guard), control.c (`control_error`).

- **Command number counter:** `static u_int number;` inside `cmdq_next`
  (cmd-queue.c:737). Each item gets `item->number = ++number;` (cmd-queue.c:770) when
  it fires. It is a **process-global monotonic counter**, not per-client and not
  reset — so numbers you see are shared across all queue items the server runs, and a
  control client only observes the subset routed to it. Do not assume a contiguous
  sequence on the wire.
- **Guard line:** `cmdq_guard(item, guard, flags)` (cmd-queue.c:825) emits, only when
  the client has `CLIENT_CONTROL`:

  ```c
  control_write(c, "%%%s %ld %u %d", guard, t, number, flags);
  ```

  → `%<guard> <time> <number> <flags>` where `<guard>` ∈ {`begin`,`end`,`error`},
  `<time>` is `item->time` (`time(NULL)`, unix seconds), `<number>` the command
  number, `<flags>` is `!!(state->flags & CMDQ_STATE_CONTROL)` (1 for commands that
  arrived over the control channel, 0 otherwise).

- **Framing:** `cmdq_fire_command` (cmd-queue.c:~595) wraps each command:
  `cmdq_guard(item, "begin", flags)` before `entry->exec(...)`, then on the result
  `cmdq_guard(item, "error", flags)` if `CMD_RETURN_ERROR` else `cmdq_guard(item,
  "end", flags)` (cmd-queue.c:619, 677, 679). Any command output printed in between
  lands between the `%begin` and `%end`/`%error` lines.

- **Parse errors** (malformed command line, before it even runs) take a separate
  path: `control_error` (control.c:527) emits a `begin`/`error` pair around a
  `parse error: <msg>` body:

  ```c
  cmdq_guard(item, "begin", 1);
  control_write(c, "parse error: %s", error);
  cmdq_guard(item, "error", 1);
  ```

Correlation rule for the Rust client: match a reply block by the `<number>` field on
the `%begin` line, accumulate body lines until the matching `%end`/`%error` with the
same number, and treat `%error` as command failure. The `<flags>` field distinguishes
replies to your own control-channel commands (1) from echoes of server-internal
commands (0).


## 3. Pane-output escaping

Location: `control_append_data` (control.c:614), the per-byte loop at
control.c:637–648:

```c
for (i = 0; i < size; i++) {
        if (new_data[i] < ' ' || new_data[i] == '\\') {
                evbuffer_add_printf(message, "\\%03o", new_data[i]);
        } else {
                start = i;
                while (i + 1 < size &&
                    new_data[i + 1] >= ' ' &&
                    new_data[i + 1] != '\\')
                        i++;
                evbuffer_add(message, new_data + start, i - start + 1);
        }
}
```

**Exact escaping rule** (confirmed): a byte is escaped iff `byte < 0x20` (i.e. `<
' '`, any control char including `\t`, `\n`, `\r`) **or** `byte == '\\'` (0x5C). The
escape is `\` followed by **exactly three octal digits**, zero-padded
(`"\\%03o"` → e.g. NUL → `\000`, newline → `\012`, backslash → `\134`). Everything
`>= 0x20` and `!= '\\'` is passed through **raw**, including bytes `>= 0x80` (UTF-8 is
emitted verbatim, not escaped). Note `0x7f` (DEL) is `>= ' '` so it is **not**
escaped. The `else` branch is just a run-length optimization that copies a maximal
run of pass-through bytes in one `evbuffer_add`; it has no effect on the output bytes.

The line is terminated by a single `"\n"` added in `control_write_data`
(control.c:662). The whole-line prefix (`%output %<id> ` or the extended form) is
added once when `message == NULL` (control.c:622), so a single logical output line can
accumulate multiple blocks before the trailing newline.


## 4. Layout strings

File: layout-custom.c (dump/parse/checksum), with the cell tree in layout.c and
`struct layout_cell` / `enum layout_type` in tmux.h.

### The checksum — `layout_checksum` (layout-custom.c:46)

```c
static u_short
layout_checksum(const char *layout)
{
        u_short csum;

        csum = 0;
        for (; *layout != '\0'; layout++) {
                csum = (csum >> 1) + ((csum & 1) << 15);
                csum += *layout;
        }
        return (csum);
}
```

Verified bit ops: 16-bit accumulator. Per char: **rotate right by 1** (low bit
wraps into bit 15: `(csum >> 1) + ((csum & 1) << 15)`), **then add the char's byte
value**, all modulo 2^16. The checksum is computed over the layout body **only** (the
part after the `csum,` prefix), and in `layout_dump` it is computed over the
already-assembled body string including any `<…>` floating-pane section.

### Dump — `layout_dump` / `layout_append` (layout-custom.c:60 / 90)

- Output format: `xasprintf(&out, "%04hx,%s", layout_checksum(layout), layout)`
  (layout-custom.c:85) → **`%04x,<body>`** — checksum is 4 lowercase hex digits,
  zero-padded, then a comma, then the body.
- Each cell (`layout_append`): `"%ux%u,%d,%d,%u"` when it has a pane
  (`sx`x`sy`,`xoff`,`yoff`,`pane-id`), else `"%ux%u,%d,%d"` (no pane id) for a node
  cell (layout-custom.c:103, 106). So a leaf is `WxH,x,y,<pane-id>` and an interior
  node is `WxH,x,y` followed immediately by its bracketed child list.
- Nesting brackets (layout-custom.c:96, 115–128): `const char *brackets =
  "][";` then for `LAYOUT_LEFTRIGHT` it is reassigned `brackets = "}{"`.
  `brackets[1]` is the opener, `brackets[0]` the closer:
  - `LAYOUT_LEFTRIGHT` → `{ … }` (children laid out left-to-right)
  - `LAYOUT_TOPBOTTOM` → `[ … ]` (children laid out top-to-bottom)
  Children are comma-joined; the trailing comma is overwritten with the closing
  bracket (`buf[strlen(buf) - 1] = brackets[0];`).
- Floating panes (3.7 addition — **deferred; we target 3.6b**, which has none; lands when the target bumps): appended after the tiled root inside `< … >`
  (layout-custom.c:71–84), comma-joined, trailing comma → `>`.

A typical two-pane horizontal split dumps as e.g.
`<csum>,158x40,0,0{79x40,0,0,0,78x40,80,0,1}` — outer node `158x40,0,0`, then `{` ...
two leaves `79x40,0,0,0` and `78x40,80,0,1` ... `}`.

### Parse — `layout_parse` → `layout_construct` (layout-custom.c:173 / 375)

- Header check (layout-custom.c:183): `sscanf(layout, "%hx,%n", &csum, &n)` and
  requires `n == 5` — i.e. the checksum prefix must be exactly 4 hex digits + comma.
  Then it recomputes `layout_checksum(layout + n)` and rejects on mismatch ("invalid
  layout").
- `layout_construct_cell` (layout-custom.c:321) parses one `%ux%u,%d,%d` cell
  (`sscanf(*layout, "%ux%u,%d,%d", &sx, &sy, &xoff, &yoff)`), then hand-advances past
  each digit run. The fiddly bit at 351–357: after the `yoff` it peeks for a `,<num>`
  that is a pane id — but if the char after that number is `x`, it's actually the
  start of the *next* cell's `WxH`, so it rewinds (`*layout = saved`) and does not
  consume it as a pane id.
- `layout_construct` (layout-custom.c:375) recurses: after a cell, a `{` sets
  `LAYOUT_LEFTRIGHT`, `[` sets `LAYOUT_TOPBOTTOM`; it loops over comma-separated
  children and requires the matching `}` / `]` closer. Terminators that end a cell
  without children: `,` `}` `]` `>` `\0` (layout-custom.c:386–391).
- `layout_check` (layout-custom.c:137) validates the geometry sums:
  left-right children must share `sy` and their `sx+1` sum to parent `sx+1`;
  top-bottom dual. **The `+1` is the pane border** — each split costs one row/column
  of border, which is why child sizes plus one accumulate to the parent size.

### Cell tree (layout.c, tmux.h)

```c
enum layout_type {            /* tmux.h:1483 */
        LAYOUT_LEFTRIGHT,
        LAYOUT_TOPBOTTOM,
        LAYOUT_WINDOWPANE
};

struct layout_cell {          /* tmux.h:1493 */
        enum layout_type type;
        int              flags;       /* LAYOUT_CELL_FLOATING 0x1 */
        struct layout_cell *parent;
        u_int            sx, sy;      /* size */
        int              xoff, yoff;  /* offset */
        struct window_pane *wp;       /* set iff LAYOUT_WINDOWPANE leaf */
        struct layout_cells cells;    /* children, TAILQ */
        TAILQ_ENTRY(layout_cell) entry;
};
```

`LAYOUT_WINDOWPANE` is a leaf (has `wp`, no children); the other two are interior
nodes (have `cells`, no `wp`). The dump emits the pane id only for leaves.


## 5. send-keys / refresh-client

### cmd-send-keys.c — `cmd_send_keys_entry` (line 32)

- Args spec: `.args = { "c:FHKlMN:Rt:X", 0, -1, NULL }`.
- `-l` (literal): `cmd_send_keys_inject_string` (line 131) — when set, skips
  `key_string_lookup_string`, treats the argument as literal UTF-8 text and injects
  each char as a key. Without `-l`, it first tries to parse the arg as a key **name**
  (e.g. `Enter`, `C-c`) and only falls back to literal if that fails.
- `-H` (hex): line 123 — parses the argument as a hex byte (`strtol(s, …, 16)`,
  must be `0..0xff`) and injects `KEYC_LITERAL | n` — i.e. a single raw byte by its
  numeric value. One arg = one byte.
- `-K` (send as key press to client): line 75 — instead of routing through the
  pane's input, constructs a `key_event` (`key | KEYC_SENT`) and feeds it via
  `server_client_handle_key` / `..._after`. Requires a target client (`tc`);
  no-ops if `tc == NULL`.
- Other relevant flags: `-N <repeat-count>` (line 183), `-R` reset pane input
  (line 230), `-X` send to the active mode's command (line 200), `-M` mouse
  (line 211), `-c <target-client>`.

### cmd-refresh-client.c — `cmd_refresh_client_entry` (line 33)

- Args spec: `.args = { "A:B:cC:Df:r:F:lLRSt:U", 0, 1, NULL }`.

| Flag | Handler (cmd-refresh-client.c)                | Purpose (control mode)                                                   |
| ---- | --------------------------------------------- | ------------------------------------------------------------------------ |
| `-C` | `cmd_refresh_client_control_client_size` (81) | Set client/window size. Accepts `@<win>:<W>x<H>`, `@<win>:` (clear), `<W>,<H>`, or `<W>x<H>`. Bounds `WINDOW_MINIMUM..WINDOW_MAXIMUM`. |
| `-A` | `cmd_refresh_client_update_offset` (133)      | Per-pane output control: value `%<pane>:on\|off\|continue\|pause`. Maps to `control_set_pane_on/off`, `control_continue_pane`, `control_pause_pane`. Control-clients only. |
| `-B` | `cmd_refresh_client_update_subscription` (46) | Add/remove a subscription: value `<name>:<what>:<format>`. `<what>` = `%*` (all panes), `%<n>`, `@*` (all windows), `@<n>`, else session. Empty (`<name>` only) removes. |
| `-f` / `-F` | `server_client_set_flags` (262–264)    | Set client flags (e.g. `pause-after`, `no-output`, `wait-exit`). `-F` is a documented alias for `-f` (line 261). |
| `-r` | `cmd_refresh_report` (166)                     | Pane colour report: `%<pane>:<report>` feeds `tty_keys_colours` to set `control_fg`/`control_bg`. |
| `-l` | line 256                                       | Clipboard query (`tty_clipboard_query`).                                 |
| `-c -L -R -U -D` | line 205                          | Pan the visible region (with optional numeric adjustment).               |
| `-S` | line 294                                       | Force status-line redraw.                                                |

Flow-control mapping for the crate: **pause-after** mode is entered by setting the
client flag (`refresh-client -f pause-after`, which yields `CLIENT_CONTROL_PAUSEAFTER`
and switches `%output` → `%extended-output`); **per-pane pause/continue** is driven by
`refresh-client -A %<pane>:pause` / `:continue`, which emit `%pause`/`%continue` and
reset the pane offset. Subscriptions are `refresh-client -B`. All of `-A`, `-B`, `-C`
error with `"not a control client"` if the target lacks `CLIENT_CONTROL`
(cmd-refresh-client.c:303).


## 6. ID sigils

Confirmed across the format strings above:

| Sigil    | Entity  | Source field           | Example emitters                                        |
| -------- | ------- | ---------------------- | ------------------------------------------------------- |
| `%<n>`   | pane    | `wp->id`               | `%output %%%u`, `%pane-mode-changed %%%u`, layout leaf  |
| `@<n>`   | window  | `w->id` / `window_id`  | `%window-add @%u`, `%window-pane-changed @%u`           |
| `$<n>`   | session | `s->id`                | `%session-changed $%u`, `%session-window-changed $%u`   |

These are the same sigils tmux accepts as command targets (`-t %3`, `-t @1`, `-t $0`)
and that `refresh-client` parses back in `-A`/`-B`/`-C` (e.g. `sscanf(copy, "%%%u",
&pane)` at cmd-refresh-client.c:147, `sscanf(size, "@%u:%ux%u", …)` at line 90). The
emit side formats them inline in the control format strings; there is no single
central "format an id" helper — each notification hardcodes the sigil.


## Porting notes / gotchas

- **`%%` doubling.** Every wire `%` is `%%` in the C printf format. When porting a
  format string, halve the leading `%%` and keep `%u`/`%s` as value substitutions.
  The one exception is the `%layout-change` template (control-notify.c:52), which is a
  **format-expand** template (`#{…}`) not a printf string, so its `%` is already
  single.
- **`%exit` is client-side.** It comes from `client.c`, the local `tmux -C` front-end,
  not the server emitter. A Rust client talking the protocol over its own transport
  won't get a server-sent `%exit`; drive teardown off the transport/`%end` of a
  detach, and off the empty-line behavior below.
- **Empty line detaches.** `control_read_callback` (control.c:567): an empty input
  line (`*line == '\0'`) sets `CLIENT_EXIT` and stops reading. So sending a bare
  newline to tmux's control channel detaches the client — do not send blank command
  lines.
- **Escaping edge cases.** Only `< 0x20` and `\\` are escaped; `0x7f` (DEL) and all
  `>= 0x80` bytes pass through raw. The escape is always **three** octal digits.
  Don't escape high bytes or DEL, and don't emit variable-width octal.
- **Layout border accounting.** The `+1` in `layout_check` is the pane border. When
  validating/round-tripping layouts, child `sx+1` (left-right) or `sy+1` (top-bottom)
  must sum to the parent dimension `+1`. The checksum is over the **body after the
  `%04x,` prefix**, recomputed and compared on parse with `n == 5` enforced
  (4 hex + comma).
- **Two layout fields.** `%layout-change` sends both `window_layout` and
  `window_visible_layout` — they diverge under zoom. Track both.
- **Version-gated / mode-gated notifications.** `%extended-output` vs `%output`,
  `%pause`/`%continue`, and `%subscription-changed` only appear when the corresponding
  client flag/subscription is enabled (`CLIENT_CONTROL_PAUSEAFTER`, an active
  `control_add_sub`). `%unlinked-window-*` variants fire only when the window is not
  in the client's own session. Floating-pane `< … >` layout sections are a 3.7
  addition — older servers won't emit them. The `-CC` DCS wrapper (`\033P1000p` open,
  `\033\\` close) is CONTROLCONTROL-only; plain `-C` omits it.
- **Command numbers are global and sparse.** `number` is a process-global counter
  shared by all queue items; a control client sees only its subset, so observed
  `%begin`/`%end` numbers are monotonic but **not contiguous**. Correlate strictly by
  the number on `%begin`, not by expecting `+1` steps.
- **`%begin/%end/%error` flags field.** The trailing `<flags>` is 1 for commands that
  arrived over the control channel and 0 for server-internal commands whose output
  still reaches the client. Use it to filter echoes from your own replies.
