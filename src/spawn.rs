//! Shared spawn configuration for the runtime drivers (not the sans-IO core).
//!
//! Both the `blocking` and `tokio` drivers build the same `tmux -C` command line;
//! the argv lives here so neither duplicates it.

/// How to spawn the `tmux -C` control client. `Default` runs `tmux -C new-session -A`
/// (attach-or-create the default session) on the default server.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SpawnOpts {
    /// The tmux binary to run.
    pub program: String,
    /// Server socket name for `-L` (isolation); `None` uses tmux's default server.
    pub socket: Option<String>,
    /// Session for `-s` (with `new-session -A`, attach-or-create); `None` leaves it unnamed.
    pub session: Option<String>,
}

impl Default for SpawnOpts {
    fn default() -> Self {
        Self {
            program: "tmux".to_string(),
            socket: None,
            session: None,
        }
    }
}

impl SpawnOpts {
    /// Start from the defaults (`tmux -C new-session -A`, default server). `SpawnOpts`
    /// is `#[non_exhaustive]`, so build it through these setters rather than a literal.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the tmux binary to run.
    pub fn program(mut self, program: impl Into<String>) -> Self {
        self.program = program.into();
        self
    }

    /// Run on a dedicated server socket (`-L`) — use this for test isolation.
    pub fn socket(mut self, socket: impl Into<String>) -> Self {
        self.socket = Some(socket.into());
        self
    }

    /// Attach-or-create this named session (`-s`, with `new-session -A`).
    pub fn session(mut self, session: impl Into<String>) -> Self {
        self.session = Some(session.into());
        self
    }

    /// The argv after the program name: `[-L <socket>] -C new-session -A [-s <session>]`.
    /// Control mode (`-C`), never `-CC`.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        if let Some(socket) = &self.socket {
            argv.push("-L".to_string());
            argv.push(socket.clone());
        }
        argv.push("-C".to_string());
        argv.push("new-session".to_string());
        argv.push("-A".to_string());
        if let Some(session) = &self.session {
            argv.push("-s".to_string());
            argv.push(session.clone());
        }
        argv
    }
}
