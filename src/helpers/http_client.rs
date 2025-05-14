use std::time::Duration;
use std::io::Read;
use log::{debug, error};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

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

impl UreqHttpClient {
    /// Create a new HTTP client with the specified timeout
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs),
        }
    }
    
    /// Create a new HTTP client with default timeout (5 seconds)
    pub fn default() -> Self {
        Self::new(5)
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
    
    fn clone_box(&self) -> Box<dyn HttpClient> {
        Box::new(self.clone())
    }
}

/// Create a new HTTP client using the default implementation
pub fn new_http_client(timeout_secs: u64) -> Box<dyn HttpClient> {
    Box::new(UreqHttpClient::new(timeout_secs))
}