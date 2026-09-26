# artiqwest ROADMAP

The single source of truth for **work**. Every item here is a `[ ]`/`[x]` checkbox in phase order.
There is no progress file, no changelog, no run log — **git history + the PRs are the record**, and
shipped consumer-facing capability is featured in `README.md`.

Phases are priority-ordered: Phase 1 (correctness/safety) outranks Phase 2 (API surface), and so on.
Within a phase, take the cheapest workable item first.

**Contributors:** the items below are written to be picked up cold — each names the file, the actual
symptom, and what "done" looks like. Start in Phase 1.

---

## Locked decisions

Settled direction. Changing any of these is a discussion, not a patch.

- **Tor-first, always.** Every non-loopback request goes over Tor. The only bypass is loopback
  (`localhost`, `127.0.0.0/8`, `::1`). It is never widened to LAN or private ranges, and Tor is never
  made opt-in.
- **The three primitives stay simple.** `get`, `post`, and `ws` are why people reach for this crate.
  New capability arrives additively (a builder, new functions); breaking those three requires a
  planned version bump and a loud note in the release.
- **Stable Rust only.** No `#![feature(...)]` gates. Nightly is used for `cargo fmt` (the
  `rustfmt.toml` options are unstable) and nothing else.
- **Security defaults go up, never down.** Certificate verification, timeouts, and bounded retries are
  the direction of travel. No new `danger_*` escape hatches without a documented, narrow reason.
- **Don't regress the bounded cache.** The stable `./tor/arti/{state,cache}` directories exist because
  a previous default grew without bound. Making them *configurable* is wanted; reverting to
  `TorClientConfig::default()` is not.
- **Reduce C dependencies, never add them.** `tokio-native-tls` (OpenSSL via FFI) is the one on the
  request path, and moving off it is a Phase 5 item.

---

## Phase 0 — Foundations ✅

- [x] `get` / `post` over Tor via `arti-client` + `hyper`
- [x] `ws` websockets over both Tor and clearnet (`tokio-tungstenite`)
- [x] Localhost detection with a `reqwest` fallback for loopback targets
- [x] Caller-supplied `TorClient` (`existing_client`), with auto-refresh up to 5 attempts
- [x] Stable, bounded arti state/cache dirs (`./tor/arti/{state,cache}`) — fixes unbounded cache growth
- [x] ALPN-driven HTTP version dispatch (`h2` → HTTP/2, else HTTP/1.1)
- [x] `Response` type carrying both the upstream request and response, with JSON helpers
- [x] CI: build, clippy `-D warnings`, `cargo test --lib`, nightly `cargo fmt --check`
- [x] Live-Tor integration tests gated behind `#[ignore]`

---

## Phase 1 — Correctness & transport safety

The highest-value work in the crate. Everything here is offline-verifiable.

- [x] **TLS certificate verification for clearnet hosts.** `https_upgrade` (`src/streams.rs`) set
      `danger_accept_invalid_certs(true)` for *every* HTTPS target. That is defensible for `.onion`
      (self-signed by design, authenticated by the onion address itself) but it silently disabled
      authentication for clearnet requests too — a MITM at the exit relay was
      unauthenticated-by-default.

      Fixed in 0.5.0. `CertPolicy` + `cert_policy(host)` (`src/streams.rs`) make the decision, kept
      pure and separate from the handshake so it is testable offline; `is_onion(host)`
      (`src/uri.rs`) classifies the host on its final label only, so a clearnet lookalike such as
      `onion.example.com` cannot claim the exemption. `danger_accept_invalid_certs` is now reached
      only on the `AcceptSelfSigned` arm, and the `match` is exhaustive so a future policy forces a
      decision rather than defaulting to insecure. Four unit tests, including an explicit
      lookalike-bypass regression test.

      **This was a behavior change, not a pure bug fix** — a caller reaching a clearnet host with a
      self-signed certificate now fails. There is deliberately no opt-out: adding a `danger_*` knob
      is against the locked decisions above, and the escape hatch belongs on the request builder when
      that lands.
