use std::time::Duration;
use std::io::Read;
use log::{debug, error, warn};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

/// ureq's own default, restated so that disabling redirects for one case
/// does not quietly change the limit for the other.
const DEFAULT_REDIRECT_LIMIT: u32 = 5;

/// Error types that can occur when interacting with HTTP clients
#[derive(Debug, Error)]
pub enum HttpClientError {
    #[error("HTTP request error: {0}")]
    RequestError(String),

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Empty response from server")]
    EmptyResponse,
}

/// A trait for HTTP client implementations
/// This version avoids generic methods to enable dynamic dispatch
pub trait HttpClient: Send + Sync + std::fmt::Debug {
    /// Send a POST request with a JSON payload
    fn post_json_value(&self, url: &str, payload: Value) -> Result<Value, HttpClientError>;
    
    /// Send a GET request and return text response
    fn get_text(&self, url: &str) -> Result<String, HttpClientError>;
    
    /// Send a GET request and return binary data with mimetype
    fn get_binary(&self, url: &str) -> Result<(Vec<u8>, String), HttpClientError>;

    /// Send a GET request with headers and return binary data with mimetype,
    /// refusing a body larger than `max_bytes`.
    ///
    /// When any header is supplied, redirects are **not** followed: a header
    /// a caller attached for one host must not be re-sent to another that a
    /// redirect chose. With no headers, redirects are followed normally.
    ///
    /// `get_binary` above takes no headers and reads with an unbounded
    /// `read_to_end`. Neither is acceptable for fetching an image named by a
    /// configured external endpoint: the credential that authorised the
    /// lookup has to authorise the image fetch too, and a service that
    /// answers with an endless body must not be able to exhaust the daemon's
    /// memory. The bound is applied to the read, not checked after it.
    fn get_binary_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        max_bytes: u64,
    ) -> Result<(Vec<u8>, String), HttpClientError>;

    /// Send a GET request with headers and return JSON value
    fn get_json_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> Result<Value, HttpClientError>;
    
    /// Send a POST request with a JSON payload and custom headers
    fn post_json_value_with_headers(&self, url: &str, payload: Value, headers: &[(&str, &str)]) -> Result<Value, HttpClientError>;
    
    /// Send a PUT request with a JSON payload and custom headers
    fn put_json_value_with_headers(&self, url: &str, payload: Value, headers: &[(&str, &str)]) -> Result<Value, HttpClientError>;
    
    /// Clone the client as a boxed trait object
    fn clone_box(&self) -> Box<dyn HttpClient>;
}

// Non-generic helper function to serialize and post JSON
pub fn post_json<T: Serialize>(
    client: &dyn HttpClient, 
    url: &str, 
    payload: &T
) -> Result<Value, HttpClientError> {
    match serde_json::to_value(payload) {
        Ok(value) => client.post_json_value(url, value),
        Err(e) => Err(HttpClientError::ParseError(format!("Failed to serialize payload: {}", e)))
    }
}

impl Clone for Box<dyn HttpClient> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// An HTTP client implementation using ureq
#[derive(Clone, Debug)]
pub struct UreqHttpClient {
    timeout: Duration,
}

impl Default for UreqHttpClient {
    fn default() -> Self {
        Self::new(5)
    }
}

impl UreqHttpClient {
    /// Create a new HTTP client with the specified timeout
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs),
        }
    }
}

impl HttpClient for UreqHttpClient {
    fn post_json_value(&self, url: &str, payload: Value) -> Result<Value, HttpClientError> {
        debug!("POST request to {}", url);
        debug!("POST payload: {}", payload);
        
        // First serialize the JSON value to a string
        let json_string = match serde_json::to_string(&payload) {
            Ok(str) => str,
            Err(e) => {
                debug!("Failed to serialize JSON payload: {}", e);
                return Err(HttpClientError::ParseError(format!("Failed to serialize JSON payload: {}", e)));
            }
        };
        
        // Use the ureq API correctly
        let response = match ureq::post(url)
            .timeout(self.timeout)
            .set("Content-Type", "application/json")
            .send_string(&json_string)
        {
            Ok(resp) => resp,
            Err(e) => {
                debug!("POST request failed: {}", e);
                debug!("POST payload was: {}", json_string);
                return Err(HttpClientError::RequestError(e.to_string()));
            }
        };
        
        let response_text = match response.into_string() {
            Ok(text) => text,
            Err(e) => {
                debug!("Failed to read response body: {}", e);
                return Err(HttpClientError::ParseError(format!("Failed to read response body: {}", e)));
            }
        };
        
        if response_text.is_empty() {
            return Err(HttpClientError::EmptyResponse);
        }
        
        match serde_json::from_str::<Value>(&response_text) {
            Ok(json_value) => Ok(json_value),
            Err(e) => {
                debug!("Failed to parse JSON response: {}", e);
                debug!("Response text: {}", response_text);
                Err(HttpClientError::ParseError(e.to_string()))
            }
        }
    }
    
