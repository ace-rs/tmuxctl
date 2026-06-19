//! Transcript replay — the primary regression net (container test-strategy ADR).
//!
//! `tests/fixtures/structural-session.txt` is a real `tmux -C` capture (tmux 3.6b:
//! new-session → new-window → split-window → rename-window → kill-window → detach).
//! Replaying its raw bytes through `Engine::feed` is fully deterministic, so we
//! assert the exact notification stream. Regenerate the fixture (in the pinned-tmux
//! container) on a tmux bump; a `smoke`-style diff then flags any wire drift.
//!
//! The reply blocks have no registered commands here, so they resolve to nothing —
//! this net validates async-notification parsing + framing, not reply correlation
//! (which the engine unit tests and the live integration tests cover).

use tmuxctl::{Engine, Incoming, Notification, PaneId, WindowId};

fn notifications(bytes: &[u8]) -> Vec<Notification> {
    let mut engine = Engine::new();
    engine
        .feed(bytes)
        .into_iter()
        .filter_map(|incoming| match incoming {
            Incoming::Notification(notification) => Some(notification),
            Incoming::Reply { .. } => None,
        })
        .collect()
}

#[test]
fn structural_session_replays_to_expected_notifications() {
    let fixture = include_bytes!("fixtures/structural-session.txt");
    let notes = notifications(fixture);

    // The headline regression: every line real tmux 3.6b emitted is modeled — none
    // fell through to `Unknown`. This is what pins our Phase-0 coverage to reality.
    let unknown: Vec<_> = notes
        .iter()
        .filter(|n| matches!(n, Notification::Unknown(_)))
        .collect();
    assert!(unknown.is_empty(), "unparsed lines: {unknown:?}");

    // The expanded notification set, exercised by real structural operations.
    let has = |want: &Notification| notes.iter().any(|n| n == want);
    assert!(has(&Notification::WindowAdd(WindowId(0))));
    assert!(has(&Notification::SessionsChanged));
    assert!(has(&Notification::SessionChanged(
        tmuxctl::SessionId(0),
        "cap".to_string()
    )));
    assert!(has(&Notification::UnlinkedWindowAdd(WindowId(1))));
    assert!(has(&Notification::WindowPaneChanged {
        window: WindowId(1),
        pane: PaneId(2),
    }));
    assert!(has(&Notification::UnlinkedWindowRenamed(
        WindowId(1),
        "fixture".to_string()
    )));
    assert!(has(&Notification::UnlinkedWindowClose(WindowId(1))));
    assert!(has(&Notification::Exit(None)));

    // The auto-rename line is cwd-dependent, so only its shape is pinned (the
    // no-Unknown check above already proved it parses).
    assert!(
        notes
            .iter()
            .any(|n| matches!(n, Notification::WindowRenamed(WindowId(0), _))),
        "expected an initial %window-renamed for @0"
    );

    // Deterministic count for this fixture (6 reply blocks yield no notifications).
    assert_eq!(notes.len(), 11, "stream drifted: {notes:?}");
}