- [ ] **Duplicate request headers are silently dropped, and there is a latent panic behind it.**
      `get`/`post` take `Vec<(&str, &str)>` and immediately `collect()` it into a
      `HashMap<String, String>` (`src/lib.rs`), so passing two headers with the same name keeps only
      one — you cannot send two `Accept` or two `Cookie` headers, and nothing warns you.

      Separately, `make_request` (`src/make_request.rs`) iterates the resulting `HeaderMap` with
      `for (key, value) in request_headers` and calls `key.unwrap()`. `HeaderMap`'s by-value iterator
      yields `Option<HeaderName>`, where `None` means "same name as the previous entry" — so that
      `unwrap()` panics on any multi-valued header. It is unreachable *today* only because the
      `HashMap` collapse upstream guarantees one value per name.

      Fix both together: carry headers in a structure that preserves duplicates, and handle the `None`
      case instead of unwrapping. Otherwise fixing the first bug turns the second one live.
- [ ] **`is_https` is decided by substring sniffing.** `parse_uri` (`src/uri.rs`) does
      `uri.scheme() == Some(&Scheme::HTTPS) || uri.to_string().contains("wss://")`. The second clause
      inspects the *whole URI string*, so `http://example.com/?redirect=wss://elsewhere` is
      misclassified as HTTPS — which then picks port 443 and attempts a TLS upgrade on a plaintext
      endpoint. Match on the scheme properly (`ws`, `wss`, `http`, `https`) and unit-test the
      query-string case.
- [ ] **Every HTTP/1.1 request carries a bogus `Upgrade: HTTP/2.0` header.** `construct_headers`
      (`src/make_request.rs`) inserts `Upgrade: HTTP/2.0` unless the caller supplied an `Upgrade`, and
      it feeds *both* the Tor path (via `MakeRequest::headers`) and the loopback `reqwest` path.

      What actually reaches the wire depends on the negotiated protocol:
      - **HTTP/2 — stripped, silently.** RFC 9113 §8.2.2 makes `Upgrade` a connection-specific field
        and says a receiver "MUST treat this as a malformed message", so hyper's h2 client removes it
        before sending (hyper 1.10.1 `src/proto/h2/client.rs` → `strip_connection_headers`). hyper's
        accompanying `warn!` is compiled out unless built with `--cfg hyper_unstable_tracing`, so
        nothing is logged. This is why nothing has visibly broken.
      - **HTTP/1.1 — sent on every request.** That covers plain `http://`, HTTPS where the server did
        not negotiate `h2`, and every loopback request. It violates RFC 9110 §7.8: "A sender of
        Upgrade MUST also send an 'Upgrade' connection option in the Connection header field", and
        no `Connection: upgrade` is ever sent. A server "MAY ignore a received Upgrade header field",
        which is the likeliest reason this has gone unnoticed. HTTP/2 was only ever reachable by upgrade
        through the `h2c` token, and RFC 9113 §3.1 deprecates that usage outright.

      **Why it matters more for a Tor client than for most:** a nonstandard header on every
      HTTP/1.1 request makes artiqwest's traffic distinguishable from other clients at the
      destination. Keeping the request indistinguishable is part of the reason to use Tor at all.

      Fix: delete the injection. The crate never performs an h2c upgrade, and it already selects
      HTTP/2 correctly through ALPN (`negotiated_http_version`). Add a unit test that
      `construct_headers` emits no `Upgrade` unless the caller set one.
      Sources: <https://www.rfc-editor.org/rfc/rfc9110#section-7.8>,
      <https://www.rfc-editor.org/rfc/rfc9113#section-8.2.2>,
      <https://www.rfc-editor.org/rfc/rfc9113#section-3.1>.