    fn get_text(&self, url: &str) -> Result<String, HttpClientError> {
        debug!("GET text request to {}", url);
        
        let response = match ureq::get(url).timeout(self.timeout).call() {
            Ok(resp) => resp,
            Err(e) => {
                debug!("GET request failed: {}", e);
                return Err(HttpClientError::RequestError(e.to_string()));
            }
        };
        
        match response.into_string() {
            Ok(text) => Ok(text),
            Err(e) => {
                debug!("Failed to read response body: {}", e);
                Err(HttpClientError::ParseError(format!("Failed to read response body: {}", e)))
            }
        }
    }
    
    fn get_binary(&self, url: &str) -> Result<(Vec<u8>, String), HttpClientError> {
        debug!("GET binary request to {}", url);
        
        let response = match ureq::get(url).timeout(self.timeout).call() {
            Ok(resp) => resp,
            Err(e) => {
                debug!("GET binary request failed: {}", e);
                return Err(HttpClientError::RequestError(e.to_string()));
            }
        };
        
        // Get the content-type header or default to "application/octet-stream"
        let content_type = response
            .header("content-type")
            .unwrap_or("application/octet-stream")
            .to_string();
            
        // Get the response body as bytes
        let mut bytes: Vec<u8> = Vec::new();
        match response.into_reader().read_to_end(&mut bytes) {
            Ok(_) => Ok((bytes, content_type)),
            Err(e) => {
                debug!("Failed to read binary response: {}", e);
                Err(HttpClientError::ParseError(format!("Failed to read binary response: {}", e)))
            }
        }
    }
    
