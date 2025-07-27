use log::debug;

/// Image grader for evaluating cover art quality
/// Provides scoring based on provider, size, and resolution
#[derive(Debug, Clone)]
pub struct ImageGrader;

/// Represents image metadata for grading
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: Option<u64>,
    pub provider: String,
}

/// Result of image grading including the score and breakdown
#[derive(Debug, Clone)]
pub struct ImageGrade {
    pub score: i32,
    pub provider_score: i32,
    pub size_score: i32,
    pub resolution_score: i32,
}

impl ImageGrader {
    /// Create a new image grader instance
    pub fn new() -> Self {
        ImageGrader
    }
    
    /// Grade an image based on provider, size, and resolution
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
        
        let total_score = provider_score + size_score + resolution_score;
        
        debug!(
            "Graded image from {}: total={} (provider={}, size={}, resolution={})",
            info.provider, total_score, provider_score, size_score, resolution_score
        );
        
        ImageGrade {
            score: total_score,
            provider_score,
            size_score,
            resolution_score,
        }
    }
    
    /// Grade based on provider quality
    /// 
    /// # Grading Rules:
    /// * Spotify: +1
    /// * TheAudioDB: +2  
    /// * FanArt.tv: +3
    /// * LastFM: 0
    /// * Unknown: 0
    fn grade_provider(&self, provider: &str) -> i32 {
        match provider.to_lowercase().as_str() {
            "spotify" => 1,
            "theaudiodb" => 2,
            "fanarttv" | "fanart.tv" => 3,
            "lastfm" | "last.fm" => 0,
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
    /// * < 100x100: -2 (very small)
    /// * < 300x300: -1 (small)
    /// * > 600x600: +1 (good)
    /// * > 1000x1000: +2 (excellent)
    /// * 300x300-600x600: 0 (neutral)
    /// * Unknown resolution: 0 (neutral)
    fn grade_resolution(&self, width: Option<u32>, height: Option<u32>) -> i32 {
        match (width, height) {
            (Some(w), Some(h)) => {
                // Use the smaller dimension for grading (ensures both dimensions meet criteria)
                let min_dimension = w.min(h);
                
                if min_dimension < 100 {
                    -2  // Very small
                } else if min_dimension < 300 {
                    -1  // Small
                } else if min_dimension > 1000 {
                    2   // Excellent
                } else if min_dimension > 600 {
                    1   // Good
                } else {
                    0   // Neutral (300-600 range)
                }
            }
            _ => {
                debug!("No resolution information available, assigning neutral score");
                0
            }
        }
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
        
        assert_eq!(grader.grade_provider("spotify"), 1);
        assert_eq!(grader.grade_provider("Spotify"), 1);
        assert_eq!(grader.grade_provider("theaudiodb"), 2);
        assert_eq!(grader.grade_provider("TheAudioDB"), 2);
        assert_eq!(grader.grade_provider("fanarttv"), 3);
        assert_eq!(grader.grade_provider("fanart.tv"), 3);
        assert_eq!(grader.grade_provider("FanArt.tv"), 3);
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
        assert_eq!(grader.grade_resolution(Some(50), Some(50)), -2);
        assert_eq!(grader.grade_resolution(Some(99), Some(150)), -2); // min dimension < 100
        
        // < 300x300
        assert_eq!(grader.grade_resolution(Some(200), Some(200)), -1);
        assert_eq!(grader.grade_resolution(Some(299), Some(400)), -1); // min dimension < 300
        
        // 300-600 (neutral)
        assert_eq!(grader.grade_resolution(Some(400), Some(400)), 0);
        assert_eq!(grader.grade_resolution(Some(300), Some(600)), 0);
        
        // > 600x600
        assert_eq!(grader.grade_resolution(Some(700), Some(700)), 1);
        assert_eq!(grader.grade_resolution(Some(600), Some(800)), 0); // min dimension = 600, not > 600
        assert_eq!(grader.grade_resolution(Some(601), Some(800)), 1); // min dimension > 600
        
        // > 1000x1000
        assert_eq!(grader.grade_resolution(Some(1200), Some(1200)), 2);
        assert_eq!(grader.grade_resolution(Some(1000), Some(1500)), 1); // min dimension = 1000, gets score 1 for > 600
        assert_eq!(grader.grade_resolution(Some(1001), Some(1500)), 2); // min dimension > 1000
        
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
            provider: "fanarttv".to_string(),
        };
        
        let grade = grader.grade_image(&high_quality_image);
        // fanarttv(+3) + >100KB(+1) + >1000x1000(+2) = 6
        assert_eq!(grade.score, 6);
        assert_eq!(grade.provider_score, 3);
        assert_eq!(grade.size_score, 1);
        assert_eq!(grade.resolution_score, 2);
        
        let low_quality_image = ImageInfo {
            url: "https://example.com/low_quality.jpg".to_string(),
            width: Some(50),
            height: Some(50),
            size_bytes: Some(5_000),
            provider: "unknown".to_string(),
        };
        
        let grade = grader.grade_image(&low_quality_image);
        // unknown(0) + <10KB(-1) + <100x100(-2) = -3
        assert_eq!(grade.score, -3);
        assert_eq!(grade.provider_score, 0);
        assert_eq!(grade.size_score, -1);
        assert_eq!(grade.resolution_score, -2);
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
                provider: "spotify".to_string(),
            },
            ImageInfo {
                url: "high.jpg".to_string(),
                width: Some(1200),
                height: Some(1200),
                size_bytes: Some(300_000),
                provider: "fanarttv".to_string(),
            },
            ImageInfo {
                url: "medium.jpg".to_string(),
                width: Some(500),
                height: Some(500),
                size_bytes: Some(50_000),
                provider: "theaudiodb".to_string(),
            },
        ];
        
        let graded = grader.grade_and_sort_images(images);
        
        // Should be sorted by score descending
        assert_eq!(graded[0].0.url, "high.jpg"); // Score: 3+1+2 = 6
        assert_eq!(graded[1].0.url, "medium.jpg"); // Score: 2+0+0 = 2
        assert_eq!(graded[2].0.url, "low.jpg"); // Score: 1+(-1)+(-2) = -2
        
        assert!(graded[0].1.score > graded[1].1.score);
        assert!(graded[1].1.score > graded[2].1.score);
    }
}
