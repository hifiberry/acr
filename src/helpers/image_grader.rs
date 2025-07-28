use log::debug;

/// Image blacklist entry for identifying unwanted images
#[derive(Debug, Clone)]
pub struct BlacklistEntry {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<String>,
    pub size_bytes: Option<u64>,
    pub provider: Option<String>,
    pub url_contains: Option<String>, // Match URLs containing this substring
    pub penalty: i32,
    pub description: String,
}

impl BlacklistEntry {
    /// Check if an image matches this blacklist entry
    pub fn matches(&self, info: &ImageInfo) -> bool {
        // All specified criteria must match for a blacklist hit
        if let Some(required_width) = self.width {
            if info.width != Some(required_width) {
                return false;
            }
        }
        
        if let Some(required_height) = self.height {
            if info.height != Some(required_height) {
                return false;
            }
        }
        
        if let Some(ref required_format) = self.format {
            match &info.format {
                Some(format) => {
                    if format.to_lowercase() != required_format.to_lowercase() {
                        return false;
                    }
                }
                None => return false,
            }
        }
        
        if let Some(required_size) = self.size_bytes {
            if info.size_bytes != Some(required_size) {
                return false;
            }
        }
        
        if let Some(ref required_provider) = self.provider {
            if info.provider.to_lowercase() != required_provider.to_lowercase() {
                return false;
            }
        }
        
        if let Some(ref url_pattern) = self.url_contains {
            if !info.url.contains(url_pattern) {
                return false;
            }
        }
        
        true
    }
}

/// Image grader for evaluating cover art quality
/// Provides scoring based on provider, size, resolution, and blacklist checking
#[derive(Debug, Clone)]
pub struct ImageGrader {
    blacklist: Vec<BlacklistEntry>,
}

/// Represents image metadata for grading
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: Option<u64>,
    pub format: Option<String>,
    pub provider: String,
}

/// Result of image grading including the score and breakdown
#[derive(Debug, Clone)]
pub struct ImageGrade {
    pub score: i32,
    pub provider_score: i32,
    pub size_score: i32,
    pub resolution_score: i32,
    pub blacklist_penalty: i32,
    pub blacklist_reason: Option<String>,
}

impl ImageGrader {
    /// Create a new image grader instance with default blacklist
    pub fn new() -> Self {
        let mut grader = ImageGrader {
            blacklist: Vec::new(),
        };
        
        // Add default blacklist entries
        grader.add_default_blacklist_entries();
        grader
    }
    
    /// Add default blacklist entries for known unwanted images
    fn add_default_blacklist_entries(&mut self) {
        // LastFM placeholder images - match by resolution, size, format, and provider only
        // This catches the placeholder regardless of URL structure or artist
        
        // 300x300 version (~4195 bytes)
        self.blacklist.push(BlacklistEntry {
            width: Some(300),
            height: Some(300),
            format: Some("PNG".to_string()),
            size_bytes: Some(4195),
            provider: Some("lastfm".to_string()),
            url_contains: None, // Don't use URL matching
            penalty: -100,
            description: "LastFM placeholder image 300x300 PNG".to_string(),
        });
        
        // 174x174 version (~2225 bytes)
        self.blacklist.push(BlacklistEntry {
            width: Some(174),
            height: Some(174),
            format: Some("PNG".to_string()),
            size_bytes: Some(2225),
            provider: Some("lastfm".to_string()),
            url_contains: None,
            penalty: -100,
            description: "LastFM placeholder image 174x174 PNG".to_string(),
        });
        
        // 64x64 version (~889 bytes)
        self.blacklist.push(BlacklistEntry {
            width: Some(64),
            height: Some(64),
            format: Some("PNG".to_string()),
            size_bytes: Some(889),
            provider: Some("lastfm".to_string()),
            url_contains: None,
            penalty: -100,
            description: "LastFM placeholder image 64x64 PNG".to_string(),
        });
        
        // 34x34 version (~520 bytes)
        self.blacklist.push(BlacklistEntry {
            width: Some(34),
            height: Some(34),
            format: Some("PNG".to_string()),
            size_bytes: Some(520),
            provider: Some("lastfm".to_string()),
            url_contains: None,
            penalty: -100,
            description: "LastFM placeholder image 34x34 PNG".to_string(),
        });
        
        debug!("Added {} blacklist entries", self.blacklist.len());
    }
    
