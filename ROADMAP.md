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

- [ ] **TLS certificate verification for clearnet hosts.** `https_upgrade` (`src/streams.rs`) sets
      `danger_accept_invalid_certs(true)` for *every* HTTPS target. That is defensible for `.onion`
      (self-signed by design, authenticated by the onion address itself) but silently disables
      authentication for clearnet requests — a MITM at the exit relay is unauthenticated-by-default.
      Verify certs normally when the host is not `.onion`; keep accept-invalid for `.onion` only.
      Add a unit test over the policy-selection function (the decision is pure; the handshake is not).
      **This is the single most important open item in the crate, and `README.md` warns users about it
      until it lands.**
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
- [ ] **Per-request stream isolation.** Investigate `arti_client` isolation tokens so two unrelated
      requests from one process are not correlatable through a shared circuit. `README.md` currently
      warns that they may be.

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
- [ ] **Connection reuse.** Every request builds a fresh Tor stream and TLS handshake; a keep-alive
      pool keyed by host would remove seconds per request on repeat calls.
- [ ] Benchmark request latency (onion and clearnet) so the pooling work above has a before/after
      number to hold.

## Cross-cutting

- [ ] Keep `README.md` current with shipped capability only — never aspirational work. Where current
      behavior is a hazard (the TLS verification gap, the missing timeouts, shared circuits), say so
      plainly rather than omitting it.
- [ ] Keep dependencies current (`cargo update`, `cargo upgrade`), including majors, whenever they can
      be made green. Respect the deliberate pin: `arti-client` / `tor-rtcompat` are matched to the
      versions the sibling `onyums` crate builds against.
- [ ] Keep `docs.rs` building; the README badge links it.
