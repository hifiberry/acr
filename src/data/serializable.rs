/// Trait for objects that can be serialized to JSON
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;

/// Error type for serialization/deserialization operations
#[derive(Debug)]
pub enum SerializationError {
    /// Error during serialization/deserialization
    SerdeError(String),
}

impl std::fmt::Display for SerializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializationError::SerdeError(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for SerializationError {}

pub trait Serializable: Serialize {
    /// Convert the object to a JSON string representation
    ///
    /// Returns:
    ///     JSON string representation of the object
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    
    /// Convert the object to a pretty-printed JSON string representation
    ///
    /// Returns:
    ///     Pretty-printed JSON string representation of the object
    fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

// Implement Serializable for all types that implement Serialize
impl<T: Serialize> Serializable for T {}

/// Trait for objects that can be deserialized from JSON
pub trait Deserializable: DeserializeOwned + Sized {
    /// Create an object from a JSON string
    ///
    /// # Arguments
    ///
    /// * `json` - JSON string to deserialize
    ///
    /// # Returns
    ///
    /// * `Result<Self, SerializationError>` - The deserialized object or an error
    fn from_json(json: &str) -> Result<Self, SerializationError> {
        serde_json::from_str(json)
            .map_err(|e| SerializationError::SerdeError(e.to_string()))
    }
}

// Implement Deserializable for all types that implement DeserializeOwned
impl<T: DeserializeOwned> Deserializable for T {}