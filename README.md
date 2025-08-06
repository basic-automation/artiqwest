![logo](./assets/artiqwest-logo.svg)
<br><br>
<a src="https://docs.rs/artiqwest/latest/artiqwest/">![docs.rs](https://img.shields.io/docsrs/artiqwest?style=for-the-badge)</a> <a src="https://crates.io/crates/artiqwest">![Crates.io Total Downloads](https://img.shields.io/crates/d/artiqwest?style=for-the-badge)</a> ![Crates.io License](https://img.shields.io/crates/l/artiqwest?style=for-the-badge)
<br><br>
Artiqwest is a simple HTTP client that routes *all *(except localhost connects where it fallbacks to [reqwest](https://github.com/seanmonstar/reqwest))* requests through the Tor network using the `arti_client` and `hyper`.
It provides two basic primitives: `get` and `post`,  functions.

Artiqwest also provides a `ws` function to create a websocket connection to a hidden service using [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite). ~~~*Currently websockets only work over tor and are untested over clearnet.*~~~

* ***NEW! Websockets no work over both Tor and clearnet! Fully tested!!!***
* ***NEW! You can now optionally pass in an existing arti `TorClient`. If you don't have one don't worry, Atriqwest will handle the Tor stuff for you automatically.***
* ***NEW! If you `TorClient` expires or loses connection we will auto-reload your `Torclient` up to five times before failing.***


## Example
```rust
use artiqwest::get;
use artiqwest::post;
use artiqwest::ws;
use futures_util::future;
use futures_util::pin_mut;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;

 #[tokio::main]
 async fn main() {
        // Make a GET request to httpbin.org
        let response = get("https://httpbin.org/get", None).await.unwrap();
        assert_eq!(response.status(), 200);

        // Make a POST request to a hidden service
        let body = r#"{"test": "testing"}"#;
        let headers = vec![("Content-Type", "application/json")];
        let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", body, Some(headers), None).await.unwrap();
        assert_eq!(response.to_string(), body);

        // Create a websocket connection to a hidden service.
        let (mut write, read) = ws("wss://ydrkehoqxt2q5atkmiyw7gmphvrmp6fkaufvt525cjr4hma3pb75nyid.onion/events", None).await.unwrap();
        let write_messages = {
        async {
            loop {
                write.send(Message::Text("Hello WebSocket".to_string())).await.unwrap();
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    };

    let read_messages = {
        read.for_each(|message| async {
            let data = message.unwrap().into_data();
            let text = String::from_utf8(data).unwrap();
            println!("Received: {text}");
        })
    };

    pin_mut!(read_messages, write_messages);

    future::select(read_messages, write_messages).await;
 }
 ```
