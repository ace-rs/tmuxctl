#!/usr/bin/env bash
# Cut a tmuxctl release: gate → tag → GitHub release → publish to crates.io.
#
# Local only — no CI / GitHub Actions. Run from a clean tree on the commit to ship.
# Dry run by default (packages + gates, publishes nothing); pass --execute to actually
# tag, push, and publish. Per CLAUDE.md's standing grant this may run on hangar's request
# (gates green + version sane); otherwise on chakrit's go. Publishing is irreversible.
set -euo pipefail

cd "$(dirname "$0")/.."

execute=0
if [ "${1:-}" = "--execute" ]; then
	execute=1
fi

# A release must be reproducible from a committed state.
if [ -n "$(git status --porcelain)" ]; then
	echo "release: working tree not clean — commit first" >&2
	exit 1
fi

# Version is the single source of truth in Cargo.toml (bump with `cargo set-version`).
version="$(cargo pkgid | sed 's/.*[#@]//')"
tag="v$version"
echo "release: tmuxctl $version (tag $tag) [execute=$execute]"

# Full done-gate, including the async drivers.
cargo test --all-features
cargo clippy --all-targets --all-features
cargo fmt --check

if [ "$execute" -eq 0 ]; then
	echo "release: dry run — verifying the package; pass --execute to publish"
	cargo publish --dry-run
	echo "release: dry run OK"
	exit 0
fi

# Idempotent: if the tag already exists (e.g. cargo publish failed and we're
# retrying), skip tag + GitHub release and go straight to publishing.
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
	echo "release: tag $tag already exists — skipping tag + GitHub release"
else
	git tag -a "$tag" -m "tmuxctl $version"
	git push gh "$tag"
	gh release create "$tag" --title "$tag" --generate-notes
fi

cargo publish
echo "release: published tmuxctl $version"
