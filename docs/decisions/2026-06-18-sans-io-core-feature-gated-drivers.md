# Sans-IO core, feature-gated runtime drivers

**Date:** 2026-06-18
**Status:** Accepted (chakrit; relayed via hangar over ace-connect)

## Context

The `Client` must spawn `tmux -C`, read/write its pipes, and correlate command replies. The
spec left two questions open: **tokio-only vs. a runtime-agnostic core**, and whether to take
the **tokio dependency** at all. Welding an async runtime into the protocol layer would force
that runtime on every consumer and couple the parser to an executor it doesn't need — async
buys nothing here, the workload is one process and two pipes, not a concurrency problem.

## Decision

**Sans-IO core, no runtime.** The protocol layer is pure and synchronous: the line `Parser`,
the reply-correlation state machine, `Layout`, and `decode_output` do no I/O and own no
executor. They take bytes/lines in and surface events out. This is the
[sans-IO](https://sans-io.readthedocs.io/) pattern (cf. `quinn-proto`, `h2`, rustls's
`ConnectionCommon`).

**Runtime support is feature-gated drivers.** Each driver owns the process spawn, the read
loop, and the writer, and pumps bytes through the sans-IO core:

- `blocking` — a reader-thread driver. **hangar uses this.**
- `tokio` — for tokio consumers.
- `smol` — for the smol ecosystem.

Each driver pulls its runtime as an **optional dependency behind its own feature**. The core
compiles with no runtime and (today) no dependency beyond `thiserror`.

## Rationale (chakrit)

`async != perf`. A sans-IO core stays runtime-free and reusable across ecosystems; consumers
pick their runtime — or none. The protocol logic is written and tested once; only the thin
I/O shell varies per runtime.

## Consequences

- **No mandatory async runtime.** No tokio in the default build; no dependency-approval gate
  to proceed (the original blocker is moot).
- **Reply correlation lives in the core as a pure state machine** — register a command,
  match `%begin`…`%end`/`%error` back by command-number, resolve. It is driven by feeding it
  parser events; it never blocks or awaits. Drivers layer the ergonomics on top: `blocking`
  exposes `command() -> Result<…>`; `tokio`/`smol` expose `async fn command()`.
- **Directly testable without a runtime.** Correlation and framing get unit-tested by
  feeding canned bytes and reading events — the test pyramid's transport-injection layer
  needs no process and no executor.
- **Supersedes** the spec's "Async on **tokio**" API sketch and resolves its open question
  "runtime-agnostic core vs. tokio-only" — the answer is sans-IO core + multi-runtime
  drivers. `docs/spec/overview.md` to be updated to match.
- Default features TBD; `blocking` is the natural default (std threads only, no third-party
  runtime). Confirm the public `Client` shape against hangar's crate spec before building it.

## Open questions

- Exact public surface of the sans-IO correlation core (what the drivers wrap) — aligning
  with hangar's crate spec before implementation.
- Whether the three drivers ship from day one or `blocking` first, `tokio`/`smol` to follow.