- [x] **Remove the panicking header serialization.** `UpstreamRequest`/`UpstreamResponse`
      (`src/response/upstream.rs`) both did `value.to_str().unwrap()` when serializing headers, so any
      non-ASCII header value panicked the caller's task. Now serialized with
      `String::from_utf8_lossy`, matching how the body was already handled, with two regression tests
      covering opaque `obs-text` octets. Shipped in 0.4.1.
- [x] **Fix the broken doctests — all 7 of them failed.** CI runs `cargo test --lib`, which skips
      doctests entirely, so these had been rotting unseen. Measured `cargo test --doc` on 2026-09-24:
      `0 passed; 7 failed`. Three failed to **compile**:
      - `src/response/mod.rs` `from_json` and `request_from_json` — `E0061`, `post(uri, &body, None)`
        is three arguments against the four-argument signature.
      - `src/response/mod.rs` `body` — `E0277`, `println!("{}", body)` where `body` is `&[u8]`, which
        is not `Display`.

      The `ws` example was broken against tungstenite 0.29 as well — `Message::Text` takes
      `Utf8Bytes` (not `String`) and `into_data()` returns `Bytes` (not `Vec<u8>`) — and the same
      stale snippet was in `README.md`. The rest dialed the live Tor network.

      Fixed in 0.4.1: the compile errors corrected, the `ws` example and its README twin brought up
      to the 0.29 API, all seven marked `no_run` so they typecheck without needing Tor, and
      `cargo test --doc` added to CI so they cannot rot unnoticed again.
- [ ] **Audit the retry loop in `create_http_stream`.** On failure it sets the global `TOR_CLIENT` to
      `None` and re-bootstraps — including when the caller passed their own `existing_client`, whose
      lifetime the crate does not own. Decide and document the contract (never discard a caller's
      client; only recycle the crate-owned global), then restructure the loop so the retry/attempt
      accounting is legible and unit-testable.
- [ ] **Request timeouts.** No operation in the crate is time-bounded: a hung onion service holds the
      caller forever. Add a per-request timeout with a sane default and a way to override it. Until
      this lands `README.md` tells users to wrap calls in `tokio::time::timeout`.
- [ ] Audit every remaining `unwrap`/`expect` on a non-test path and either remove it or justify it
      in a comment.

## Phase 2 — Public API surface

- [ ] **Make the error type public.** `mod error` is private and `Error` is never re-exported, so
      callers only ever see an opaque `anyhow::Error` and cannot match on failure modes. Export it,
      fix the `Unkown`/`Faild` spellings in the same change, and decide whether the public signatures
      return `Result<T, artiqwest::Error>` instead of `anyhow::Result<T>` (a breaking change — plan
      the version bump).
- [ ] **HTTP methods beyond GET/POST.** `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS` — either as
      sibling functions or (preferred) one generic `request(method, uri, ..)` that `get`/`post`
      delegate to, collapsing the duplicated bodies currently in `lib.rs`.
- [ ] **The loopback path supports only GET and POST.** `make_local_request`
      (`src/make_request.rs`) matches on the method and returns
      `Error::Reqwest("Unsupported method")` for anything else. So whatever the item above adds for
      the Tor path has to be added here too, or the same call silently behaves differently depending
      on whether the target happens to be loopback.
- [ ] **Loopback response bodies are not byte-preserved.** `make_local_request` reads the response
      with `reqwest`'s `.text()`, a lossy UTF-8 conversion, then re-encodes it — so binary payloads
      (images, protobuf, gzip) come back corrupted on the loopback path while working fine over Tor.
      Read bytes instead.
- [ ] **A request builder.** The four-positional-argument signature (`uri, body, headers, client`)
      does not extend. A builder (`Request::get(uri).header(..).timeout(..).client(..).send()`) absorbs
      timeouts, redirects, methods, and bodies without another breaking signature change each time.
- [ ] **Redirect handling.** 3xx responses are returned raw; every caller reimplements the follow
      loop. Add opt-in redirect following with a bounded hop count and a cross-origin policy.
- [ ] **Non-string request bodies.** `post` takes `&str`, so binary payloads must round-trip through
      UTF-8. Accept `impl Into<Bytes>`.