    fn get_binary_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        max_bytes: u64,
    ) -> Result<(Vec<u8>, String), HttpClientError> {
        debug!("GET binary request with headers to {}", url);

        // Caller-supplied headers do not survive a redirect, because they
        // cannot be trusted to. ureq's default is to follow up to five, and
        // it strips only `content-length`, `cookie` and `authorization` on
        // the way -- every other name is re-sent to whatever host the
        // redirect names. A caller that checked the destination before
        // calling would have that check bypassed by a 302, and a credential
        // in any header not literally called `Authorization` would go to a
        // host it never approved. Following redirects is fine when there is
        // nothing attached to leak.
        //
        // The redirect limit belongs to the agent rather than the request,
        // so the agent is built per call.
        let agent = ureq::AgentBuilder::new()
            .redirects(if headers.is_empty() { DEFAULT_REDIRECT_LIMIT } else { 0 })
            .build();

        let mut request = agent.get(url).timeout(self.timeout);

        for &(name, value) in headers {
            request = request.set(name, value);
        }

        let response = match request.call() {
            Ok(resp) => resp,
            Err(e) => {
                debug!("GET binary request with headers failed: {}", e);
                return Err(HttpClientError::RequestError(e.to_string()));
            }
        };

        // A 3xx arrives here as a normal response rather than an error,
        // because the redirect limit above is zero. Nothing further logs it:
        // the body will not sniff as an image, so the image is dropped, and
        // the only other message is an aggregate warning that fires solely
        // when every image in an answer failed. Since refusing the redirect
        // is a deliberate break of a configuration that used to work, say so
        // here, or an operator has nothing to grep for.
        if !headers.is_empty() && (300..400).contains(&response.status()) {
            warn!(
                "GET {} answered {} but redirects are not followed while headers are attached, so the credential is not re-sent; the response will not be usable as an image",
                url,
                response.status()
            );
        }

        let content_type = response
            .header("content-type")
            .unwrap_or("application/octet-stream")
            .to_string();

        // Read one byte past the cap: if it arrives, the body is over the
        // limit, and we know that without having buffered the rest of it.
        let mut bytes: Vec<u8> = Vec::new();
        let mut reader = response.into_reader().take(max_bytes.saturating_add(1));
        if let Err(e) = reader.read_to_end(&mut bytes) {
            debug!("Failed to read binary response: {}", e);
            return Err(HttpClientError::ParseError(format!(
                "Failed to read binary response: {}",
                e
            )));
        }

        if bytes.len() as u64 > max_bytes {
            return Err(HttpClientError::ParseError(format!(
                "Response body exceeds the {} byte limit",
                max_bytes
            )));
        }

        Ok((bytes, content_type))
    }

    fn clone_box(&self) -> Box<dyn HttpClient> {
        Box::new(self.clone())
    }

    fn get_json_with_headers(&self, url: &str, headers: &[(&str, &str)]) -> Result<Value, HttpClientError> {
        debug!("GET JSON request with headers to {}", url);
        
        let mut request = ureq::get(url).timeout(self.timeout);
        
        // Add all headers to the request
        for &(name, value) in headers {
            debug!("Adding header '{}': '{}'", name, if name == "Authorization" { 
                // Don't log full auth token but show the first few characters
                if value.len() > 15 {
                    format!("{}...", &value[0..15])
                } else {
                    "[hidden]".to_string()
                }
            } else { 
                value.to_string() 
            });
            request = request.set(name, value);
        }
        
        // Send the request
        let response = match request.call() {
            Ok(resp) => {
                debug!("GET request with headers succeeded with status: {}", resp.status());
                resp
            },
            Err(e) => {
                // Check if it's a ureq::Error::Status with HTTP status code
                match e {
                    ureq::Error::Status(code, response) => {
                        let error_body = response.into_string().unwrap_or_else(|_| "<failed to read response body>".to_string());
                        
                        // Provide more specific error info for authentication issues
                        if code == 401 {
                            error!("HTTP 401 Unauthorized error - check if the X-Proxy-Secret header is correct");
                            error!("HTTP 401 error body: {}", error_body);
                            return Err(HttpClientError::ServerError(format!(
                                "HTTP 401 Unauthorized: Authentication failed. Check that the proxy_secret is correct in secrets.txt and matches what the OAuth service expects. Error: {}", 
                                error_body
                            )));
                        } else {
                            error!("HTTP error {}: {}", code, error_body);
                            return Err(HttpClientError::ServerError(format!("HTTP {} error: {}", code, error_body)));
                        }
                    },
                    _ => {
                        error!("GET request with headers failed: {}", e);
                        return Err(HttpClientError::RequestError(e.to_string()));
                    }
                }
            }
        };
        
        // Get the response as text
        let response_text = match response.into_string() {
            Ok(text) => text,
            Err(e) => {
                debug!("Failed to read response body: {}", e);
                return Err(HttpClientError::ParseError(format!("Failed to read response body: {}", e)));
            }
        };
        
        if response_text.is_empty() {
            return Err(HttpClientError::EmptyResponse);
        }
        
        // Parse the response as JSON
        match serde_json::from_str::<Value>(&response_text) {
            Ok(json_value) => Ok(json_value),
            Err(e) => {
                // Log the actual response content (truncated if too large)
                let truncated_response = if response_text.len() > 500 {
                    format!("{}... (truncated, total length: {} bytes)", &response_text[0..500], response_text.len())
                } else {
                    response_text.clone()
                };
                error!("Failed to parse JSON response: {}", e);
                error!("Response content: {}", truncated_response);
                // Try to determine if it might be HTML instead of JSON
                if response_text.contains("<html") || response_text.contains("<!DOCTYPE") {
                    error!("Response appears to be HTML instead of JSON - check if the OAuth URL is correct");
                    return Err(HttpClientError::ParseError("Response is HTML instead of expected JSON. The OAuth service might be returning an error page.".to_string()));
                }
                Err(HttpClientError::ParseError(format!("Failed to parse response: {}", e)))
            }
        }
    }
    
    fn post_json_value_with_headers(&self, url: &str, payload: Value, headers: &[(&str, &str)]) -> Result<Value, HttpClientError> {
        debug!("POST request with headers to {}", url);
        debug!("POST payload: {}", payload);

        // Serialize the JSON value to a string
        let json_string = match serde_json::to_string(&payload) {
            Ok(str) => str,
            Err(e) => {
                debug!("Failed to serialize JSON payload: {}", e);
                return Err(HttpClientError::ParseError(format!("Failed to serialize JSON payload: {}", e)));
            }
        };

        let mut request = ureq::post(url).timeout(self.timeout);
        for &(name, value) in headers {
            debug!("Adding header '{}': '{}'", name, if name == "Authorization" {
                if value.len() > 15 { format!("{}...", &value[0..15]) } else { "[hidden]".to_string() }
            } else { value.to_string() });
            request = request.set(name, value);
        }

        let response = match request.send_string(&json_string) {
            Ok(resp) => resp,
            Err(e) => {
                debug!("POST request with headers failed: {}", e);
                debug!("POST payload was: {}", json_string);
                return Err(HttpClientError::RequestError(e.to_string()));
            }
        };

        let response_text = match response.into_string() {
            Ok(text) => text,
            Err(e) => {
                debug!("Failed to read response body: {}", e);
                return Err(HttpClientError::ParseError(format!("Failed to read response body: {}", e)));
            }
        };

        if response_text.is_empty() {
            return Err(HttpClientError::EmptyResponse);
        }

        match serde_json::from_str::<Value>(&response_text) {
            Ok(json_value) => Ok(json_value),
            Err(e) => {
                debug!("Failed to parse JSON response: {}", e);
                debug!("Response text: {}", response_text);
                Err(HttpClientError::ParseError(e.to_string()))
            }
        }
    }
    
    fn put_json_value_with_headers(&self, url: &str, payload: Value, headers: &[(&str, &str)]) -> Result<Value, HttpClientError> {
        debug!("PUT request with headers to {}", url);
        debug!("PUT payload: {}", payload);

        // Serialize the JSON value to a string
        let json_string = match serde_json::to_string(&payload) {
            Ok(str) => str,
            Err(e) => {
                debug!("Failed to serialize JSON payload: {}", e);
                return Err(HttpClientError::ParseError(format!("Failed to serialize JSON payload: {}", e)));
            }
        };

        let mut request = ureq::put(url).timeout(self.timeout);
        for &(name, value) in headers {
            debug!("Adding header '{}': '{}'", name, if name == "Authorization" {
                if value.len() > 15 { format!("{}...", &value[0..15]) } else { "[hidden]".to_string() }
            } else { value.to_string() });
            request = request.set(name, value);
        }

        let response = match request.send_string(&json_string) {
            Ok(resp) => resp,
            Err(e) => {
                debug!("PUT request with headers failed: {}", e);
                debug!("PUT payload was: {}", json_string);
                return Err(HttpClientError::RequestError(e.to_string()));
            }
        };

        let response_text = match response.into_string() {
            Ok(text) => text,
            Err(e) => {
                debug!("Failed to read response body: {}", e);
                return Err(HttpClientError::ParseError(format!("Failed to read response body: {}", e)));
            }
        };

        if response_text.is_empty() {
            return Err(HttpClientError::EmptyResponse);
        }

        match serde_json::from_str::<Value>(&response_text) {
            Ok(json_value) => Ok(json_value),
            Err(e) => {
                debug!("Failed to parse JSON response: {}", e);
                debug!("Response text: {}", response_text);
                Err(HttpClientError::ParseError(e.to_string()))
            }
        }
    }
}

