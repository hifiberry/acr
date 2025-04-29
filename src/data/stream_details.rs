/// Stream format details representing audio format information
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StreamDetails {
    /// Sample rate in Hz (e.g., 44100, 48000)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    
    /// Bits per sample (e.g., 16, 24, 32)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits_per_sample: Option<u8>,
    
    /// Number of audio channels (e.g., 1 for mono, 2 for stereo)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<u8>,
    
    /// Type of sample encoding (e.g., "pcm", "dsd", "mqa")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_type: Option<String>,
    
    /// Indicates if the stream is lossless or lossy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lossless: Option<bool>,
}

impl StreamDetails {
    /// Create a new empty StreamDetails
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Calculate bits per second (bitrate) if sample information is available
    /// Returns None if any required information is missing
    pub fn bitrate(&self) -> Option<u64> {
        if let (Some(rate), Some(bits), Some(channels)) = 
            (self.sample_rate, self.bits_per_sample, self.channels) {
            Some(u64::from(rate) * u64::from(bits) * u64::from(channels))
        } else {
            None
        }
    }
    
    /// Create a human-readable description of the stream format
    pub fn format_description(&self) -> String {
        let mut parts = Vec::new();
        
        // Add sample rate if available
        if let Some(rate) = self.sample_rate {
            if rate >= 1000 {
                parts.push(format!("{:.1} kHz", rate as f32 / 1000.0));
            } else {
                parts.push(format!("{} Hz", rate));
            }
        }
        
        // Add bit depth and sample type if available
        if let Some(bits) = self.bits_per_sample {
            if let Some(sample_type) = &self.sample_type {
                if sample_type.eq_ignore_ascii_case("pcm") {
                    parts.push(format!("{}-bit", bits));
                } else {
                    parts.push(format!("{}-bit {}", bits, sample_type.to_uppercase()));
                }
            } else {
                parts.push(format!("{}-bit", bits));
            }
        } else if let Some(sample_type) = &self.sample_type {
            parts.push(sample_type.to_uppercase());
        }
        
        // Add channel information
        if let Some(channels) = self.channels {
            match channels {
                1 => parts.push("Mono".to_string()),
                2 => parts.push("Stereo".to_string()),
                _ => parts.push(format!("{} channels", channels)),
            }
        }
        
        // Add lossless indicator
        if let Some(lossless) = self.lossless {
            parts.push(if lossless { "Lossless".to_string() } else { "Lossy".to_string() });
        }
        
        // Join all parts with spaces
        parts.join(" ")
    }
}