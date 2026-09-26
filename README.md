![logo](./assets/artiqwest-logo.svg)

<br>

[![docs.rs](https://img.shields.io/docsrs/artiqwest?style=for-the-badge)](https://docs.rs/artiqwest/latest/artiqwest/)
[![Crates.io Version](https://img.shields.io/crates/v/artiqwest?style=for-the-badge)](https://crates.io/crates/artiqwest)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/artiqwest?style=for-the-badge)](https://crates.io/crates/artiqwest)
![Crates.io License](https://img.shields.io/crates/l/artiqwest?style=for-the-badge)

<br>

**An HTTP and WebSocket client that speaks Tor, with an API small enough to learn in a minute.**

Artiqwest routes your requests through the Tor network using [arti](https://gitlab.torproject.org/tpo/core/arti)
and [hyper](https://github.com/hyperium/hyper). There is no separate Tor daemon to install, no SOCKS
proxy to configure, and no connection setup to write — call `get`, and a Tor client is bootstrapped
for you on first use.

It reaches both `.onion` hidden services and ordinary clearnet sites, over HTTP/1.1 or HTTP/2, plus
WebSockets over either.

## Install

```toml
[dependencies]
artiqwest = "0.5"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

That is everything the quick start needs. Some examples further down use a few more crates:
`serde` (with the `derive` feature) for JSON, `futures-util` and `tokio-tungstenite` for WebSockets,
and `arti-client` if you want to build your own Tor client.

## Quick start

```rust
use artiqwest::get;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // A Tor client bootstraps automatically on the first request.
    let response = get("https://check.torproject.org/api/ip", None, None).await?;

    println!("status: {}", response.status());
    println!("body:   {response}");

    Ok(())
}
```

## The whole API

Three functions:

| Function | Returns |
|---|---|
| `get(uri, headers, client)` | `Result<Response>` |
| `post(uri, body, headers, client)` | `Result<Response>` |
| `ws(uri, client)` | `Result<(Sink, Stream)>` |

`headers` and `client` are optional — pass `None` and sensible defaults apply.

### GET with headers

```rust
use artiqwest::get;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let headers = vec![("User-Agent", "my-app/1.0")];
    let response = get("https://httpbin.org/get", Some(headers), None).await?;

    assert!(response.status().is_success());
    Ok(())
}
```

### POST to a hidden service

```rust
use artiqwest::post;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let body = r#"{"test": "testing"}"#;
    let headers = vec![("Content-Type", "application/json")];

    let response = post(
        "http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo",
        body,
        Some(headers),
        None,
    )
    .await?;

    assert_eq!(response.to_string(), body);
    Ok(())
}
```

### Reading a response

`Response` carries both halves of the exchange — what you sent and what came back.

```rust
use artiqwest::get;
use serde::Deserialize;

#[derive(Deserialize)]
struct Ip {
    origin: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let response = get("https://httpbin.org/ip", None, None).await?;

    let _ = response.status();   // hyper::StatusCode
    let _ = response.headers();  // &hyper::HeaderMap
    let _ = response.version();  // hyper::Version — which protocol was negotiated
    let _ = response.body();     // &[u8], the raw bytes
    let _ = response.to_string();// the body as a String, via Display

    // Deserialize the body as JSON.
    let ip: Ip = response.from_json()?;
    println!("the exit relay appeared as: {}", ip.origin);

    Ok(())
}
```

The request side is available too, which helps when you need to see what actually went out:
`request_method()`, `request_uri()`, `request_headers()`, `request_version()`,
`request_to_string()`, and `request_from_json()`.

### WebSockets

Works over Tor and clearnet alike. `ws` hands back a split sink and stream, so reading and writing
are independent.

```rust
use artiqwest::ws;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (mut write, mut read) = ws(
        "wss://ydrkehoqxt2q5atkmiyw7gmphvrmp6fkaufvt525cjr4hma3pb75nyid.onion/events",
        None,
    )
    .await
    .unwrap();

    write.send(Message::Text("Hello WebSocket".into())).await?;

    while let Some(Ok(message)) = read.next().await {
        if message.is_close() {
            break;
        }
        println!("received: {}", String::from_utf8_lossy(&message.into_data()));
    }

    Ok(())
}
```

## How requests are routed

**Everything goes over Tor except loopback.** Requests to `localhost`, `127.0.0.0/8`, or `::1` are
sent directly, without Tor — routing them through the network would be pointless and would fail
anyway. Everything else, clearnet and `.onion` alike, goes through a Tor circuit.

"Loopback" means loopback specifically, not "local". A request to a LAN address such as
`192.168.1.10` is **not** treated as local and will be routed over Tor.

Loopback requests are served by [reqwest](https://github.com/seanmonstar/reqwest) rather than the Tor
path, which has two consequences worth knowing:

- Only `GET` and `POST` are supported there; other methods return an error.
- Response bodies are read as text, so binary payloads are not byte-preserved on that path.

Both are tracked in the [roadmap](./ROADMAP.md).

## The Tor client

You do not have to manage one. On the first request Artiqwest bootstraps a `TorClient` and caches it
for the life of the process; later requests reuse it. If it expires or drops its connection, the
client is rebuilt automatically — up to five attempts before the call fails.

If you already have a bootstrapped `arti_client::TorClient`, pass it as the last argument and it will
be used instead of the internal one:

```rust
use artiqwest::get;
use arti_client::TorClient;
use arti_client::config::TorClientConfigBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Give arti its own state and cache directories. Prefer this over
    // TorClientConfig::default(), whose shared location can grow large.
    let mut builder = TorClientConfigBuilder::from_directories(
        "./tor/arti/state",
        "./tor/arti/cache",
    );
    builder.address_filter().allow_onion_addrs(true);

    // `create_bootstrapped` already hands back an `Arc<TorClient<_>>`.
    let tor_client = TorClient::create_bootstrapped(builder.build()?).await?;

    let response = get("https://example.com", None, Some(tor_client.clone())).await?;
    println!("{}", response.status());

    Ok(())
}
```

### Where Tor state lives

By default Artiqwest keeps its arti state and directory cache in **`./tor/arti/state` and
`./tor/arti/cache`, relative to the process's working directory.** Two things follow from that, and
both have caught people out:

- The location depends on where your program was *started*, not where the binary lives. Launching the
  same program from two different directories gives it two separate Tor caches.
- The process needs write access there, and arti additionally refuses a state directory that has a
  **world-writable ancestor**, failing with `Incorrect permissions: ... must be o-w`.

Pass your own `TorClient` (above) if you need the state somewhere else. Making the built-in
directories configurable is on the [roadmap](./ROADMAP.md).

## Security notes

Please read this before using Artiqwest for anything that matters.

- **Clearnet certificates are verified; `.onion` certificates are not.** This split is deliberate.
  A clearnet host is checked against the platform trust store exactly as any other HTTPS client would
  check it — Tor conceals *who* is asking, but it does nothing to prove the far end is who it claims,
  and the exit relay is precisely the position from which to substitute a certificate. An onion
  service is exempt, because there the hostname *is* the service's public key and the Tor protocol
  authenticates it during the rendezvous; no public CA issues certificates for `.onion`, so
  self-signed is the norm there. Only the final label counts, so a clearnet lookalike like
  `onion.example.com` is verified normally.

  *Changed in 0.5.0.* Earlier versions accepted invalid certificates for **every** host, clearnet
  included. If you were relying on that to reach a clearnet host with a self-signed or expired
  certificate, that request now fails — which is the point, but it is a behavior change. There is no
  opt-out yet; it will arrive with the request builder on the [roadmap](./ROADMAP.md).
- **Requests are not time-bounded.** There is no timeout yet, so an unresponsive service can hold a
  call indefinitely. Wrap calls in `tokio::time::timeout` if you need a bound.
- **The Tor client is process-wide.** Requests may share circuits, so two requests from the same
  process are not necessarily unlinkable. Per-request stream isolation is on the roadmap.

## Requirements

- A recent stable Rust toolchain. The crate is edition 2024 and uses let-chains; an exact MSRV is not
  yet declared (see the [roadmap](./ROADMAP.md)).
- OpenSSL development headers, because TLS currently goes through `native-tls`. On Debian and Ubuntu:
  `apt-get install libssl-dev pkg-config`. Moving to `rustls` and dropping this requirement is on the
  roadmap.

## Testing

```bash
cargo test                # fast, offline: unit tests and doc examples
cargo test -- --ignored   # the full suite, which bootstraps real Tor
```

The integration tests (`test_get`, `test_post`, `test_ws`) reach live onion and clearnet services, so
they are `#[ignore]`d by default and CI does not run them — they are slow and depend on the Tor
network being reachable. The doc examples are marked `no_run`: type-checked on every build, never
executed.

## Contributing

Open work is tracked as checkboxes in [ROADMAP.md](./ROADMAP.md), roughly in priority order —
correctness and transport safety first, then the API surface. Each item names the file and the actual
symptom, so they can be picked up cold. Issues and pull requests are welcome.

## License

MIT. See [LICENSE](./LICENSE).