/// Create a new HTTP client using the default implementation
pub fn new_http_client(timeout_secs: u64) -> Box<dyn HttpClient> {
    Box::new(UreqHttpClient::new(timeout_secs))
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A local one-shot server, so the test exercises ureq rather than a mock.
    fn serve_once(status: u16, content_type: &str, body: Vec<u8>) -> (u16, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
        let port = listener.local_addr().expect("a bound address").port();
        let (tx, rx) = std::sync::mpsc::channel();
        let content_type = content_type.to_string();

        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while stream.read_exact(&mut byte).is_ok() {
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&request).into_owned());

            let head = format!(
                "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                status,
                content_type,
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        });

        (port, rx)
    }

    #[test]
    fn a_binary_get_returns_the_body_and_its_content_type() {
        let (port, _rx) = serve_once(200, "image/png", vec![1, 2, 3, 4]);
        let client = UreqHttpClient::new(5);

        let (bytes, mime) = client
            .get_binary_with_headers(&format!("http://127.0.0.1:{}/i.png", port), &[], 1024)
            .expect("the request succeeds");

        assert_eq!(bytes, vec![1, 2, 3, 4]);
        assert_eq!(mime, "image/png");
    }

    /// The whole reason this method exists alongside `get_binary`: the
    /// endpoint's credential has to reach the image host too, or an
    /// authenticated image URL is unfetchable.
    #[test]
    fn a_binary_get_sends_the_headers_it_was_given() {
        let (port, rx) = serve_once(200, "image/jpeg", vec![7; 10]);
        let client = UreqHttpClient::new(5);

        client
            .get_binary_with_headers(
                &format!("http://127.0.0.1:{}/i.jpg", port),
                &[("Authorization", "Bearer sekrit")],
                1024,
            )
            .expect("the request succeeds");

        let request = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("a request arrived");
        assert!(
            request.contains("Authorization: Bearer sekrit"),
            "the credential was not on the wire; got: {}",
            request
        );
    }

    /// The cap has to bound the read itself, not merely reject afterwards: a
    /// service that streams without end must not be able to make the daemon
    /// allocate without end. `read_to_end` on an unbounded body is exactly
    /// the shape this replaces.
    /// Serve a 302 to `location`, and report whether anything ever arrived.
    fn serve_redirect(location: &str) -> u16 {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
        let port = listener.local_addr().expect("a bound address").port();
        let location = location.to_string();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while stream.read_exact(&mut byte).is_ok() {
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let head = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    location
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.flush();
            }
        });

        port
    }

    /// A caller that checked the destination before calling would have that
    /// check bypassed by a redirect. ureq follows up to five by default and
    /// strips only `authorization`, so a credential in any other header name
    /// would be re-sent to whatever host the redirect chose.
    #[test]
    fn a_binary_get_with_headers_does_not_follow_a_redirect() {
        let (target_port, target_rx) = serve_once(200, "image/png", vec![1, 2, 3]);
        let redirect_port = serve_redirect(&format!("http://127.0.0.1:{}/img.png", target_port));
        let client = UreqHttpClient::new(5);

        let _ = client.get_binary_with_headers(
            &format!("http://127.0.0.1:{}/start.png", redirect_port),
            &[("X-Api-Key", "sekrit")],
            1024,
        );

        assert!(
            target_rx
                .recv_timeout(std::time::Duration::from_millis(500))
                .is_err(),
            "the redirect target must never be contacted while headers are attached"
        );
    }

    /// With nothing attached there is nothing to leak, so redirects still
    /// work normally.
    #[test]
    fn a_binary_get_without_headers_still_follows_a_redirect() {
        let (target_port, _rx) = serve_once(200, "image/png", vec![1, 2, 3]);
        let redirect_port = serve_redirect(&format!("http://127.0.0.1:{}/img.png", target_port));
        let client = UreqHttpClient::new(5);

        let (bytes, _) = client
            .get_binary_with_headers(
                &format!("http://127.0.0.1:{}/start.png", redirect_port),
                &[],
                1024,
            )
            .expect("a redirect is followed when no header is attached");

        assert_eq!(bytes, vec![1, 2, 3]);
    }

    #[test]
    fn a_binary_get_refuses_a_body_over_the_cap() {
        let (port, _rx) = serve_once(200, "image/jpeg", vec![0; 5000]);
        let client = UreqHttpClient::new(5);

        let result = client
            .get_binary_with_headers(&format!("http://127.0.0.1:{}/big.jpg", port), &[], 1000);

        // Asserting only `is_err()` would also pass if the request had simply
        // failed to connect, which would let the size check be deleted
        // without this test noticing. Match the error class and the limit.
        let error = result.expect_err("an oversized body must not be returned");
        assert!(
            matches!(&error, HttpClientError::ParseError(message) if message.contains("1000")),
            "expected a refusal naming the byte limit, got: {:?}",
            error
        );
    }

    #[test]
    fn a_binary_get_accepts_a_body_exactly_at_the_cap() {
        let (port, _rx) = serve_once(200, "image/jpeg", vec![0; 1000]);
        let client = UreqHttpClient::new(5);

        let (bytes, _) = client
            .get_binary_with_headers(&format!("http://127.0.0.1:{}/exact.jpg", port), &[], 1000)
            .expect("a body at the cap is allowed");

        assert_eq!(bytes.len(), 1000);
    }
}
