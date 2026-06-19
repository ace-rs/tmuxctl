//! Live integration against a real tmux — the truth oracle for the parts a fake
//! transport can't reach (chiefly that commands we *emit* are accepted).
//!
//! `#[ignore]`d: these spawn tmux, so they run only via `scripts/integration.sh` or
//! `cargo test --features blocking --test integration -- --ignored`. Keyed off
//! `TMUXCTL_TMUX_BIN` (default `tmux`); each test isolates on its own server socket
//! and kills it on teardown. See `docs/decisions/2026-06-18-container-test-strategy.md`.

#![cfg(feature = "blocking")]

use std::process::Command;
use std::time::Duration;

use tmuxctl::{Client, Notification, SpawnOpts};

fn tmux_bin() -> String {
    std::env::var("TMUXCTL_TMUX_BIN").unwrap_or_else(|_| "tmux".to_string())
}

/// Unique per test so concurrent runs don't share a server.
fn opts(socket: &str) -> SpawnOpts {
    SpawnOpts::new()
        .program(tmux_bin())
        .socket(socket)
        .session("it")
}

/// Detaching the control client leaves the server running — tear it down explicitly.
fn kill_server(socket: &str) {
    let _ = Command::new(tmux_bin())
        .args(["-L", socket, "kill-server"])
        .status();
}

#[test]
#[ignore = "spawns real tmux; run via scripts/integration.sh"]
fn command_round_trip() {
    let socket = format!("tmuxctl-it-rt-{}", std::process::id());
    let client = Client::spawn(opts(&socket)).expect("spawn tmux -C");

    // A real command, parsed and answered by real tmux — exercises the write side.
    let windows = client
        .command("list-windows -F '#{window_id}'")
        .expect("list-windows succeeds");
    assert!(!windows.lines.is_empty(), "expected at least one window");
    assert!(
        windows.lines[0].starts_with('@'),
        "expected a window id, got {:?}",
        windows.lines
    );

    // A bogus command must surface as a CommandError, not a hang or a wrong Ok.
    assert!(
        client.command("this-is-not-a-tmux-command").is_err(),
        "a bogus command should be Err"
    );

    drop(client);
    kill_server(&socket);
}

#[test]
#[ignore = "spawns real tmux; run via scripts/integration.sh"]
fn new_window_emits_notification() {
    let socket = format!("tmuxctl-it-win-{}", std::process::id());
    let mut client = Client::spawn(opts(&socket)).expect("spawn tmux -C");
    let events = client.events().expect("events receiver");

    client.command("new-window").expect("new-window succeeds");

    // tmux announces the new window asynchronously; scan the events that arrive in a
    // bounded window (recv_timeout, so a quiet stream can't hang the test).
    let mut saw_window_add = false;
    while let Ok(notification) = events.recv_timeout(Duration::from_secs(2)) {
        if matches!(notification, Notification::WindowAdd(_)) {
            saw_window_add = true;
            break;
        }
    }
    assert!(saw_window_add, "expected a %window-add notification");

    drop(client);
    kill_server(&socket);
}
