#
# Dockerfile for the chilled-proxy server application.
#
# chilled-proxy is a multi-registry caching proxy (crates.io, npm, PyPI, Maven)
# with cooldown age-gating; see README.md for attribution.
#

### First stage: Build the application itself.
FROM rust:alpine AS builder

WORKDIR /builds/chilled-proxy

# Copy source data (see .dockerignore for excludes).
COPY . .

# Build deps: musl-dev plus the toolchain aws-lc-rs (rustls crypto backend,
# pulled in by reqwest's rustls-tls) needs on Alpine — a C/C++ compiler, cmake,
# and clang/libclang for bindgen.
RUN \
apk add --no-cache musl-dev build-base cmake clang-dev && \
cargo build --release -p chilled-proxy

### Second stage: Copy the built application into the runtime image.
FROM alpine:latest AS runner

LABEL version="0.1.0"
LABEL description="chilled-proxy: multi-registry caching proxy with cooldown age-gating"
LABEL maintainer="3lpsy"

# Install the compiled executable into the system.
COPY --from=builder /builds/chilled-proxy/target/release/chilled-proxy /usr/bin/chilled-proxy

# Add the proxy service user and create the cache directory writable by it.
RUN \
adduser -SHD -u 777 -h /var/empty -s /sbin/nologin -g "chilled-proxy" app && \
mkdir /var/cache/chilled && \
chown app /var/cache/chilled

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

# Run the proxy server (info logging to stdout). ENTRYPOINT (not CMD) so that
# flags passed to `docker run <image> --flag ...` append to the binary instead
# of replacing it.
ENTRYPOINT ["chilled-proxy"]
