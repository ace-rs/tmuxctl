# decisions

Architecture Decision Records — dated, append-only, one decision per file. Each pins a
choice and the reasoning behind it so a later reader need not reconstruct it.

Name files `YYYY-MM-DD-short-slug.md`. Supersede rather than edit: a reversed decision gets
a new ADR that references the old one.

- [`2026-06-18-crate-name-license-and-shape.md`](2026-06-18-crate-name-license-and-shape.md)
  — crate name `tmuxctl`, dual MIT/Apache-2.0, standalone publishable crate. (Its "tokio
  async" call was superseded — see the sans-IO ADR below.)
- [`2026-06-18-sans-io-core-feature-gated-drivers.md`](2026-06-18-sans-io-core-feature-gated-drivers.md)
  — sans-IO core, no mandatory runtime; feature-gated `blocking`/`tokio`/`smol` drivers.
- [`2026-06-18-lock-step-tmux-and-robustness.md`](2026-06-18-lock-step-tmux-and-robustness.md)
  — one pinned tmux, produce strictly / accept liberally, no version matrix or gating.
- [`2026-06-18-container-test-strategy.md`](2026-06-18-container-test-strategy.md)
  — four-layer test pyramid; containerized real-tmux integration doubles as fixture generator.
- [`2026-06-21-target-tmux-3.6b-floats-out-of-scope.md`](2026-06-21-target-tmux-3.6b-floats-out-of-scope.md)
  — pin stable tmux `3.6b`; native floats deferred (composited client-side in hangar for now).
