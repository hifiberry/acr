// Import constants for use in API modules
pub use crate::constants::API_PREFIX;

/// Rewrite internal API-relative URLs (starting with API_PREFIX) to the externally
/// visible API base if a reverse proxy forwards `X-Forwarded-Prefix`.
pub fn rewrite_api_relative_url(url: &str, forwarded_prefix: Option<&str>) -> String {
	let Some(prefix) = normalize_forwarded_prefix(forwarded_prefix) else {
		return url.to_string();
	};

	if url == API_PREFIX {
		return prefix;
	}

	if let Some(suffix) = url.strip_prefix(API_PREFIX) {
		return format!("{}{}", prefix, suffix);
	}

	url.to_string()
}

fn normalize_forwarded_prefix(prefix: Option<&str>) -> Option<String> {
	let raw = prefix?.trim();
	if raw.is_empty() {
		return None;
	}

	let without_trailing = raw.trim_end_matches('/');
	if without_trailing.is_empty() {
		return None;
	}

	if without_trailing.starts_with('/') {
		Some(without_trailing.to_string())
	} else {
		Some(format!("/{}", without_trailing))
	}
}

// Export the players module
pub mod players;

// Export the plugins module
pub mod plugins;

// Export the library module
pub mod library;

// Export the imagecache module
pub mod imagecache;

// Export the coverart module
pub mod coverart;

// Export the event module
pub mod events;

// Export the lastfm module
pub mod lastfm;

// Export the spotify module
pub mod spotify;

// Export the theaudiodb module
pub mod theaudiodb;

// Export the favourites module
pub mod favourites;

// Export the volume module
pub mod volume;

// Export the lyrics module
pub mod lyrics;

// Export the m3u module
pub mod m3u;

// Export the settings module
pub mod settings;

// Export the cache module
pub mod cache;

// Export the backgroundjobs module
pub mod backgroundjobs;

// Export the server module
pub mod server;