    /// Add a custom blacklist entry
    pub fn add_blacklist_entry(&mut self, entry: BlacklistEntry) {
        self.blacklist.push(entry);
    }
    
    /// Grade an image based on provider, size, resolution, and blacklist
    /// 
    /// # Arguments
    /// * `info` - Image information to grade
    /// 
    /// # Returns
    /// * `ImageGrade` - Detailed grading result
    pub fn grade_image(&self, info: &ImageInfo) -> ImageGrade {
        let provider_score = self.grade_provider(&info.provider);
        let size_score = self.grade_size(info.size_bytes);
        let resolution_score = self.grade_resolution(info.width, info.height);
        let (blacklist_penalty, blacklist_reason) = self.check_blacklist(info);
        
        let total_score = provider_score + size_score + resolution_score + blacklist_penalty;
        
        if blacklist_penalty < 0 {
            debug!(
                "Graded image from {}: total={} (provider={}, size={}, resolution={}, blacklist={}): BLACKLISTED - {}",
                info.provider, total_score, provider_score, size_score, resolution_score, blacklist_penalty,
                blacklist_reason.as_ref().unwrap_or(&"Unknown reason".to_string())
            );
        } else {
            debug!(
                "Graded image from {}: total={} (provider={}, size={}, resolution={})",
                info.provider, total_score, provider_score, size_score, resolution_score
            );
        }
        
        ImageGrade {
            score: total_score,
            provider_score,
            size_score,
            resolution_score,
            blacklist_penalty,
            blacklist_reason,
        }
    }
    
    /// Grade based on provider quality
    /// 
    /// # Grading Rules:
    /// * Spotify: +2
    /// * TheAudioDB: +3  
    /// * FanArt.tv: +4
    /// * LastFM: 1
    /// * Unknown: 0
    fn grade_provider(&self, provider: &str) -> i32 {
        match provider.to_lowercase().as_str() {
            "spotify" => 2,
            "theaudiodb" => 3,
            "fanarttv" | "fanart.tv" => 4,
            "lastfm" | "last.fm" => 1,
            _ => {
                debug!("Unknown provider '{}', assigning score 0", provider);
                0
            }
        }
    }
    
    /// Grade based on file size in bytes
    /// 
    /// # Grading Rules:
    /// * < 10KB: -1 (too small, likely low quality)
    /// * > 100KB: +1 (good size, likely high quality)
    /// * 10KB-100KB: 0 (neutral)
    /// * Unknown size: 0 (neutral)
    fn grade_size(&self, size_bytes: Option<u64>) -> i32 {
        match size_bytes {
            Some(size) => {
                if size < 10_240 {  // < 10KB
                    -1
                } else if size > 102_400 {  // > 100KB
                    1
                } else {
                    0  // 10KB-100KB range is neutral
                }
            }
            None => {
                debug!("No size information available, assigning neutral score");
                0
            }
        }
    }
    
    /// Grade based on image resolution
    /// 
    /// # Grading Rules:
    /// * < 100x100: -1 (very small)
    /// * < 300x300: 0 (small)
    /// * > 600x600: +2 (good)
    /// * > 1000x1000: +3 (excellent)
    /// * 300x300-600x600: 0 (neutral)
    /// * Unknown resolution: 0 (neutral)
    fn grade_resolution(&self, width: Option<u32>, height: Option<u32>) -> i32 {
        match (width, height) {
            (Some(w), Some(h)) => {
                // Use the smaller dimension for grading (ensures both dimensions meet criteria)
                let min_dimension = w.min(h);
                
                if min_dimension < 100 {
                    -1  // Very small
                } else if min_dimension < 300 {
                    0  // Small
                } else if min_dimension > 1000 {
                    3   // Excellent
                } else if min_dimension > 600 {
                    2   // Good
                } else {
                    1   // Neutral (300-600 range)
                }
            }
            _ => {
                debug!("No resolution information available, assigning neutral score");
                0
            }
        }
    }
    
