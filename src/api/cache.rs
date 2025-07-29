use rocket::serde::json::Json;
use rocket::get;
use serde::{Deserialize, Serialize};
use log::{debug, error};
use crate::helpers::attributecache::{get_cache_stats, CacheStats};

/// Response structure for cache statistics
#[derive(Serialize, Deserialize)]
pub struct CacheStatsResponse {
    pub success: bool,
    pub stats: Option<CacheStats>,
    pub message: Option<String>,
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
/// memory entries, memory usage in bytes, and memory limit.
#[get("/stats")]
pub fn get_cache_statistics() -> Json<CacheStatsResponse> {
    debug!("API request: get cache statistics");

    match get_cache_stats() {
        Ok(stats) => {
            debug!("Successfully retrieved cache stats: disk_entries={}, memory_entries={}, memory_bytes={}, memory_limit_bytes={}", 
                stats.disk_entries, stats.memory_entries, stats.memory_bytes, stats.memory_limit_bytes);
            
            Json(CacheStatsResponse {
                success: true,
                stats: Some(stats),
                message: None,
            })
        }
        Err(e) => {
            error!("Failed to retrieve cache stats: {}", e);
            Json(CacheStatsResponse {
                success: false,
                stats: None,
                message: Some(format!("Failed to retrieve cache statistics: {}", e)),
            })
        }
    }
}
