use std::fmt;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use std::hash::{Hash, Hasher};

/// Represents an identifier that can be either numeric or string-based
#[derive(Debug, Clone, Eq)]
pub enum Identifier {
    /// Numeric identifier (e.g., internal hash)
    Numeric(u64),
    /// String identifier (e.g., external ID like MusicBrainz ID)
    String(String),
}

impl Identifier {
    /// Get the numeric value, if available
    pub fn numeric(&self) -> Option<u64> {
        match self {
            Identifier::Numeric(n) => Some(*n),
            _ => None,
        }
    }

    /// Get the string value, if available
    pub fn string(&self) -> Option<&str> {
        match self {
            Identifier::String(s) => Some(s),
            _ => None,
        }
    }
}

// Make Identifier hashable, hashing based on content
impl Hash for Identifier {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Identifier::Numeric(n) => {
                0u8.hash(state); // Tag for numeric
                n.hash(state);
            },
            Identifier::String(s) => {
                1u8.hash(state); // Tag for string
                s.hash(state);
            },
        }
    }
}

// Make Identifier comparable
impl PartialEq for Identifier {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Identifier::Numeric(a), Identifier::Numeric(b)) => a == b,
            (Identifier::String(a), Identifier::String(b)) => a == b,
            // Different types are not equal
            _ => false,
        }
    }
}

// Implement Display for better debugging
impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Identifier::Numeric(n) => write!(f, "{}", n),
            Identifier::String(s) => write!(f, "{}", s),
        }
    }
}

// Implement serialization for Identifier
impl Serialize for Identifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Identifier::Numeric(n) => serializer.serialize_str(&n.to_string()),
            Identifier::String(s) => serializer.serialize_str(s),
        }
    }
}

// Implement deserialization for Identifier
impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // Try to parse as u64 first
        if let Ok(num) = s.parse::<u64>() {
            Ok(Identifier::Numeric(num))
        } else {
            Ok(Identifier::String(s))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both variants go out as a JSON string; a numeric id is not a JSON
    /// number.
    #[test]
    fn both_variants_serialize_as_a_string() {
        assert_eq!(serde_json::to_string(&Identifier::Numeric(42)).unwrap(), r#""42""#);
        assert_eq!(serde_json::to_string(&Identifier::String("abc".to_string())).unwrap(), r#""abc""#);
        assert_eq!(
            serde_json::to_string(&Identifier::Numeric(u64::MAX)).unwrap(),
            r#""18446744073709551615""#
        );
    }

    #[test]
    fn a_string_of_digits_deserializes_as_numeric() {
        let id: Identifier = serde_json::from_str(r#""42""#).unwrap();
        assert_eq!(id, Identifier::Numeric(42));
    }

    #[test]
    fn any_other_string_deserializes_as_a_string() {
        for text in ["abc", "", "-1", "1.5", " 42", "18446744073709551616"] {
            let id: Identifier = serde_json::from_str(&serde_json::to_string(text).unwrap()).unwrap();
            assert_eq!(id, Identifier::String(text.to_string()));
        }
    }

    /// A string id made only of digits comes back as the numeric variant,
    /// which does not compare equal to the one that went out. The wire format
    /// cannot tell the two apart.
    #[test]
    fn a_digit_only_string_id_does_not_round_trip_as_a_string() {
        let sent = Identifier::String("123".to_string());
        let received: Identifier = serde_json::from_str(&serde_json::to_string(&sent).unwrap()).unwrap();
        assert_eq!(received, Identifier::Numeric(123));
        assert_ne!(received, sent);
    }

    #[test]
    fn a_json_number_is_rejected() {
        assert!(serde_json::from_str::<Identifier>("42").is_err());
    }

    #[test]
    fn identifiers_work_as_json_map_keys() {
        let mut map = std::collections::HashMap::new();
        map.insert(Identifier::Numeric(7), "seven");
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(json, r#"{"7":"seven"}"#);

        let back: std::collections::HashMap<Identifier, String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get(&Identifier::Numeric(7)).map(String::as_str), Some("seven"));
    }
}
