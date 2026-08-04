# chilled-proxy task runner. Recipes grouped by prefix; `_` helpers up top.
# `just` (no args) lists the public recipes.

default:
	@just --list

# ── Helpers (`_` — deps of other recipes; hidden from `just --list`) ───────────

_version-bump part:
	#!/usr/bin/env bash
	set -euo pipefail
	part='{{part}}'
	# [workspace.package] version only (deps also have version= lines).
	cur="$(sed -n '/^\[workspace.package\]/,/^\[/p' Cargo.toml | grep -m1 '^version' | sed -E 's/.*"([^"]+)".*/\1/')"
	if [[ ! "$cur" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
	  echo "error: [workspace.package] version '$cur' is not a plain X.Y.Z" >&2
	  exit 1
	fi
	major="${BASH_REMATCH[1]}"; minor="${BASH_REMATCH[2]}"; patch="${BASH_REMATCH[3]}"
	case "$part" in
	  major) major=$((major + 1)); minor=0; patch=0 ;;
	  minor) minor=$((minor + 1)); patch=0 ;;
	  patch) patch=$((patch + 1)) ;;
	  *) echo "error: unknown bump part '$part'" >&2; exit 1 ;;
	esac
	new="${major}.${minor}.${patch}"
	# Rewrite only the first version line inside [workspace.package].
	awk -v new="$new" '
	  /^\[/ { inpkg = ($0 == "[workspace.package]") }
	  inpkg && /^version[[:space:]]*=/ && !done { sub(/"[^"]*"/, "\"" new "\""); done = 1 }
	  { print }
	' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
	echo "[workspace.package] version: ${cur} -> ${new}"
	echo "Next: cargo build (refresh Cargo.lock), commit, then 'just ci-tagged-release'."

# ── Formatting ────────────────────────────────────────────────────────────────

fmt:
	cargo fmt --all

# ── Release (ci-*) ────────────────────────────────────────────────────────────

# Push v<version> → docker.yml publishes chilled-proxy:<version> + :<major>.<minor> to ghcr.io.
ci-tagged-release:
	#!/usr/bin/env bash
	set -euo pipefail
	# Bump the workspace version (version-bump-*) and commit before tagging.
	echo "Formatting..."
	cargo fmt --all
	version="$(sed -n '/^\[workspace.package\]/,/^\[/p' Cargo.toml | grep -m1 '^version' | sed -E 's/.*"([^"]+)".*/\1/')"
	tag="v${version}"
	# Tag must point at a clean, pushed commit — CI checks out the tagged ref. Cargo.lock
	# drift is exempt but does NOT ship: CI builds from the COMMITTED lock at the tag.
	if [ -n "$(git status --porcelain -- ':!Cargo.lock')" ]; then
	  echo "error: working tree dirty (besides Cargo.lock); commit or stash before tagging ${tag}" >&2
	  exit 1
	fi
	if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
	  echo "error: tag ${tag} already exists (bump the workspace version first)" >&2
	  exit 1
	fi
	git tag -a "${tag}" -m "Release ${tag}"
	git push origin "${tag}"
	echo "Pushed ${tag} -> CI builds & pushes chilled-proxy:${version} (+ :${version%.*}) to ghcr.io"

# ── Version bump (version-bump-*) ─────────────────────────────────────────────
# Bump [workspace.package] in Cargo.toml, then cargo build (refresh lock) + commit + ci-tagged-release.

version-bump-bugfix: (_version-bump "patch")
version-bump-minor: (_version-bump "minor")
version-bump-major: (_version-bump "major")
