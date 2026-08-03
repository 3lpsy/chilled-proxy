Chilled Proxy
=============

A multi-registry caching proxy — **crates.io, npm, PyPI, and Maven** on one listener — with a
configurable **cooldown delay**: the proxy hides registry versions newer than a chosen window,
so freshly-published (possibly malicious) releases are withheld until the community has had time
to detect and yank them. It is built on the caching proxy core from
[`crates-io-proxy`](https://github.com/ravenexp/crates-io-proxy), with age-gating derived from
[menhera.org's cooldown proxy](https://www.menhera.org/crates-io-cooldown-proxy-mitigating-supply-chain-attacks/)
— see [Acknowledgements](#acknowledgements).

**Warning:** This project has not been reviewed for vulnerabilities and security issues. It is
not recommended to expose the service to the internet or adversarial networks. This project is
intended to be used in a personal homelab environment. If you wish to leverage the public Docker
builds, it is recommended to pin a tagged version or hash as breaking changes are to be expected.

Each registry is mounted under a path prefix on a single port (default `3080`):

| Default mount | Registry | Client setting |
| --- | --- | --- |
| `/crates` | crates.io | `.cargo/config.toml` sparse index |
| `/npm` | npm | `.npmrc` `registry=` |
| `/pypi` | PyPI | pip/uv `index-url` |
| `/maven` | Maven Central | `settings.xml` mirror / Gradle repo |

The mounts are configurable — see [Mount paths](#mount-paths).

```
# server: 7-day cooldown for every registry
chilled-proxy --cooldown 7d
```

### Client configuration

**cargo** (`.cargo/config.toml`):

```toml
[source.crates-io]
replace-with = "chilled"

[registries.chilled]
index = "sparse+http://proxy.example.com:3080/crates/index/"
```

**npm / pnpm / yarn** (`.npmrc`):

```
registry=http://proxy.example.com:3080/npm/
```

**pip** (`pip.conf` or CLI) / **uv**:

```ini
[global]
index-url = http://proxy.example.com:3080/pypi/simple/
```

**Maven** (`~/.m2/settings.xml`):

```xml
<mirrors><mirror>
  <id>chilled</id><mirrorOf>central</mirrorOf>
  <url>http://proxy.example.com:3080/maven</url>
</mirror></mirrors>
```

**Gradle**:

```kotlin
repositories { maven { url = uri("http://proxy.example.com:3080/maven") } }
```

### General vs per-registry knobs

Every general flag applies to all four registries; each has per-registry variants that override
it. Env vars use the `CHILLED_*` prefix (flag name uppercased).

| General flag | Per-registry variants | Default |
| --- | --- | --- |
| `--cooldown` | `--cooldown-crates` `--cooldown-npm` `--cooldown-pypi` `--cooldown-maven` | `0` (off) |
| `--cache-ttl` | `--cache-ttl-crates` ... | `3600` |
| `--cooldown-overrides` | `--cooldown-overrides-crates` ... (replaces the general list) | empty |
| `--restrict-downloads` | `--restrict-downloads-crates` ... (`=false` opts a registry out) | off |
| `--cache-dir` | (registries use `<cache-dir>/{crates,npm,pypi,maven}`) | `/var/cache/chilled` |
| — | `--crates-path` `--npm-path` `--pypi-path` `--maven-path` | `/<registry>` |

Registries are all enabled by default; unmount one with `--disable-crates`, `--disable-npm`,
`--disable-pypi`, or `--disable-maven`.

```
# 7-day cooldown everywhere, 2 days for npm, none for maven
chilled-proxy --cooldown 7d --cooldown-npm 2d --cooldown-maven 0
```

### Mount paths

Each registry's path is set with `--<registry>-path` (or `CHILLED_<REGISTRY>_PATH`), defaulting
to `/<registry>`. Paths must be absolute; a trailing slash is optional. Whatever you choose is
reflected automatically in the URLs the proxy rewrites into metadata — cargo's `config.json`
download URL, npm tarball URLs, and PyPI file URLs — so clients only ever need the mount itself.

```
chilled-proxy --crates-path /rust --npm-path /registry/npm
```

**Serving from the root.** A registry can take `/` when it is the *only* one enabled, which
suits a single-ecosystem deployment:

```
chilled-proxy --npm-path / --disable-crates --disable-pypi --disable-maven
# .npmrc: registry=http://proxy.example.com:3080/
```

Starting with more than one registry enabled and any of them at `/` is refused at startup.
Note that the server's own endpoints still win at the root, so a package whose name collides
with one of them is unreachable in that layout.

**Reserved paths.** `/healthz`, `/metrics`, `/ui`, and `/api` — and anything beneath them — are
kept for the server; `/ui` and `/api` are held for a future management plane and web UI. Mounts
must also be distinct from one another. All of this is checked at startup, before the listener
binds, so a bad configuration fails immediately rather than half-serving.

### Exempting packages from the cooldown

`--cooldown-overrides` takes a comma-separated package list served unfiltered. Use it for
first-party packages you publish and consume yourself. Matching is case-insensitive; names use
each registry's canonical form — crates.io names (`-`/`_` **not** normalized), npm names
(`@scope/name`), PyPI [PEP 503-normalized](https://peps.python.org/pep-0503/#normalized-names)
names, Maven `group:artifact` coordinates. The general list applies to every registry; a
per-registry list (`--cooldown-overrides-npm ...`) *replaces* it for that registry.

```
chilled-proxy --cooldown 7d --cooldown-overrides my-app,@myco/tool,my-pylib \
  --cooldown-overrides-maven com.example:my-artifact
```

### Restricting downloads

By default the cooldown only hides too-new versions from registry *metadata*, so resolvers never
pick them. The **download** endpoints stay version-agnostic — a client that already knows an
exact version (a hand-edited lockfile) could still fetch it directly. `--restrict-downloads`
also enforces the cooldown on downloads: a version whose publish time is newer than the window
is refused with `403`.

The check is **fail-closed** per registry:

- **crates.io** reads the version's `pubtime` from the locally cached pristine index entry.
- **npm** reads the version's `time` entry from the locally cached pristine packument.
- **PyPI** reads the file's `upload-time` from the locally cached simple-index JSON.
- **Maven** uses the per-version timestamp store (see below), probing on demand.

If the metadata isn't cached the proxy fetches it on demand, so installs that skip metadata
entirely (`npm ci` from a lockfile, a pinned `Cargo.lock`) still work. If it still cannot
establish the version's age — unknown version, unreachable upstream, missing timestamp — the
download is refused. Cooldown-override packages are exempt here too.

### When a timestamp is missing

Registries differ in what they guarantee, so the filters do too. crates.io and npm **keep** a
version whose timestamp is missing or unparseable (private registries and mirrors do not always
emit one, and hiding everything would be worse than the gap). PyPI **drops** a file with no
`upload-time`, because PEP 700 requires the field and its absence means a non-conforming index.
Maven has no timestamps at all in its metadata, so an unprobeable version is treated as
first-seen-now and gated until the window passes — retried while it gates, so a transient probe
failure clears itself. The download gate is fail-closed on every registry regardless.

### Per-registry cooldown mechanics

- **crates.io** — sparse-index NDJSON lines carry `pubtime`; too-new lines are dropped.
- **npm** — the full packument's `time` map is authoritative; too-new versions are removed from
  `versions`/`time`, dangling dist-tags are dropped, and `latest` is repointed to the newest
  surviving version. Tarball URLs are rewritten through the proxy so the gate can't be bypassed.
- **PyPI** — the upstream is always fetched as PEP 691 JSON (which carries per-file
  `upload-time`); too-new files are dropped and both JSON and PEP 503 HTML are rendered from the
  filtered document. File URLs are rewritten through the proxy. An upstream that can't serve the
  JSON simple API cannot be age-gated and is refused while cooldown is active (fail-closed).
- **Maven** — `maven-metadata.xml` carries no per-version timestamps, so the proxy HEAD-probes
  each version's `.pom` and uses its `Last-Modified` (immutable on Maven Central), persisted in a
  sidecar file per artifact; if a probe fails the version is treated as first-seen-now (gated for
  a full window). Too-new `<version>` entries are dropped and `<latest>`/`<release>` repointed.
  Because the served metadata differs from upstream, `maven-metadata.xml.sha1/.md5/...` are
  regenerated from the filtered bytes. Note: a Maven build pinned to an exact version fetches
  artifacts without consulting metadata — pair cooldown with `--restrict-downloads` for a real
  gate. SNAPSHOT repositories are not age-gated.

### Logging

Logs are written to **stdout**. The level defaults to `info` and is set with `--log-level`
(or `CHILLED_LOG_LEVEL`): `error`, `warn`, `info`, `debug`, `trace`, or `off`. `-v`/`-vv` are
shortcuts for `debug`/`trace`, and `RUST_LOG` still overrides everything (and allows per-module
filters). At `info`, each artifact download is logged along with errors, malformed requests, and
bad upstream responses; routine metadata cache hits stay at `debug`.

### Status, health, and metrics endpoints

`GET /` is a minimal liveness endpoint listing the mounted registries:

```json
{"status":"running","registries":["crates","npm","pypi","maven"]}
```

`GET /healthz` is a health-check endpoint for probes/load balancers — HTTP 200 with a plain
`ok` body.

`GET /metrics` lists the artifacts currently cached per registry, with cache timestamps (unix
seconds). It is **only routed when enabled** with `--enable-metrics` (or
`CHILLED_ENABLE_METRICS=1`); otherwise it returns `404`.

```json
{"service":"chilled-proxy","registries":{
  "crates":{"cached_count":1,"artifacts":[{"name":"cfg-if","version":"1.0.0","cached_at":1780376385}]},
  "npm":{"cached_count":0,"artifacts":[]},
  "pypi":{"cached_count":0,"artifacts":[]},
  "maven":{"cached_count":0,"artifacts":[]}}}
```

---

## How it works

Each registry proxy shares the same skeleton (inherited from `crates-io-proxy`):

- **Metadata endpoints** are forwarded upstream with conditional requests (revalidated after
  `--cache-ttl`), cached *pristine* on disk, and age-gated at serve time. Filtered responses get
  a derived weak `ETag` so client caching stays correct even as the window moves; filtered bodies
  are memoized in memory.
- **Download endpoints** are forwarded, cached pristine on disk, and served verbatim — artifact
  bytes are never modified, only metadata is filtered.
- On an upstream transport failure the proxy serves the (possibly stale) cached copy when it has
  one; upstream error statuses are forwarded.

### `config.json` rewriting (crates.io)

`GET /crates/index/config.json` is generated on the fly: it returns
`{"dl": "<crates-proxy-url>/api/v1/crates", "api": "<upstream-url>"}`, pointing cargo's crate
downloads back through this proxy. The `<crates-proxy-url>` comes from `--crates-proxy-url`
(default derived from `--listen`). npm packument tarball URLs and PyPI file URLs are rewritten
the same way.

### Key parameters

| Flag (env var) | Purpose | Default |
| --- | --- | --- |
| `--cache-dir` (`CHILLED_CACHE_DIR`) | Root of the on-disk cache; one subdir per registry. | `/var/cache/chilled` |
| `--cache-ttl` (`CHILLED_CACHE_TTL`) | Seconds before cached metadata is revalidated. | `3600` |
| `--listen` / `--listen-unix` | Listen on a `host:port` or a Unix-domain socket. | `0.0.0.0:3080` |
| `--crates-index-url` (`CHILLED_CRATES_INDEX_URL`) | Upstream sparse-index URL. | `https://index.crates.io/` |
| `--crates-upstream-url` (`CHILLED_CRATES_UPSTREAM_URL`) | Upstream crate-download URL. | `https://crates.io/` |
| `--npm-upstream-url` (`CHILLED_NPM_UPSTREAM_URL`) | Upstream npm registry URL. | `https://registry.npmjs.org/` |
| `--pypi-upstream-url` (`CHILLED_PYPI_UPSTREAM_URL`) | Upstream PyPI simple-index URL. | `https://pypi.org/simple/` |
| `--pypi-files-url` (`CHILLED_PYPI_FILES_URL`) | Upstream PyPI file host. | `https://files.pythonhosted.org/` |
| `--maven-upstream-url` (`CHILLED_MAVEN_UPSTREAM_URL`) | Upstream Maven repository URL. | `https://repo.maven.apache.org/maven2/` |
| `--{crates,npm,pypi,maven}-proxy-url` | External mount URLs (rewritten into metadata). | derived from `--listen` |

Run `chilled-proxy --help` for the full list.

### Migrating from `chilled-crates` (0.3.x)

- The binary is now `chilled-proxy`; the crates registry lives under the `/crates` prefix —
  repoint cargo at `sparse+http://.../crates/index/`.
- Env vars renamed: `CRATES_IO_PROXY_*` → `CHILLED_*` (see the table above); the `-I`/`-U`/`-S`
  short flags are gone (`--crates-index-url`, `--crates-upstream-url`, `--crates-proxy-url`).
- The default cache dir moved from `/var/cache/chilled-crates` to `/var/cache/chilled/crates`.
- `/metrics` output is now grouped per registry.

### TLS roots

By default the bundled `webpki` root certificates are used. Build with the `native-certs`
feature to use the OS-native trusted root store instead (run-time selection is not supported).

### Workspace layout

```
crates/chilled-core     registry-agnostic machinery (cooldown math, caches, ETag markers, HTTP)
crates/crates-proxy     crates.io proxy library
crates/npm-proxy        npm proxy library
crates/pypi-proxy       PyPI proxy library
crates/maven-proxy      Maven proxy library
crates/chilled-testkit  shared blackbox-test harness (wiremock upstream, in-process server)
crates/chilled-proxy    the CLI + unified server binary
```

## Acknowledgements

This project would not exist without the work it is built on:

- **[`crates-io-proxy`](https://github.com/ravenexp/crates-io-proxy)** by Sergey Kvachonok
  (ravenexp) — the caching HTTP proxy core for the `crates.io` sparse index and crate download
  server this project grew from. Licensed under `MIT OR Apache-2.0`, carried over unchanged.
- **[menhera.org's crates.io cooldown proxy](https://www.menhera.org/crates-io-cooldown-proxy-mitigating-supply-chain-attacks/)**
  by metastable-void — the sparse-index age-gating approach the cooldown filter is ported from.
- **[`httpdate`](https://crates.io/crates/httpdate)** by Pyfisch — the IMF-fixdate
  formatting/parsing logic (and Howard Hinnant's civil-date algorithms it builds on) that
  `chilled-core` vendors in place of the crate.
