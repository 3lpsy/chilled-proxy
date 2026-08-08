#
# Dockerfile for the chilled-proxy server application.
#
# chilled-proxy is a multi-registry caching proxy (crates.io, npm, PyPI, Maven)
# with cooldown age-gating; see README.md for attribution.
#

### Chef base: toolchain + cargo-chef, shared by both build stages. The
### expensive layers below (chef cook) rebuild only when a manifest or the
### lockfile changes — source edits reuse the compiled-dependency layers.
FROM rust:alpine AS chef
WORKDIR /builds/chilled-proxy
RUN apk add --no-cache musl-dev build-base && cargo install cargo-chef --locked

### Planner: distill the workspace into a dependency recipe.
FROM chef AS planner
# Copy source data (see .dockerignore for excludes).
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

### UI stage: compile the Dioxus SPA to wasm and bindgen it into /dist.
FROM chef AS ui-builder

# Must equal the wasm-bindgen version in Cargo.lock (asserted below): the CLI
# and the crate speak a matched ABI.
ARG WASM_BINDGEN_VERSION=0.2.125

RUN \
rustup target add wasm32-unknown-unknown && \
cargo install --root /wbg wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked

# Dependency layer: cook the wasm dep graph from the recipe alone.
COPY --from=planner /builds/chilled-proxy/recipe.json recipe.json
RUN cargo chef cook --recipe-path recipe.json \
  -p chilled-ui --profile wasm-release --target wasm32-unknown-unknown

COPY . .

RUN \
want="$(awk '/^name = "wasm-bindgen"$/{f=1} f && /^version = /{gsub(/[",]/,"",$3); print $3; exit}' Cargo.lock)" && \
if [ "$want" != "${WASM_BINDGEN_VERSION}" ]; then \
  echo "error: Cargo.lock wants wasm-bindgen $want, ARG says ${WASM_BINDGEN_VERSION}" >&2; exit 1; \
fi && \
cargo build --locked -p chilled-ui --profile wasm-release --target wasm32-unknown-unknown && \
mkdir -p /dist && \
/wbg/bin/wasm-bindgen --target web --no-typescript --out-dir /dist --out-name chilled-ui \
  target/wasm32-unknown-unknown/wasm-release/chilled-ui.wasm && \
cp crates/chilled-ui/index.html crates/chilled-ui/assets/style.css /dist/

### First stage: Build the application itself.
FROM chef AS builder

# Extra build deps the toolchain aws-lc-rs (rustls crypto backend, pulled in
# by reqwest's rustls-tls) needs on Alpine: cmake and clang/libclang for bindgen.
RUN apk add --no-cache cmake clang-dev

# Dependency layer: cook the server dep graph from the recipe alone.
COPY --from=planner /builds/chilled-proxy/recipe.json recipe.json
RUN cargo chef cook --recipe-path recipe.json --release -p chilled-proxy

COPY . .

# The built UI lands in dist/ before the server build so include_dir! embeds
# it — the binary is the whole deploy.
COPY --from=ui-builder /dist dist/

RUN cargo build --release --locked -p chilled-proxy

### Second stage: Copy the built application into the runtime image.
FROM alpine:latest AS runner

LABEL version="1.0.1"
LABEL description="chilled-proxy: multi-registry caching proxy with cooldown age-gating"
LABEL maintainer="3lpsy"

# Install the compiled executable into the system.
COPY --from=builder /builds/chilled-proxy/target/release/chilled-proxy /usr/bin/chilled-proxy

# Add the proxy service user and create the cache directory plus the UI
# database directory writable by it.
RUN \
adduser -SHD -u 777 -h /var/empty -s /sbin/nologin -g "chilled-proxy" app && \
mkdir /var/cache/chilled /var/lib/chilled && \
chown app /var/cache/chilled /var/lib/chilled

# Switch to the service user to run the proxy process.
USER app
WORKDIR /var/empty

# Single listener; registries are mounted at /crates /npm /pypi /maven.
EXPOSE 3080

