#!/usr/bin/env bash
# Run the live tmux integration tests (tests/integration.rs) against a tmux binary.
#
# Per docs/decisions/2026-06-18-container-test-strategy.md: integration is keyed off
# TMUXCTL_TMUX_BIN and #[ignore]d so the default `cargo test` stays pure and fast.
# No CI / GitHub Actions — run this locally (or inside the pinned-tmux container).
set -euo pipefail

tmux_bin="${TMUXCTL_TMUX_BIN:-tmux}"

if ! command -v "$tmux_bin" >/dev/null 2>&1; then
	echo "integration: tmux binary '$tmux_bin' not found (set TMUXCTL_TMUX_BIN)" >&2
	exit 1
fi

echo "integration: tmux = $tmux_bin ($("$tmux_bin" -V))"

TMUXCTL_TMUX_BIN="$tmux_bin" \
	cargo test --features blocking --test integration -- --ignored --nocapture "$@"
