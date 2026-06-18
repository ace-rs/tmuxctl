# spec

How `tmuxctl` works and what it must do. Reverse-specced against the tmux source and
iTerm2's control-mode client; reconcile divergences here rather than papering over them.

- [`overview.md`](overview.md) — **the protocol contract.** The keystone: transport and
  handshake, reply framing and correlation, pane-output escaping, the full notification
  set, layout strings, the Rust API sketch, testing and publishing. Read this first.

Backing the spec: [`../reference/tmux-source-map.md`](../reference/tmux-source-map.md) maps
each wire detail to the tmux C source that produces it.
