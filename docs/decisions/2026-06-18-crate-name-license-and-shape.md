# Crate name, license, and shape

**Date:** 2026-06-18
**Status:** Accepted

## Context

This repo (`tmux-rs`) hosts the standalone tmux control-mode client specced in hangar's
`docs/spec/tmux-control-crate.md`. Onboarding into ACE forced three identity decisions that
ripple through `Cargo.toml`, every doc, and the eventual crates.io listing.

## Decisions

**Crate name: `tmuxctl`.** Chosen over the spec's draft `tmux-control` and the alternatives
`tmux-cc` / `tmuxc`. Reads like `systemctl` — "tmux control" — short at the callsite,
unambiguous, and self-explanatory. All candidates were free on crates.io at decision time;
`tmuxctl` confirmed available. The repo keeps the name `tmux-rs`; the crate publishes as
`tmuxctl`.

**License: dual MIT OR Apache-2.0.** The broader Rust ecosystem norm; the Apache half adds
an explicit patent grant that a standalone, publishable crate benefits from. (The sibling
`ace` crate is MIT-only, but it is an application binary, not a library others link.)

**Shape: standalone, separately-publishable library.** Protocol layer only — no terminal
emulation, no UI. Async on **tokio** with a minimal dependency tree (hand-rolled line
parser over a framework, to keep compile time and footprint small). Developed independently
of hangar's timeline; hangar depends on it by path during co-development, then by version
once published. The crate never depends on hangar.

## Consequences

- `Cargo.toml`: `name = "tmuxctl"`, `license = "MIT OR Apache-2.0"`, two LICENSE files.
- Semver from `0.x`; release via a local `scripts/` flow, not CI (house convention bans
  GitHub Actions).
- Open question deferred: whether to publish before or alongside hangar's first release.

## Open questions (from the spec, still live)

- Runtime-agnostic core (expose `AsyncRead`/`AsyncWrite`) vs. tokio-only.
- How much of tmux's command surface to type vs. leaving the raw escape hatch primary.
- Whether to expose the format/subscription system as first-class.
- Reconnect/resilience when the tmux server dies vs. a merely-detached control session.
