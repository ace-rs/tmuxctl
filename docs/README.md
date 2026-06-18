# tmuxctl docs

Durable documentation for the `tmuxctl` crate, split by purpose.

Start at [`roadmap.md`](roadmap.md) for the sequencing from here to a published crate.

Split by purpose:

**Usage docs** (sorted by type):

- [`guides/`](guides/) — task-oriented how-tos (added as the API stabilizes).
- [`reference/`](reference/) — exhaustive lookups. Currently the
  [tmux source map](reference/tmux-source-map.md).

**Design record** (sorted by permanence):

- [`spec/`](spec/) — how it works and what it must do. Start at
  [`spec/overview.md`](spec/overview.md), the protocol contract.
- [`decisions/`](decisions/) — dated ADRs; pinned choices with rationale.
- [`notes/`](notes/) — working notes, investigations, scratch. The default home for
  anything not yet permanent.

When in doubt where a doc goes, default to `notes/`.
