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
    ///
    /// Only the localisation wire tests need this, so it is dead code in a
    /// build that does not compile them.
    #[allow(dead_code)]
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
    ///
    /// Only the localisation wire tests need this, so it is dead code in a
    /// build that does not compile them.
    #[allow(dead_code)]
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

        Self { port, last_request, requests }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/coverart", self.port)
    }

    /// The base URL, for building a second path the same server answers.
    ///
    /// Only the localisation wire tests need this, so it is dead code in a
    /// build that does not compile them.
    #[allow(dead_code)]
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The most recent request, headers included, as received.
    pub fn last_request(&self) -> Option<String> {
        self.last_request.lock().clone()
    }

    /// Every request received, in arrival order.
    ///
    /// Only the localisation wire tests need this, so it is dead code in a
    /// build that does not compile them.
    #[allow(dead_code)]
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
    // Read up to the end of the headers. These requests carry no body.
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while stream.read_exact(&mut byte).is_ok() {
        request.push(byte[0]);
        if request.ends_with(b"\r\n\r\n") {
            break;
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