# Configuration is read from the environment (or CLI flags) at run time. These
# are NOT declared with `ENV` on purpose: an `ENV` default would shadow the
# binary's own built-in default, creating two sources of truth. Pass any of
# them with `-e NAME=value` / `--env-file`; unset vars use the code defaults.
#
# Boolean vars accept 1/0, true/false, yes/no, on/off (case-insensitive).
#
# General (apply to every registry):
#   CHILLED_CACHE_DIR            cache directory            (/var/cache/chilled)
#   CHILLED_CACHE_TTL            metadata TTL, seconds      (3600)
#   CHILLED_COOLDOWN             age-gate window, e.g. 7d   (0 = off)
#   CHILLED_COOLDOWN_OVERRIDES   comma-separated exempt packages
#   CHILLED_RESTRICT_DOWNLOADS   also refuse too-new downloads (boolean)
#   CHILLED_MAX_METADATA_SIZE    upstream metadata cap, e.g. 64m (per-registry default)
#   CHILLED_MAX_ARTIFACT_SIZE    upstream artifact cap, e.g. 512m (per-registry default;
#                                bodies are buffered, so this is a memory ceiling too)
#   CHILLED_ENABLE_METRICS       expose /metrics (boolean)
#   CHILLED_LISTEN               listen address             (0.0.0.0:3080)
#   CHILLED_LISTEN_UNIX          Unix socket path (overrides CHILLED_LISTEN)
#   CHILLED_LOG_LEVEL            error|warn|info|debug|trace|off (info)
#   RUST_LOG                     overrides the log level
#
# Per-registry overrides (default to the general value):
#   CHILLED_COOLDOWN_{CRATES,NPM,PYPI,MAVEN}
#   CHILLED_CACHE_TTL_{CRATES,NPM,PYPI,MAVEN}
#   CHILLED_COOLDOWN_OVERRIDES_{CRATES,NPM,PYPI,MAVEN}
#   CHILLED_PYPI_FILE_HOSTS      extra hosts PyPI mounts may fetch files from
#   CHILLED_MAX_METADATA_SIZE_{CRATES,NPM,PYPI,MAVEN}
#   CHILLED_MAX_ARTIFACT_SIZE_{CRATES,NPM,PYPI,MAVEN}
#   CHILLED_RESTRICT_DOWNLOADS_{CRATES,NPM,PYPI,MAVEN}
#   CHILLED_DISABLE_{CRATES,NPM,PYPI,MAVEN}   1/true to unmount a registry
#
# Registry-specific URLs:
#   CHILLED_CRATES_INDEX_URL     (https://index.crates.io/)
#   CHILLED_CRATES_UPSTREAM_URL  (https://crates.io/)
#   CHILLED_NPM_UPSTREAM_URL     (https://registry.npmjs.org/)
#   CHILLED_PYPI_UPSTREAM_URL    (https://pypi.org/simple/)
#   CHILLED_PYPI_FILES_URL       (https://files.pythonhosted.org/)
#   CHILLED_MAVEN_UPSTREAM_URL   (https://repo.maven.apache.org/maven2/)
#   CHILLED_{CRATES,NPM,PYPI,MAVEN}_PROXY_URL   external mount URLs
#
# Extra mounts (one upstream each; `;` separates whole specs):
#   CHILLED_{CRATES,NPM,PYPI,MAVEN}_MOUNTS
#     e.g. CHILLED_MAVEN_MOUNTS='name=internal,upstream=https://nexus.example.com/maven2/'
#   CHILLED_NO_DEFAULT_MOUNTS    drop the built-in /gradle-plugins and
#                                /google-maven mounts (boolean)
#
# Upstream auth, per mount (<NAME> is the mount name uppercased, `-`/`.`->`_`):
#   CHILLED_<NAME>_BASIC_AUTH_USERNAME / _PASSWORD
#   CHILLED_<NAME>_HEADERS       'X-Build: ci; X-Team: platform'
# Prefer these over --upstream-basic-auth / --upstream-header: an argv value is
# readable from `ps`. Mount the values as secrets rather than baking them in.
#
# Web UI + management API (all off unless CHILLED_UI is set):
#   CHILLED_UI                   serve /ui + /api (boolean)
#   CHILLED_UI_AUTH              builtin | oidc                    (builtin)
#   CHILLED_UI_OIDC_USER_HEADER  trusted identity header (oidc; required),
#                                e.g. x-auth-request-email from oauth2-proxy
#   CHILLED_UI_OIDC_LOGIN_URL    navbar Login target in oidc mode, e.g. /oauth2/sign_in
#   CHILLED_UI_PUBLIC_READONLY_ENABLED  unauthenticated read access to state APIs (boolean)
#   CHILLED_UI_CACHE_UPDATE_INTERVAL    snapshot interval, e.g. 10m  (10m, min 30s)
#   CHILLED_UI_TRUST_FIRST_USER_SIGNUP  first visitor creates the account
#                                       (builtin only; boolean)
#   CHILLED_UI_ADMIN_USERNAME / _PASSWORD  bootstrap user (builtin only)
#   CHILLED_UI_DB_PATH           sqlite file            (/var/lib/chilled/chilled.db)
#   CHILLED_UI_SESSION_TTL       login lifetime, e.g. 7d (7d)
# Persist /var/lib/chilled alongside /var/cache/chilled: users and the cache
# table live there, and it deliberately survives cache wipes.

# Run the proxy server (info logging to stdout). ENTRYPOINT (not CMD) so that
# flags passed to `docker run <image> --flag ...` append to the binary instead
# of replacing it.
ENTRYPOINT ["chilled-proxy"]
