# artiqwest ROADMAP

The single source of truth for **work**. Every item here is a `[ ]`/`[x]` checkbox in phase order.
There is no progress file, no changelog, no run log — **git history + the PRs are the record**, and
shipped consumer-facing capability is featured in `README.md`.

Phases are priority-ordered: Phase 1 (correctness/safety) outranks Phase 2 (API surface), and so on.
Within a phase, take the cheapest workable item first.

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
- [ ] **Remove the panicking header serialization.** `UpstreamRequest`/`UpstreamResponse`
      (`src/response/upstream.rs`) both do `value.to_str().unwrap()` when serializing headers — any
      non-ASCII header value panics the caller's task. Serialize lossily (or skip + record) and unit-test
      with a `HeaderValue::from_bytes(&[0xff])`.
- [ ] **Fix the broken doctests — all 7 of them fail.** CI runs `cargo test --lib`, which skips
      doctests entirely, so these have been rotting unseen. Measured `cargo test --doc` on 2026-09-24:
      `0 passed; 7 failed`. Three fail to **compile**:
      - `src/response/mod.rs` `from_json` (line 23) and `request_from_json` (line 100) — `E0061`,
        `post(uri, &body, None)` is three arguments against the four-argument signature.
      - `src/response/mod.rs` `body` (line 56) — `E0277`, `println!("{}", body)` where `body` is
        `&[u8]`, which is not `Display`.

      The other four (the crate-level example, `get`, `post`, `ws`) compile but fail at **runtime**
      because they dial the live Tor network. Fix the three compile errors, then mark the
      network-touching examples `no_run` so they typecheck without needing Tor, and add
      `cargo test --doc` to CI so they cannot rot again.
- [ ] **Audit the retry loop in `create_http_stream`.** On failure it sets the global `TOR_CLIENT` to
      `None` and re-bootstraps — including when the caller passed their own `existing_client`, whose
      lifetime the crate does not own. Decide and document the contract (never discard a caller's
      client; only recycle the crate-owned global), then restructure the loop so the retry/attempt
      accounting is legible and unit-testable.
- [ ] **Request timeouts.** No operation in the crate is time-bounded: a hung onion service holds the
      caller forever. Add a per-request timeout with a sane default and a way to override it.
- [ ] Audit every remaining `unwrap`/`expect` on a non-test path and either remove it or justify it
      in a comment.

## Phase 2 — Public API surface

- [ ] **Make the error type public.** `mod error` is private and `Error` is never re-exported, so
      callers only ever see an opaque `anyhow::Error` and cannot match on failure modes. Export it,
      fix the `Unkown`/`Faild` spellings in the same change, and decide whether the public signatures
      return `Result<T, artiqwest::Error>` instead of `anyhow::Result<T>` (a breaking change — plan
      the major bump).
- [ ] **HTTP methods beyond GET/POST.** `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS` — either as
      sibling functions or (preferred) one generic `request(method, uri, ..)` that `get`/`post`
      delegate to, collapsing the duplicated bodies currently in `lib.rs`.
- [ ] **A request builder.** The four-positional-argument signature (`uri, body, headers, client`)
      does not extend. A builder (`Request::get(uri).header(..).timeout(..).client(..).send()`) absorbs
      timeouts, redirects, methods, and bodies without another breaking signature change each time.
- [ ] **Redirect handling.** 3xx responses are returned raw; every caller reimplements the follow
      loop. Add opt-in redirect following with a bounded hop count and a cross-origin policy.
- [ ] **Non-string request bodies.** `post` takes `&str`, so binary payloads must round-trip through
      UTF-8. Accept `impl Into<Bytes>`.
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
- [ ] Investigate per-request stream isolation (`arti_client` isolation tokens) so two unrelated
      requests are not correlatable through a shared circuit.

## Phase 4 — Testing & CI

- [ ] **Grow the offline unit-test suite.** Only `src/uri.rs` has unit tests today; CI's
      `cargo test --lib` therefore proves very little. Add offline tests for the pure logic:
      `negotiated_http_version` mapping, the header `HashMap` conversion, request construction in
      `make_request`, the TLS-policy decision from Phase 1.
- [ ] **A loopback integration harness that does not need Tor.** Most of `make_request`/`streams` can
      be exercised against a local axum server over plain TCP. Getting this green unlocks real
      coverage in CI, where `#[ignore]`d live-Tor tests never run.
- [ ] **Supply-chain gates in CI:** `cargo audit` (or `cargo deny check`, as the sibling onyums repo
      does) on a schedule, plus `cargo deny check licenses|bans`.
- [ ] **Declare an MSRV** (`rust-version` in `Cargo.toml`) and verify it in CI. `edition = "2024"`
      already implies a floor; state it.
- [ ] Add `cargo test --doc` to CI (depends on the Phase 1 doctest fix).

## Phase 5 — Footprint & performance

- [ ] **Move `tracing-subscriber` to `[dev-dependencies]`.** A library must not ship a subscriber —
      installing a global default is the binary's decision, and every consumer currently pays the
      compile cost for something only the tests use.
- [ ] **Trim the `reqwest` dependency.** A full-default `reqwest` is pulled in solely for the loopback
      fallback, duplicating the TLS and hyper stacks. Either narrow its features or serve loopback
      with the `hyper` client the crate already has.
- [ ] **Dedupe the crate graph.** `lib.rs` carries `#![allow(clippy::multiple_crate_versions)]`;
      find out what is actually duplicated (`cargo tree --duplicates`) and remove the allow if the
      duplication can be resolved.
- [ ] **Evaluate `rustls` in place of `tokio-native-tls`.** The crate's TLS today is OpenSSL through
      FFI — it is why CI has to `apt-get install libssl-dev`, and it is the only C dependency on the
      request path. `rustls` is pure Rust, is already in `[dev-dependencies]`, and is what `arti`
      itself prefers. Scope the migration (ALPN, the per-host verification policy from Phase 1, the
      `.onion` self-signed case) before committing to it; a feature flag may be the landing strategy.
- [ ] **Connection reuse.** Every request builds a fresh Tor stream and TLS handshake; a keep-alive
      pool keyed by host would remove seconds per request on repeat calls.
- [ ] Benchmark request latency (onion and clearnet) so the pooling work above has a before/after
      number to hold.

## Cross-cutting

- [ ] Keep `README.md` current with shipped capability only — never aspirational work.
- [ ] Keep dependencies current (`cargo update`, `cargo upgrade`), including majors, whenever they can
      be made green. Respect the deliberate pin: `arti-client` / `tor-rtcompat` are matched to the
      versions the sibling `onyums` crate builds against.
- [ ] Keep `docs.rs` building; the README badge links it.
