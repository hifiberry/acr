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
}

impl StubServer {
    /// Answer every connection with this status and body, then keep
    /// listening.
    pub fn serving(status: u16, body: &str) -> Self {
        Self::start(Some((status, body.to_string())))
    }

    /// Accept connections and never answer, so the client hits its timeout.
    pub fn silent() -> Self {
        Self::start(None)
    }

    fn start(response: Option<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
        let port = listener.local_addr().expect("a bound address").port();
        let last_request = Arc::new(Mutex::new(None));

        let recorded = last_request.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let recorded = recorded.clone();
                let response = response.clone();
                thread::spawn(move || handle(stream, response, recorded));
            }
        });

        Self { port, last_request }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/coverart", self.port)
    }

    /// The most recent request, headers included, as received.
    pub fn last_request(&self) -> Option<String> {
        self.last_request.lock().clone()
    }
}

fn handle(
    mut stream: TcpStream,
    response: Option<(u16, String)>,
    recorded: Arc<Mutex<Option<String>>>,
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
    *recorded.lock() = Some(String::from_utf8_lossy(&request).into_owned());

    let Some((status, body)) = response else {
        // Hold the connection open with no answer, so the client times out.
        thread::sleep(std::time::Duration::from_secs(30));
        return;
    };

    let reason = if (200..300).contains(&status) { "OK" } else { "Error" };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}