- [ ] **No response decompression.** The crate never sends `Accept-Encoding` and never decodes a
      response body, so a server that compresses anyway hands the caller bytes they have to inflate
      themselves. Decide whether to negotiate and decode, or to document the omission.
- [ ] **Custom headers for `ws`.** `ws(uri, client)` has no header parameter, so an authenticated
      websocket handshake is impossible.
- [ ] **Streaming response bodies.** `Response` buffers the whole body into `Bytes`; large downloads
      are unbounded memory. Expose a streaming variant.

## Phase 3 — Tor client lifecycle & configuration

- [ ] **Configurable state/cache directories.** `tor_config()` hardcodes `./tor/arti/{state,cache}`,
      relative to the *process* working directory — two artiqwest consumers started from different
      CWDs silently get different Tor caches, and a consumer with no write access to CWD fails. Allow
      the caller to supply the directories (and/or a full `TorClientConfig`), defaulting to today's
      behavior.

      **This is currently a hard blocker on the dev workstation, not just a nicety.** arti walks the
      ancestor chain of its state directory and refuses a world-writable one. `/mnt/deepmem` is
      `drwxrwxrwx`, so every live-Tor run from this dev tree dies before it reaches the network with:

      > `Incorrect permissions: "/mnt/deepmem" is u=rwx,g=rwx,o=rwx; must be o-w`

      So the `#[ignore]`d integration tests and the four networked doctests cannot pass here at all,
      for a reason that has nothing to do with the network or with this crate's logic. Letting the
      caller point the state dir at a private location (e.g. under `$HOME`) fixes it without touching
      the permissions of a volume shared with the household server stack.
- [ ] **Replace the fixed 5-second post-bootstrap sleep** in `get_or_refresh` with a real readiness
      check — it is a guess that both wastes 5s on a warm client and can be too short on a cold one.
- [ ] **Document the global-client contract.** `TOR_CLIENT` is a process-wide `LazyLock`; spell out in
      the docs what that means for multi-tenant callers and isolation between requests.
- [ ] **Per-request stream isolation.** Two unrelated requests from one process can currently share a
      circuit, and so be correlated; `README.md` warns about this. **The API needed already exists in
      the arti-client we pin** — no upgrade required: `TorClient::isolated_client()` (arti-client
      0.43.0 `src/client.rs`) returns an `Arc<TorClient<R>>` that shares the original's internals but
      never shares circuits with it, and its docs call it "usually preferable to creating a completely
      separate TorClient instance". So this is a design question, not a research one: decide the
      isolation *unit* (per call? per destination host? caller-chosen?) and whether it is on by
      default. Interacts with the global-client contract item above.

## Phase 4 — Testing & CI

- [ ] **Grow the offline unit-test suite.** Only `src/uri.rs` and `src/response/upstream.rs` have unit
      tests today (4 in total), so CI's `cargo test --lib` proves very little. Add offline tests for
      the pure logic: `negotiated_http_version` mapping, the header conversion, request construction
      in `make_request`, the scheme parsing and the TLS-policy decision from Phase 1.
- [ ] **A loopback integration harness that does not need Tor.** Most of `make_request`/`streams` can
      be exercised against a local axum server over plain TCP. Getting this green unlocks real
      coverage in CI, where `#[ignore]`d live-Tor tests never run.
- [ ] **Supply-chain gates in CI:** `cargo audit` (or `cargo deny check`, as the sibling onyums repo
      does) on a schedule, plus `cargo deny check licenses|bans`. Dependabot is already reporting
      against this repo, but nothing runs in CI and nothing fails a build — RUSTSEC advisories reach
      the maintainer only as a GitHub alert nobody is required to look at. The `quinn-proto`
      memory-exhaustion advisory (GHSA high, `< 0.11.15`) sat in `Cargo.lock` until it was noticed by
      hand; a gate in CI is what makes the next one impossible to miss.
