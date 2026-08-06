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
| `/maven` | Maven Central | `settings.xml` mirror / Gradle init script |

Maven also gets `/gradle-plugins` and `/google-maven` out of the box, since a Gradle build needs
all three. The mounts are configurable — see [Mount paths](#mount-paths) and
[Multiple mounts](#multiple-mounts).

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

**Gradle** — Gradle does not read the `<mirrors>` section of `~/.m2/settings.xml`, so the Maven
setting above has no effect on a Gradle build. Copy
[`examples/gradle/chilled.init.gradle`](examples/gradle/chilled.init.gradle) into `~/.gradle/init.d/`
(or pass it with `-I`) and name the proxy:

```
export CHILLED_PROXY_URL=http://proxy.example.com:3080
gradle build
```

A Gradle build resolves from three Maven-layout repositories, and a mount serves one upstream, so
the server mounts all three out of the box — the script maps each to its default mount:

| Upstream | Default mount |
| --- | --- |
| Maven Central | `/maven` |
| Gradle Plugin Portal (`plugins { }` blocks) | `/gradle-plugins` |
| Google Maven — AndroidX, AGP | `/google-maven` |

Override any one with `CHILLED_MAVEN_URL`, `CHILLED_MAVEN_PLUGINS_URL`, or
`CHILLED_MAVEN_GOOGLE_URL` — to name a non-default mount path, or to leave that upstream direct.
An upstream with neither is **not** age-gated. Every variable also reads a system property
(`chilled.proxy.url`, `chilled.maven.url`, `.plugins.url`, `.google.url`), so it can live in
`gradle.properties`.

The script rewrites repository URLs in place — across `pluginManagement`,
`dependencyResolutionManagement`, the `settings.gradle` `buildscript`, and project `buildscript`
and `repositories` blocks — so a build using `RepositoriesMode.FAIL_ON_PROJECT_REPOS` keeps
working. Run with `--info` to see what it changed.

The settings-level blocks are hooked from `beforeSettings` rather than `settingsEvaluated`,
because a `plugins { }` block or a `buildscript { }` classpath in `settings.gradle` resolves
*while* the settings script is evaluated — a later hook would let those two reach the real
upstream ungated.

### General vs per-registry knobs

Every general flag applies to all four registries; each has per-registry variants that override
it. Env vars use the `CHILLED_*` prefix (flag name uppercased).

| General flag | Per-registry variants | Default |
| --- | --- | --- |
| `--cooldown` | `--cooldown-crates` `--cooldown-npm` `--cooldown-pypi` `--cooldown-maven` | `0` (off) |
| `--cache-ttl` | `--cache-ttl-crates` ... | `3600` |
| `--cooldown-overrides` | `--cooldown-overrides-crates` ... (replaces the general list) | empty |
| `--restrict-downloads` | `--restrict-downloads-crates` ... (`=false` opts a registry out) | off |
| `--max-metadata-size` | `--max-metadata-size-crates` ... | per-registry (see below) |
| `--max-artifact-size` | `--max-artifact-size-crates` ... | per-registry (see below) |
| `--cache-dir` | (registries use `<cache-dir>/{crates,npm,pypi,maven}`) | `/var/cache/chilled` |
| — | `--crates-path` `--npm-path` `--pypi-path` `--maven-path` | `/<registry>` |
| — | `--crates-mount` `--npm-mount` `--pypi-mount` `--maven-mount` (repeatable) | see [Multiple mounts](#multiple-mounts) |

Registries are all enabled by default; unmount one with `--disable-crates`, `--disable-npm`,
`--disable-pypi`, or `--disable-maven`.

```
# 7-day cooldown everywhere, 2 days for npm, none for maven
chilled-proxy --cooldown 7d --cooldown-npm 2d --cooldown-maven 0
```

### Size caps

An upstream response larger than its cap is refused with `507`, never truncated. Sizes accept
`k`, `m`, and `g` suffixes (powers of 1024 in every spelling — `512MB` and `512MiB` both mean
512 × 1024²). Unlike the other knobs these have no single default, because a 16 MiB crate and a
512 MiB jar are both normal:

| Registry | `--max-metadata-size` | `--max-artifact-size` |
| --- | --- | --- |
| crates.io | 64 MiB (index) | 16 MiB (`.crate`) |
| npm | 64 MiB (packument) | 256 MiB (tarball) |
| PyPI | 64 MiB (simple JSON) | 256 MiB (wheel/sdist) |
| Maven | 8 MiB (`maven-metadata.xml`) | 512 MiB (jar/aar) |

An unset flag leaves each registry on its own default; the general flag overrides all four, a
per-registry flag overrides that, and a mount's `max-artifact-size` key overrides everything.

Bodies are read into memory before being cached and served, so **`--max-artifact-size` is also a
per-request memory ceiling**. Raising it far past the default trades a clean `507` for memory
pressure under concurrency — size it against the RAM you can spare times the downloads you expect
at once, not against the largest file in existence.

Large ML wheels are the usual reason to raise it — CUDA wheels such as `nvidia-cudnn-cu13`
(~349 MiB) exceed the 256 MiB PyPI default and are refused with `507`. They are published on
PyPI, so they arrive through the ordinary PyPI mount and the per-registry flag is what moves:

```
chilled-proxy --max-artifact-size-pypi 512m
```

A mount pointed at the PyTorch index works too, and is the way to keep large ML
wheels off the shared PyPI mount:

```
chilled-proxy --pypi-mount 'name=pytorch,\
  upstream=https://download.pytorch.org/whl/cpu/,\
  files=https://download-r2.pytorch.org/,\
  cooldown=180d,max-artifact-size=2g'
```

`download.pytorch.org` serves a PEP 503 **HTML** index rather than PEP 691 JSON, and spreads its
files across more than one host — see [HTML upstreams](#html-upstreams) and
[Multi-host indexes](#multi-host-indexes) for what each part of that line is doing.

### Multi-host indexes

PyPI keeps every file on one host, so `--pypi-files-url` can name it once. An index is free not
to: PyTorch links `torch` at its own CDN, its dependencies at PyPI's, and some wheels relatively.
Reconstructing every download against a single pinned host can only ever serve one of those
slices; the rest 404.

So the file's host is read from the index document, which is the only place that knows it. That
host is upstream-controlled, so it is honored only when you have allowed it:

```
--pypi-mount 'name=pytorch,...,file-hosts=download-r2.pytorch.org'
--pypi-file-hosts 'host-a.example host-b.example'    # general, space-separated
```

The allowlist already contains the mount's own index host (covering relative links) and its
`files=` host, so an ordinary PyPI mount needs no configuration. A host that is *not* allowed
falls back to substituting `files=`, exactly as before — that fallback is what lets
`--pypi-files-url` point at an internal mirror of PyPI's file host. When the fallback is wrong
(a genuinely multi-host index), the proxy logs which host it skipped and how to allow it.

Age-gating and `--restrict-downloads` are unaffected: both already read the same cached index
document, and now the gate and the download resolve from the *same entry*, so they cannot
disagree about which file they mean.

### HTML upstreams

PyPI publishes upload times only in its PEP 691 JSON API, so JSON is requested first and
preferred. An index that answers with HTML is parsed into the same document model, which means a
PEP 503-only mirror gets the full feature set rather than a degraded pass-through.

Age-gating an HTML index needs a per-file upload time. PEP 503 has no standardized spelling for
one, but indexes that publish it (PyTorch, devpi) use a `data-upload-time` attribute, and that is
what the parser reads. The fail-closed rule is unchanged and applies per file: an entry without a
usable upload time is withheld under a cooldown.

An index where *nothing* is datable therefore serves an **empty** index — every file withheld —
rather than an error. That is deliberate: a resolver reads "no versions here" and falls through
to another index that *can* date the package, so a mount fronting an undated slice coexists with
a gated PyPI mount instead of aborting the whole resolution. Nothing ungated is ever served, and
the proxy logs loudly which project it withheld.

Because `data-upload-time` is a convention rather than a standard, an index could stop emitting
it. That degrades to an empty index plus a warning, never to silently ungated serving.

**Gating an index that only dates some of its projects.** The PyTorch index dates `torch` and
`torchvision` but not their dependencies, so a cooldown on that mount gates the former and
withholds the latter. Resolve the two from the indexes that can date them:

```
# torch wheels from the gated pytorch mount
uv pip install --no-deps --default-index "$PYTORCH_REGISTRY" torch torchvision
# their dependencies from the gated PyPI mount
uv pip install -r requirements.txt          # UV_DEFAULT_INDEX = the PyPI mount
```

Both halves are then age-gated. `--index-strategy unsafe-best-match` across both mounts also
works, but it turns off uv's dependency-confusion protection, which is at odds with running a
gating proxy in the first place.

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
must also be distinct from one another, and none may nest inside another. All of this is checked
at startup, before the listener binds, so a bad configuration fails immediately rather than
half-serving.

### Multiple mounts

A mount serves one upstream, so a registry with more than one — Maven Central plus the Gradle
Plugin Portal, an internal mirror alongside the public one — needs a mount each. Repeat
`--<registry>-mount` with a comma-separated `key=value` spec:

```
chilled-proxy --cooldown 7d \
  --maven-mount name=internal,upstream=https://nexus.corp.example.com/repository/maven-public/ \
  --npm-mount   name=npm-fast,path=/npm-edge,cooldown=1d
```

Only `name` is required. It identifies the mount in `/metrics`, names its cache subdirectory, and
supplies the default path (`/<name>`) — so two mounts of one registry never share cached
artifacts. Everything else falls back to that registry's flags, then to the general ones.

| Key | Applies to | Default |
| --- | --- | --- |
| `name` | all | *required* |
| `path` | all | `/<name>` |
| `upstream` | all | the registry's `--<registry>-upstream-url` |
| `index` | crates.io | `--crates-index-url` — but see below |
| `files` | PyPI | `--pypi-files-url` — but see below |
| `file-hosts` | PyPI | `--pypi-file-hosts` (space-separated; replaces, not extends) |
| `proxy-url` | all | `--reverse-proxy-url` + path, else derived from `--listen` |
| `cooldown` `cache-ttl` `restrict-downloads` | all | the registry's flag, then the general one |
| `max-metadata-size` `max-artifact-size` | all | the registry's flag, the general one, then its built-in default |

A mount that sets `upstream=` on a two-URL registry must state the second URL too: `index=` for
crates.io, `files=` for PyPI. Inheriting the default there would silently pair a private mirror
with the public index or file host, so that combination is refused at startup.

Cooldown-override lists are not settable per mount — the spec grammar spends the comma on its own
separator — so a mount inherits `--cooldown-overrides[-<registry>]`. In an env var, separate
whole specs with `;`: `CHILLED_MAVEN_MOUNTS='name=internal,upstream=…;name=snapshots,path=/snap'`.

**Built-in mounts.** Because gating Maven Central alone leaves Gradle's plugins and AndroidX
ungated, two extra Maven mounts are served by default:

| Mount | Upstream |
| --- | --- |
| `/gradle-plugins` | `https://plugins.gradle.org/m2/` |
| `/google-maven` | `https://dl.google.com/dl/android/maven2/` |

They inherit Maven's cooldown and cache settings. A `--maven-mount` of the same name replaces one
(to move its path or change its upstream), `--no-default-mounts` drops both, and `--disable-maven`
takes them with it. They are also skipped when Maven is mounted at `/`, which owns the listener.

### Upstream authentication

A private upstream — an internal Nexus or Artifactory, a token-gated registry — takes credentials
per mount, as HTTP basic auth or as arbitrary headers. Both are attached to every upstream request
that mount makes.

Prefer the per-mount environment variables for secrets. `<NAME>` is the mount name uppercased with
`-` and `.` folded to `_`, so the `gradle-plugins` mount reads `CHILLED_GRADLE_PLUGINS_*`:

```
CHILLED_INTERNAL_BASIC_AUTH_USERNAME=ci
CHILLED_INTERNAL_BASIC_AUTH_PASSWORD=s3cr3t
CHILLED_INTERNAL_HEADERS='X-Build: nightly; X-Team: platform'
```

The same thing on the command line, repeatable, one mount per value:

```
chilled-proxy \
  --maven-mount name=internal,upstream=https://nexus.corp.example.com/repository/maven-public/ \
  --upstream-basic-auth 'internal=ci:s3cr3t' \
  --upstream-header 'internal=X-Build: nightly'
```

Headers are not limited to authentication — any valid header works, which covers token schemes
that are not basic auth (`--upstream-header 'internal=Authorization: Bearer <token>'`) as well as
routing or tracing headers your upstream wants. Setting both basic auth and an explicit
`Authorization` header on one mount is refused rather than silently resolved.

Notes worth knowing:

- **Argv is visible.** Anything passed as a flag can be read from `ps` by other users on the host;
  the environment variables exist for that reason. Docker secrets or a systemd `EnvironmentFile`
  are better still.
- **Credentials do not cross mounts.** Each authenticated mount gets its own HTTP client, so a
  mount you did not configure sends nothing — even to the same upstream host.
- **Both of a mount's URLs carry them.** The crates.io mount authenticates its index *and* its
  download host; PyPI its simple index *and* its file host. Both are URLs you configured for that
  mount.
- **Typos fail at startup.** Auth naming a mount that is not served, half a credential pair, or
  two mounts folding to the same `CHILLED_<NAME>_*` token are all refused before the listener
  binds — the alternative is an unauthenticated mount that surfaces as an upstream 401 much later.
- **Nothing is logged.** Credentials are marked sensitive, so they stay out of debug output, and
  a `user:pass@host` upstream URL is masked in the startup log.

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
| `--cache-dir` (`CHILLED_CACHE_DIR`) | Root of the on-disk cache; one subdir per mount. | `/var/cache/chilled` |
| `--cache-ttl` (`CHILLED_CACHE_TTL`) | Seconds before cached metadata is revalidated. | `3600` |
| `--listen` / `--listen-unix` | Listen on a `host:port` or a Unix-domain socket. | `0.0.0.0:3080` |
| `--reverse-proxy-url` (`CHILLED_REVERSE_PROXY_URL`) | External base URL behind a reverse proxy; every mount's default proxy URL is this base plus its path. | unset (derive from `--listen`) |
| `--crates-index-url` (`CHILLED_CRATES_INDEX_URL`) | Upstream sparse-index URL. | `https://index.crates.io/` |
| `--crates-upstream-url` (`CHILLED_CRATES_UPSTREAM_URL`) | Upstream crate-download URL. | `https://crates.io/` |
| `--npm-upstream-url` (`CHILLED_NPM_UPSTREAM_URL`) | Upstream npm registry URL. | `https://registry.npmjs.org/` |
| `--pypi-upstream-url` (`CHILLED_PYPI_UPSTREAM_URL`) | Upstream PyPI simple-index URL. | `https://pypi.org/simple/` |
| `--pypi-files-url` (`CHILLED_PYPI_FILES_URL`) | Upstream PyPI file host. | `https://files.pythonhosted.org/` |
| `--maven-upstream-url` (`CHILLED_MAVEN_UPSTREAM_URL`) | Upstream Maven repository URL. | `https://repo.maven.apache.org/maven2/` |
| `--{crates,npm,pypi,maven}-proxy-url` | External mount URLs (rewritten into metadata). | `--reverse-proxy-url` + path, else derived from `--listen` |

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