    /// Check if an image matches any blacklist entry
    /// 
    /// # Arguments
    /// * `info` - Image information to check
    /// 
    /// # Returns
    /// * Tuple of (penalty, reason) - penalty is 0 if not blacklisted, negative if blacklisted
    fn check_blacklist(&self, info: &ImageInfo) -> (i32, Option<String>) {
        for entry in &self.blacklist {
            if entry.matches(info) {
                debug!(
                    "Image blacklisted: {} ({}x{}, {}, {} bytes) - {}",
                    info.url,
                    info.width.unwrap_or(0),
                    info.height.unwrap_or(0),
                    info.format.as_ref().unwrap_or(&"unknown".to_string()),
                    info.size_bytes.unwrap_or(0),
                    entry.description
                );
                return (entry.penalty, Some(entry.description.clone()));
            }
        }
        (0, None)
    }
    
    /// Convenience method to grade multiple images and sort by score (highest first)
    /// 
    /// # Arguments
    /// * `images` - Vector of image info to grade and sort
    /// 
    /// # Returns
    /// * Vector of tuples containing (ImageInfo, ImageGrade) sorted by score descending
    pub fn grade_and_sort_images(&self, images: Vec<ImageInfo>) -> Vec<(ImageInfo, ImageGrade)> {
        let mut graded_images: Vec<(ImageInfo, ImageGrade)> = images
            .into_iter()
            .map(|info| {
                let grade = self.grade_image(&info);
                (info, grade)
            })
            .collect();
        
        // Sort by score descending (highest quality first)
        graded_images.sort_by(|a, b| b.1.score.cmp(&a.1.score));
        
        graded_images
    }
}

impl Default for ImageGrader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_grading() {
        let grader = ImageGrader::new();
        
