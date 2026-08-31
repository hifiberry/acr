use rocket::serde::json::Json;
use rocket::get;
use serde::{Deserialize, Serialize};
use log::{debug, error};
use crate::helpers::attributecache::{get_cache_stats, CacheStats};
use crate::helpers::imagecache;

/// Response structure for cache statistics
#[derive(Serialize, Deserialize)]
pub struct CacheStatsResponse {
    pub success: bool,
    pub stats: Option<CacheStats>,
    pub image_cache_stats: Option<ImageCacheStats>,
    pub message: Option<String>,
}

/// Image cache statistics for API response
#[derive(Serialize, Deserialize)]
pub struct ImageCacheStats {
    /// Everything on disk, generated variants included.
    pub total_images: usize,
    /// Every byte on disk, generated variants included.
    pub total_size: u64,
    /// How many of `total_images` are generated thumbnails.
    ///
    /// Variants are the only cache content that can grow without bound, and
    /// `POST /api/imagecache/variants/purge` is the only lever against that
    /// growth. Reporting them separately is what lets an operator see how much
    /// a purge would reclaim, and whether it worked.
    pub variant_images: usize,
    /// How many of `total_size` bytes are generated thumbnails.
    pub variant_size: u64,
    pub last_updated: u64,
}

/// Response structure for error operations
#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub message: String,
}

/// Get cache statistics
/// 
/// This endpoint retrieves current cache statistics including disk entries,
/// memory entries, memory usage in bytes, memory limit, and image cache statistics.
#[get("/stats")]
pub fn get_cache_statistics() -> Json<CacheStatsResponse> {
    debug!("API request: get cache statistics");

    // Get attribute cache stats
    let attribute_stats = match get_cache_stats() {
        Ok(stats) => {
            debug!("Successfully retrieved attribute cache stats: disk_entries={}, memory_entries={}, memory_bytes={}, memory_limit_bytes={}", 
                stats.disk_entries, stats.memory_entries, stats.memory_bytes, stats.memory_limit_bytes);
            Some(stats)
        }
        Err(e) => {
            error!("Failed to retrieve attribute cache stats: {}", e);
            None
        }
    };

    // Get image cache stats
    let image_stats = match imagecache::get_cache_statistics() {
        Ok(stats) => {
            debug!("Successfully retrieved image cache stats: total_images={}, total_size={}, variant_images={}, variant_size={}, last_updated={}",
                stats.total_images, stats.total_size, stats.variant_images, stats.variant_size, stats.last_updated);
            Some(ImageCacheStats {
                total_images: stats.total_images,
                total_size: stats.total_size,
                variant_images: stats.variant_images,
                variant_size: stats.variant_size,
                last_updated: stats.last_updated,
            })
        }
        Err(e) => {
            error!("Failed to retrieve image cache stats: {}", e);
            None
        }
    };

    // Determine success based on whether we got at least one set of stats
    let success = attribute_stats.is_some() || image_stats.is_some();
    let message = if !success {
        Some("Failed to retrieve any cache statistics".to_string())
    } else if attribute_stats.is_none() {
        Some("Failed to retrieve attribute cache statistics".to_string())
    } else if image_stats.is_none() {
        Some("Failed to retrieve image cache statistics".to_string())
    } else {
        None
    };

    Json(CacheStatsResponse {
        success,
        stats: attribute_stats,
        image_cache_stats: image_stats,
        message,
    })
}
