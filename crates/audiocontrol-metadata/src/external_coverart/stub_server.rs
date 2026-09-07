//! A single-connection HTTP server for testing the provider's wire
//! behaviour.
//!
//! Small enough to be obvious, which is the point: a test that needs a
//! dependency to verify that a GET carries the right headers has stopped
//! testing the thing it was written for.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use parking_lot::Mutex;

pub struct StubServer {
    port: u16,
    last_request: Arc<Mutex<Option<String>>>,
    requests: Arc<Mutex<Vec<String>>>,
    queue: Arc<Mutex<Vec<Canned>>>,
}

/// One canned answer: a status, a content type and a body.
#[derive(Clone)]
pub struct Canned {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Canned {
    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "application/json".to_string(),
            body: body.as_bytes().to_vec(),
        }
    }

    /// An answer that is not JSON -- image bytes, most of the time.
    pub fn bytes(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self { status, content_type: content_type.to_string(), body }
    }
}

impl StubServer {
    /// Answer every connection with this status and body, then keep
    /// listening.
    pub fn serving(status: u16, body: &str) -> Self {
        Self::start(Some(vec![Canned::json(status, body)]))
    }

    /// Answer the queued responses in order, one per connection. Once the
    /// queue is down to its last entry that entry is repeated, so a test only
    /// has to describe the answers it cares about -- and so `serving`, which
    /// is a queue of one, keeps answering every request.
    pub fn queued(responses: Vec<Canned>) -> Self {
        Self::start(Some(responses))
    }

    /// Accept connections and never answer, so the client hits its timeout.
    pub fn silent() -> Self {
        Self::start(None)
    }

    fn start(responses: Option<Vec<Canned>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
        let port = listener.local_addr().expect("a bound address").port();
        let last_request = Arc::new(Mutex::new(None));
        let requests = Arc::new(Mutex::new(Vec::new()));

        let recorded = last_request.clone();
        let all = requests.clone();
        let silent = responses.is_none();
        let queue = Arc::new(Mutex::new(responses.unwrap_or_default()));
        let kept = queue.clone();

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let recorded = recorded.clone();
                let all = all.clone();
                let queue = queue.clone();
                thread::spawn(move || {
                    let response = if silent {
                        None
                    } else {
                        let mut queue = queue.lock();
                        // Repeat the last answer once the queue is down to
                        // one, so `serving` keeps answering every request.
                        if queue.len() > 1 {
                            Some(queue.remove(0))
                        } else {
                            queue.first().cloned()
                        }
                    };
                    handle(stream, response, recorded, all)
                });
            }
        });

        Self { port, last_request, requests, queue: kept }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/coverart", self.port)
    }

    /// The base URL, for building a second path the same server answers.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The most recent request, headers and body included, as received.
    pub fn last_request(&self) -> Option<String> {
        self.last_request.lock().clone()
    }

    /// Replace the queued answers after construction.
    ///
    /// The one thing a constructor cannot do: an answer that has to name the
    /// port this server is listening on can only be written once the server
    /// exists. A test builds the server, reads `base_url`, then sets the
    /// queue -- rather than needing two servers to describe one exchange.
    pub fn set_queue(&self, responses: Vec<Canned>) {
        *self.queue.lock() = responses;
    }

    /// Every request received, in arrival order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().clone()
    }
}

fn handle(
    mut stream: TcpStream,
    response: Option<Canned>,
    recorded: Arc<Mutex<Option<String>>>,
    all: Arc<Mutex<Vec<String>>>,
) {
    // Read up to the end of the headers, then -- when a Content-Length says
    // there is one -- the body that follows, so the recorded request carries
    // what a POST actually sent, not just its headers. A request with no
    // Content-Length (every GET these servers have ever seen) reads zero
    // further bytes, so it cannot block waiting for a body that never comes.
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while stream.read_exact(&mut byte).is_ok() {
        request.push(byte[0]);
        if request.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let content_length = String::from_utf8_lossy(&request)
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim().eq_ignore_ascii_case("content-length").then(|| value.trim().to_string())
        })
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);

    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        if stream.read_exact(&mut body).is_ok() {
            request.extend_from_slice(&body);
        }
    }

    let request = String::from_utf8_lossy(&request).into_owned();
    *recorded.lock() = Some(request.clone());
    all.lock().push(request);

    let Some(canned) = response else {
        // Hold the connection open with no answer, so the client times out.
        thread::sleep(std::time::Duration::from_secs(30));
        return;
    };

    let reason = if (200..300).contains(&canned.status) { "OK" } else { "Error" };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        canned.status,
        reason,
        canned.content_type,
        canned.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&canned.body);
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use acr_http::http_client;

    /// The capability `core_client`'s tests rely on: a POST's body is
    /// captured, not just its headers. Guarded here directly rather than
    /// only being exercised incidentally by a client test elsewhere.
    #[test]
    fn a_posted_body_is_recorded_in_the_request_log() {
        let server = StubServer::serving(200, r#"{"ok":true}"#);
        let client = http_client::new_http_client(5);
        let _ = client.post_json_value(
            &format!("{}/thing", server.base_url()),
            serde_json::json!({"title": "Nemo", "artist": "Nightwish"}),
        );

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let body = requests[0]
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("a body after the headers");
        // Not a fixed string: `serde_json::json!` builds a `Value` backed by
        // a `BTreeMap`, so its wire order is alphabetical by key rather than
        // the order written here.
        let parsed: serde_json::Value = serde_json::from_str(body).expect("valid JSON body");
        assert_eq!(parsed["title"], "Nemo");
        assert_eq!(parsed["artist"], "Nightwish");
    }

    /// A request with no body (every GET) must not block waiting for one
    /// that never arrives.
    #[test]
    fn a_bodyless_request_is_recorded_without_blocking() {
        let server = StubServer::serving(200, r#"{"images":[]}"#);
        let client = http_client::new_http_client(5);
        let _ = client.get_json_with_headers(&server.url(), &[]);

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /coverart HTTP/1.1"));
    }
}