        assert_eq!(grader.grade_provider("spotify"), 2);
        assert_eq!(grader.grade_provider("Spotify"), 2);
        assert_eq!(grader.grade_provider("theaudiodb"), 3);
        assert_eq!(grader.grade_provider("TheAudioDB"), 3);
        assert_eq!(grader.grade_provider("fanarttv"), 4);
        assert_eq!(grader.grade_provider("fanart.tv"), 4);
        assert_eq!(grader.grade_provider("FanArt.tv"), 4);
        assert_eq!(grader.grade_provider("lastfm"), 1);
        assert_eq!(grader.grade_provider("last.fm"), 1);
        assert_eq!(grader.grade_provider("unknown"), 0);
    }

    #[test]
    fn test_size_grading() {
        let grader = ImageGrader::new();
        
        // < 10KB
        assert_eq!(grader.grade_size(Some(5_000)), -1);
        assert_eq!(grader.grade_size(Some(10_239)), -1);
        
        // 10KB-100KB (neutral)
        assert_eq!(grader.grade_size(Some(10_240)), 0);
        assert_eq!(grader.grade_size(Some(50_000)), 0);
        assert_eq!(grader.grade_size(Some(102_400)), 0);
        
        // > 100KB
        assert_eq!(grader.grade_size(Some(102_401)), 1);
        assert_eq!(grader.grade_size(Some(500_000)), 1);
        
        // Unknown size
        assert_eq!(grader.grade_size(None), 0);
    }

    #[test]
    fn test_resolution_grading() {
        let grader = ImageGrader::new();
        
        // < 100x100
        assert_eq!(grader.grade_resolution(Some(50), Some(50)), -1);
        assert_eq!(grader.grade_resolution(Some(99), Some(150)), -1); // min dimension < 100
        
        // < 300x300
        assert_eq!(grader.grade_resolution(Some(200), Some(200)), 0);
        assert_eq!(grader.grade_resolution(Some(299), Some(400)), 0); // min dimension < 300
        
        // 300-600 (neutral)
        assert_eq!(grader.grade_resolution(Some(400), Some(400)), 1);
        assert_eq!(grader.grade_resolution(Some(300), Some(600)), 1);
        
        // > 600x600
        assert_eq!(grader.grade_resolution(Some(700), Some(700)), 2);
        assert_eq!(grader.grade_resolution(Some(600), Some(800)), 1); // min dimension = 600, gets score 1 for 300-600 range
        assert_eq!(grader.grade_resolution(Some(601), Some(800)), 2); // min dimension > 600
        
        // > 1000x1000
        assert_eq!(grader.grade_resolution(Some(1200), Some(1200)), 3);
        assert_eq!(grader.grade_resolution(Some(1000), Some(1500)), 2); // min dimension = 1000, gets score 2 for > 600
        assert_eq!(grader.grade_resolution(Some(1001), Some(1500)), 3); // min dimension > 1000
        
        // Unknown resolution
        assert_eq!(grader.grade_resolution(None, None), 0);
        assert_eq!(grader.grade_resolution(Some(500), None), 0);
        assert_eq!(grader.grade_resolution(None, Some(500)), 0);
    }

    #[test]
    fn test_complete_grading() {
        let grader = ImageGrader::new();
        
        let high_quality_image = ImageInfo {
            url: "https://example.com/high_quality.jpg".to_string(),
            width: Some(1200),
            height: Some(1200),
            size_bytes: Some(300_000),
            format: Some("JPEG".to_string()),
            provider: "fanarttv".to_string(),
        };
        
        let grade = grader.grade_image(&high_quality_image);
        // fanarttv(+4) + >100KB(+1) + >1000x1000(+3) = 8
        assert_eq!(grade.score, 8);
        assert_eq!(grade.provider_score, 4);
        assert_eq!(grade.size_score, 1);
        assert_eq!(grade.resolution_score, 3);
        
        let low_quality_image = ImageInfo {
            url: "https://example.com/low_quality.jpg".to_string(),
            width: Some(50),
            height: Some(50),
            size_bytes: Some(5_000),
            format: Some("JPEG".to_string()),
            provider: "unknown".to_string(),
        };
        
        let grade = grader.grade_image(&low_quality_image);
        // unknown(0) + <10KB(-1) + <100x100(-1) = -2
        assert_eq!(grade.score, -2);
        assert_eq!(grade.provider_score, 0);
        assert_eq!(grade.size_score, -1);
        assert_eq!(grade.resolution_score, -1);
    }

    #[test]
    fn test_grade_and_sort() {
        let grader = ImageGrader::new();
        
        let images = vec![
            ImageInfo {
                url: "low.jpg".to_string(),
                width: Some(100),
                height: Some(100),
                size_bytes: Some(5_000),
                format: Some("JPEG".to_string()),
                provider: "spotify".to_string(),
            },
            ImageInfo {
                url: "high.jpg".to_string(),
                width: Some(1200),
                height: Some(1200),
                size_bytes: Some(300_000),
                format: Some("JPEG".to_string()),
                provider: "fanarttv".to_string(),
            },
            ImageInfo {
                url: "medium.jpg".to_string(),
                width: Some(500),
                height: Some(500),
                size_bytes: Some(50_000),
                format: Some("JPEG".to_string()),
                provider: "theaudiodb".to_string(),
            },
        ];
        
        let graded = grader.grade_and_sort_images(images);
        
        // Should be sorted by score descending
        assert_eq!(graded[0].0.url, "high.jpg"); // Score: fanarttv(4) + >100KB(1) + >1000x1000(3) = 8
        assert_eq!(graded[1].0.url, "medium.jpg"); // Score: theaudiodb(3) + 10-100KB(0) + 300-600(1) = 4  
        assert_eq!(graded[2].0.url, "low.jpg"); // Score: spotify(2) + <10KB(-1) + <300x300(0) = 1
        
        assert!(graded[0].1.score > graded[1].1.score);
        assert!(graded[1].1.score > graded[2].1.score);
    }

    #[test]
    fn test_blacklist_functionality() {
        let grader = ImageGrader::new();
        
        // Test the 300x300 LastFM placeholder image that should be blacklisted
        // Use a different URL to test that URL doesn't matter - only dimensions, size, format, provider
        let blacklisted_300x300 = ImageInfo {
            url: "https://lastfm.freetls.fastly.net/i/u/300x300/some_other_artist.png".to_string(),
            width: Some(300),
            height: Some(300),
            size_bytes: Some(4195),
            format: Some("PNG".to_string()),
            provider: "lastfm".to_string(),
        };
        
        let grade = grader.grade_image(&blacklisted_300x300);
        assert_eq!(grade.blacklist_penalty, -100);
        assert!(grade.blacklist_reason.is_some());
        assert!(grade.blacklist_reason.unwrap().contains("LastFM placeholder"));
        
        // Test the 174x174 variant with completely different URL
        let blacklisted_174x174 = ImageInfo {
            url: "https://different.domain.com/artist123/cover.png".to_string(),
            width: Some(174),
            height: Some(174),
            size_bytes: Some(2225),
            format: Some("PNG".to_string()),
            provider: "lastfm".to_string(),
        };
        
        let grade2 = grader.grade_image(&blacklisted_174x174);
        assert_eq!(grade2.blacklist_penalty, -100);
        assert!(grade2.blacklist_reason.is_some());
        
        // Test the 64x64 variant
        let blacklisted_64x64 = ImageInfo {
            url: "https://lastfm.example.org/cover/64x64.png".to_string(),
            width: Some(64),
            height: Some(64),
            size_bytes: Some(889),
            format: Some("PNG".to_string()),
            provider: "lastfm".to_string(),
        };
        
        let grade3 = grader.grade_image(&blacklisted_64x64);
        assert_eq!(grade3.blacklist_penalty, -100);
        assert!(grade3.blacklist_reason.is_some());
        
        // Test the 34x34 variant
        let blacklisted_34x34 = ImageInfo {
            url: "https://cdn.lastfm.net/small/artist.png".to_string(),
            width: Some(34),
            height: Some(34),
            size_bytes: Some(520),
            format: Some("PNG".to_string()),
            provider: "lastfm".to_string(),
        };
        
        let grade4 = grader.grade_image(&blacklisted_34x34);
        assert_eq!(grade4.blacklist_penalty, -100);
        assert!(grade4.blacklist_reason.is_some());
        
        // Test image with same dimensions/format but different size - should NOT be blacklisted
        let similar_but_different_size = ImageInfo {
            url: "https://lastfm.freetls.fastly.net/i/u/300x300/real_album_cover.png".to_string(),
            width: Some(300),
            height: Some(300),
            size_bytes: Some(50000), // Different size
            format: Some("PNG".to_string()),
            provider: "lastfm".to_string(),
        };
        
        let grade_different_size = grader.grade_image(&similar_but_different_size);
        assert_eq!(grade_different_size.blacklist_penalty, 0);
        assert!(grade_different_size.blacklist_reason.is_none());
        
        // Test similar image from different provider - should NOT be blacklisted
        let similar_but_not_blacklisted = ImageInfo {
            url: "https://example.com/300x300.png".to_string(),
            width: Some(300),
            height: Some(300),
            size_bytes: Some(4195), // Even same size
            format: Some("PNG".to_string()),
            provider: "spotify".to_string(), // Different provider
        };
        
        let grade_clean = grader.grade_image(&similar_but_not_blacklisted);
        
        // Should NOT have blacklist penalty
        assert_eq!(grade_clean.blacklist_penalty, 0);
        assert!(grade_clean.blacklist_reason.is_none());
        
        // Total score should be positive without blacklist penalty
        // spotify(2) + <10KB(-1) + 300x300(1) + no blacklist(0) = 2
        assert_eq!(grade_clean.score, 2);
    }
}
