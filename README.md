# Artiquest
Artiqwest is a simple HTTP client that routes all requests through the Tor network using the `arti_client` and `hyper`.
It provides two basic primitives: `get` and `post` functions.

## Example
```rust
use artiqwest::get;
use artiqwest::post;

 #[tokio::main]
 async fn main() {
         // Make a GET request to httpbin.org
         let response = get("https://httpbin.org/get").await.unwrap();
         assert_eq!(response.status(), 200);

         // Make a POST request to a hidden service
         let body = r#"{"test": "testing"}"#;
         let headers = vec![("Content-Type", "application/json")];
         let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", body, Some(headers)).await.unwrap();
         assert_eq!(response.to_string(), body);
 }
 ```