- [ ] **`recursion_depth_exceeding_limit` fails clippy on recent nightlies.** Under nightly 1.100
      (`rustc 1.100.0-nightly (6bb1652a0 2026-09-22)`), `cargo clippy --all-targets -- -D warnings`
      fails with 2 errors from this new future-compat lint, firing on `Send` auto-trait computation
      through `arti_client`'s own future types (`connect_with_prefs` → `get_or_launch_exit_tunnel`).
      **Confirmed pre-existing**, not introduced by any recent change: unmodified master at `b225869`
      reproduces it identically. CI runs clippy on *stable* and is green, so this is invisible there
      for now — but the lint text says "this was previously accepted by the compiler but is being
      phased out; it will become a hard error in a future release", so it will reach stable
      eventually. Track rust-lang/rust#159228
      (<https://github.com/rust-lang/rust/issues/159228>) and decide between raising
      `#![recursion_limit]` and waiting for upstream arti to shrink the future graph. **Do not
      silence it with `#![allow(...)]`.**
- [ ] **Two yanked crates in `Cargo.lock`** — `chacha20 v0.10.0` and `spin v0.9.8`, both surfaced as
      warnings by `cargo publish`. Neither is direct; find what pins them and move to unyanked
      versions.
- [ ] **`h2` in `Cargo.lock` is affected by RUSTSEC-2026-0258** (GHSA-q83h-524g-xf6h): h2 accepted and
      queued empty DATA frames without limit, so a server that does not drain streams could drive
      unbounded memory use or a length-overflow panic. The advisory rates it low severity. The lock
      has `h2 0.4.15`; the fix is in `>= 0.4.16`, and 0.4.19 is current. It reaches this crate through
      hyper's HTTP/2 client.

      This is **lockfile-only** for a library: artiqwest's dependents resolve `h2` themselves and
      already get a patched version, so no published release is affected — including 0.5.0, which was
      packaged with 0.4.15 in its lock. A cheap first increment: `cargo update -p h2`, verify, commit.

      Worth noting what it says about process: this is the second advisory in a month found by
      reading rather than by tooling (`quinn-proto` was the first), and `cargo publish` does not
      audit, so nothing on the release path would have flagged it. That is the case for the
      supply-chain gate item above. Source: <https://rustsec.org/advisories/RUSTSEC-2026-0258.html>.
- [ ] **Declare an MSRV** (`rust-version` in `Cargo.toml`) and verify it in CI. `edition = "2024"`
      implies a floor, but `src/uri.rs` also uses let-chains, which raises it further — find the real
      minimum rather than guessing, then state it. `README.md` currently has to hedge on this.
- [ ] **Nothing compiles the README's examples.** They are the first code most users run, and they
      are checked by no tooling at all — the stale tungstenite `ws` snippet sat there through a whole
      release, and a hand-written `Arc::new(TorClient::create_bootstrapped(..))` double-wrap was
      caught in review only because the examples were extracted into a scratch crate and compiled by
      hand. `create_bootstrapped` already returns `Arc<TorClient<_>>`.

      Make this automatic. `#![doc = include_str!("../README.md")]` would fold the README into the
      crate docs so `cargo test --doc` type-checks its examples on every build, and would also stop
      `README.md` and the crate-level docs from describing the same API in two places that drift
      apart. Mind the interaction with the `no_run` convention and with the README's non-Rust fences.
- [ ] Check that `docs.rs` builds cleanly, including the `arti-client` `static` feature; the README
      badge links it.
- [x] Add `cargo test --doc` to CI (shipped alongside the Phase 1 doctest fix, in 0.4.1).

## Phase 5 — Footprint & performance

- [ ] **Move `tracing-subscriber` to `[dev-dependencies]`.** A library must not ship a subscriber —
      installing a global default is the binary's decision, and every consumer currently pays the
      compile cost for something only the tests use.
- [ ] **Trim the `reqwest` dependency.** A full-default `reqwest` is pulled in solely for the loopback
      fallback, duplicating the TLS and hyper stacks. Either narrow its features or serve loopback
      with the `hyper` client the crate already has. Doing the latter would also fix the two loopback
      divergences filed in Phase 2 (methods, binary bodies) at the root.
- [ ] **Dedupe the crate graph.** `lib.rs` carries `#![allow(clippy::multiple_crate_versions)]`;
      find out what is actually duplicated (`cargo tree --duplicates`) and remove the allow if the
      duplication can be resolved.
- [ ] **Evaluate `rustls` in place of `tokio-native-tls`.** The crate's TLS today is OpenSSL through
      FFI — it is why CI has to `apt-get install libssl-dev`, why `README.md` has to tell users to
      install it, and it is the only C dependency on the request path. `rustls` is pure Rust, is
      already in `[dev-dependencies]`, and is what `arti` itself prefers. Scope the migration (ALPN,
      the per-host verification policy from Phase 1, the `.onion` self-signed case) before committing
      to it; a feature flag may be the landing strategy.

      **There is now a working precedent next door.** onyums 0.5.0 made this exact move — it depends
      on `tokio-rustls ^0.26.4` and `rustls-graviola ^0.4.0`, with no `native-tls` —
      (<https://crates.io/api/v1/crates/onyums/0.5.0/dependencies>). Read how it handles the
      self-signed onion certificate before designing artiqwest's side, since the `.onion` exemption
      from Phase 1 has to survive the migration intact.
- [ ] **Connection reuse.** Every request builds a fresh Tor stream and TLS handshake; a keep-alive
      pool keyed by host would remove seconds per request on repeat calls.
- [ ] Benchmark request latency (onion and clearnet) so the pooling work above has a before/after
      number to hold.

## Phase 6 — JavaScript bindings

Make artiqwest callable from JavaScript, so a Node/Deno/Bun program can fetch over Tor without
shelling out to a Tor daemon or writing any Rust. This is a new deliverable rather than a fix to
existing code, so it sits after the correctness phases — but it is the phase most likely to bring new
users, and none of it is blocked by the open items above.

**Decide the target before writing any binding code.** The three options are not interchangeable and
the choice determines everything after it:

- [ ] **Pick the binding strategy: native addon vs WASM vs a local sidecar.** The decisive constraint
      is that arti needs real TCP sockets and a filesystem for its state and directory cache.
      - **Native addon (`napi-rs`, recommended starting assumption).** Compiles the real crate,
        real arti, real sockets. Gives Node/Deno/Bun a normal `import`. Cost: prebuilt binaries per
        platform and libc, which is the bulk of the work — and today the crate links OpenSSL through
        `native-tls`, so the `rustls` migration in Phase 5 is close to a prerequisite for sane
        cross-compilation.
      - **WASM (`wasm-bindgen`).** A browser cannot open raw TCP, so browser WASM cannot run arti at
        all — this is a hard blocker, not an inconvenience. `wasm32-wasip1` with socket support is
        plausible for server-side runtimes but a much larger bet. Do not promise a browser build.
      - **Sidecar.** Ship a small Rust binary exposing a local HTTP/IPC API and a thin pure-JS
        client. Least elegant, by far the cheapest to deliver and support, and the only option that
        avoids per-platform native artifacts entirely. Worth costing honestly rather than dismissing.

      Write the decision and its reasoning into this item before starting, because the items below
      assume it.
- [ ] **Decide the JS API shape, and write it down before implementing.** A `fetch`-shaped API is
      what a JS caller will expect and would let existing code migrate by swapping the import, but
      artiqwest's surface is narrower than `fetch` (no streaming bodies, no `AbortSignal`, GET/POST
      only until Phase 2 lands) and pretending otherwise invites bug reports. Either implement a
      genuine `fetch` subset and document exactly which parts are absent, or expose an explicitly
      artiqwest-shaped API (`get`/`post`/`ws`) that does not imply more than it delivers. Prefer the
      latter until the Phase 2 API work lands.
- [ ] **Map the error type across the boundary.** Blocked on the Phase 2 item that makes `Error`
      public — until callers can match on failure modes in Rust, a JS binding can only surface
      opaque strings, which is not a usable API. Sequence these together.
- [ ] **Decide how the Tor client is shared.** The Rust side keeps a process-wide `TorClient` behind a
      `LazyLock`. Exposing that to JS raises questions the Rust API has so far avoided: does the JS
      module own one implicit client, can a caller construct and pass several, and what happens on
      bootstrap failure? Resolve alongside the Phase 3 global-client contract item.
- [ ] **Decide where the Tor state directory lives for a JS caller.** The current CWD-relative
      `./tor/arti/{state,cache}` is a poor default for an npm package — it would scatter state
      wherever `node` happened to be started, and arti refuses a world-writable ancestor. Depends on
      the Phase 3 configurable-directories item; a JS binding should probably default to a per-user
      data directory instead.
- [ ] **WebSockets across the boundary.** `ws` returns a Rust sink/stream pair. JS expects either the
      `WebSocket` event API or an async iterator. Decide which, and note that this is the hardest part
      of any binding strategy — a sidecar in particular needs its own framing for it.
- [ ] **Publishing and CI.** An npm package, a release pipeline that builds whatever artifacts the
      chosen strategy needs, and a smoke test that actually fetches over Tor from JS in CI. Note that
      CI cannot currently run anything live-Tor at all (see Phase 4), so this needs that solved first
      or an explicitly offline smoke test.
- [ ] **Research the ecosystem before committing.** Check what already exists for Tor-from-JS and how
      other arti consumers have approached bindings; `napi-rs` and `wasm-bindgen` release notes for
      anything that changes the calculus above. Fold findings back into these items with source URLs.

## Cross-cutting

- [ ] Keep `README.md` current with shipped capability only — never aspirational work. Where current
      behavior is a hazard (the missing timeouts, shared circuits), say so plainly rather than omitting
      it — and remove the warning the release it stops being true, as the TLS one was in 0.5.0.
- [ ] Keep dependencies current (`cargo update`, `cargo upgrade`), including majors, whenever they can
      be made green. Respect the deliberate pin: `arti-client` / `tor-rtcompat` are matched to the
      versions the sibling `onyums` crate builds against. As of 2026-09-26 the other direct
      dependencies have releases available: hyper 1.11.1 (we lock 1.10.1), reqwest 0.13.5 (0.13.4),
      and tokio-tungstenite **0.30.0** (0.29). That last one is a `0.x` minor, so treat it as
      potentially breaking — the websocket API changed under us at 0.29, and the README and doctest
      `ws` examples are the canary.
- [ ] **Move arti to 0.46, together with the onyums dev-dependency.** The pin above now points somewhere
      new: onyums 0.5.0 requires `arti-client ^0.46.0` and `tor-rtcompat ^0.46.0`
      (<https://crates.io/api/v1/crates/onyums/0.5.0/dependencies>), while artiqwest still pins
      0.43.0 and dev-depends on onyums 0.3.1. arti-client 0.46.0 is the current release.

      What is known about the jump, from the crates.io feature lists: 0.46 **removed** the
      `counter-galois-onion` and `flowctl-cc` features (and added `hsc-/hss-negotiate-extensions`),
      but artiqwest enables only `full` and `static`, and both still exist; `tor-rtcompat` 0.46 still
      has `full`, `tokio`, and `native-tls`. So the manifest likely needs no feature edits — but the
      API across three minors has not been checked, and should be.

      **Move both in one change.** Bumping only onyums would put a second arti stack in every test
      build (0.43 for the crate, 0.46 for onyums), and a cold build is already ~38 minutes on the dev
      workstation. Also check the test helpers (`create_onyums_server`: `serve`, `get_onion_name`)
      against the onyums 0.5 API, which has not been verified.
- [ ] Keep `docs.rs` building; the README badge links it.
