# chilled-proxy task runner. Recipes grouped by prefix; `_` helpers up top.
# `just` (no args) lists the public recipes.

# podman if present, else docker; both accept every flag used below.
# Override with CONTAINER_ENGINE=docker.
engine := env("CONTAINER_ENGINE", `command -v podman >/dev/null 2>&1 && echo podman || echo docker`)

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

# Fails fast when the installed wasm-bindgen CLI mismatches Cargo.lock.
_ui-check-wbg:
	#!/usr/bin/env bash
	set -euo pipefail
	want="$(awk '/^name = "wasm-bindgen"$/{f=1} f && /^version = /{gsub(/[",]/,"",$3); print $3; exit}' Cargo.lock)"
	have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')" || true
	if [ "${have:-}" != "$want" ]; then
	  echo "error: wasm-bindgen CLI ${have:-missing} != Cargo.lock $want" >&2
	  echo "fix: cargo install wasm-bindgen-cli --version $want" >&2
	  exit 1
	fi

# ── Formatting ────────────────────────────────────────────────────────────────

fmt:
	cargo fmt --all

# ── Web UI (ui-*) ─────────────────────────────────────────────────────────────
# One-time setup: rustup target add wasm32-unknown-unknown

# Build the Dioxus UI into dist/ (wasm + bindgen + static files).
ui-build: _ui-check-wbg
	cargo build -p chilled-ui --profile wasm-release --target wasm32-unknown-unknown
	mkdir -p dist
	wasm-bindgen --target web --no-typescript --out-dir dist --out-name chilled-ui \
		"${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/wasm-release/chilled-ui.wasm"
	cp crates/chilled-ui/index.html crates/chilled-ui/assets/style.css dist/

# Type-check the wasm crate (web-sys and dioxus move fast).
ui-check:
	cargo check -p chilled-ui --target wasm32-unknown-unknown

# UI iteration never re-embeds or relinks the server: dist/ is read from disk.
# Dev loop: rebuild the UI, then run the server serving dist/ from disk.
ui-dev *args: ui-build
	cargo run -p chilled-proxy -- --ui --ui-dev-dist-dir dist {{args}}

# ── Container (docker-*) ──────────────────────────────────────────────────────

# Build the image (the ui-builder stage compiles and embeds the web UI).
docker-build:
	{{ engine }} build -t chilled-proxy:dev .

# Named volumes persist the cache and the UI database (users) across runs;
# the engine relabels/chowns them from the image, so rootless podman works.
# Default args: UI + first-visitor signup + 7-day cooldown; pass your own to
# override, e.g.: just docker-run --ui --ui-auth oidc --ui-oidc-user-header x-auth-request-email
# Runs on http://localhost:3080 with the web UI at /ui/. Builds the image only
# if it doesn't exist yet — run `just docker-build` to pick up code changes.
docker-run *args="--ui --ui-trust-first-user-signup --cooldown 7d --ui-cache-update-interval 30s":
	#!/usr/bin/env bash
	set -euo pipefail
	if ! {{ engine }} image inspect chilled-proxy:dev >/dev/null 2>&1; then
	  just docker-build
	fi
	{{ engine }} run --rm -it -p 3080:3080 \
		-v chilled-cache:/var/cache/chilled \
		-v chilled-data:/var/lib/chilled \
		chilled-proxy:dev {{args}}

# Remove the named volumes (cache + UI database). Stop the server first;
# the next docker-run starts from a completely clean state.
docker-clean:
	-{{ engine }} volume rm chilled-cache chilled-data

# Pull 15 packages each through the npm, PyPI, and crates mounts of a running
# `just docker-run` server. Clients run in containers (never on the host).
docker-seed proxy="http://127.0.0.1:3080":
	#!/usr/bin/env bash
	set -euo pipefail
	proxy='{{proxy}}'
	echo "== npm: 15 packages (+ older versions) via ${proxy}/npm/"
	{{ engine }} run --rm --network host node:22-alpine sh -ec "
	  mkdir -p /seed && cd /seed && npm init -y >/dev/null
	  npm install --registry ${proxy}/npm/ --no-audit --no-fund --loglevel warn \
	    lodash react express axios chalk commander debug uuid semver \
	    minimist yargs dayjs zod nanoid picocolors
	  mkdir -p /seed-old && cd /seed-old && npm init -y >/dev/null
	  npm install --registry ${proxy}/npm/ --no-audit --no-fund --loglevel warn \
	    zod@3.24.1 axios@1.6.8 lodash@4.17.20"
	echo "== pip: 15 packages via ${proxy}/pypi/simple/"
	{{ engine }} run --rm --network host -e PIP_DISABLE_PIP_VERSION_CHECK=1 \
	  python:3.12-alpine sh -ec "
	  pip download --no-cache-dir --only-binary :all: --timeout 30 --retries 2 \
	    --index-url ${proxy}/pypi/simple/ --dest /seed \
	    requests flask click rich idna urllib3 certifi charset-normalizer \
	    packaging six pyyaml jinja2 markupsafe itsdangerous werkzeug
	  pip download --no-cache-dir --only-binary :all: --timeout 30 --retries 2 \
	    --index-url ${proxy}/pypi/simple/ --dest /seed-old \
	    'requests==2.31.0' 'click==8.1.7'"
	echo "== cargo: 15 crates via ${proxy}/crates/index/"
	{{ engine }} run --rm --network host rust:alpine sh -ec "
	  mkdir -p /seed && cd /seed && cargo init -q --name seed
	  mkdir -p .cargo && printf '%s\n' \
	    '[source.crates-io]' 'replace-with = \"chilled\"' \
	    '[registries.chilled]' 'index = \"sparse+${proxy}/crates/index/\"' \
	    > .cargo/config.toml
	  printf '%s\n' \
	    'serde = \"*\"' 'serde_json = \"*\"' 'anyhow = \"*\"' 'thiserror = \"*\"' \
	    'log = \"*\"' 'itoa = \"*\"' 'ryu = \"*\"' 'once_cell = \"*\"' \
	    'bitflags = \"*\"' 'libc = \"*\"' 'cfg-if = \"*\"' 'memchr = \"*\"' \
	    'bytes = \"*\"' 'either = \"*\"' 'pin-project-lite = \"*\"' \
	    >> Cargo.toml
	  cargo fetch -q
	  mkdir -p /seed-old && cd /seed-old && cargo init -q --name seedold
	  mkdir -p .cargo && cp /seed/.cargo/config.toml .cargo/config.toml
	  printf '%s\n' 'serde = \"=1.0.100\"' 'anyhow = \"=1.0.40\"' >> Cargo.toml
	  cargo fetch -q"
	echo "== seeded; check the UI table after the next snapshot (or POST /api/snapshots/refresh)"

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
