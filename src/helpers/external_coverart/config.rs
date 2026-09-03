//! Parsing of `services.external_coverart` from `audiocontrol.json`.

use std::collections::{HashMap, HashSet};

use log::{info, warn};

use crate::config::get_service_config;
use crate::helpers::coverart::CoverartMethod;

/// How long an error is cached. Short, because an error is a statement about
/// the service rather than about the artwork: a 502 or a timeout must not
/// leave a track blank for the weeks a real answer is kept.
pub const ERROR_TTL_SECONDS: u64 = 3600;

const DEFAULT_TIMEOUT_SECONDS: u64 = 45;
const DEFAULT_CACHE_TTL_DAYS: u64 = 30;
const DEFAULT_NEGATIVE_CACHE_TTL_DAYS: u64 = 7;
const DEFAULT_MAX_CONCURRENT: usize = 1;

/// When an endpoint is worth asking.
///
/// Note that on the now-playing path this controls cost, not outcome:
/// `BasePlayerController::apply_song_information` refuses to replace artwork
/// that belongs to a song, so an `Always` answer for a song that already has
/// real artwork is dropped. `Always` changes the outcome only for the REST
/// endpoints and for artist images, where no such policy applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Ask only when the song has no artwork, or only a placeholder.
    Fallback,
    /// Ask for every song.
    Always,
}

/// One configured endpoint.
#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub name: String,
    pub display_name: String,
    pub url: String,
    pub methods: Vec<CoverartMethod>,
    pub headers: HashMap<String, String>,
    pub timeout_seconds: u64,
    pub trigger: Trigger,
    pub cache_ttl_days: u64,
    pub negative_cache_ttl_days: u64,
    pub max_concurrent: usize,
}

fn parse_method(name: &str) -> Option<CoverartMethod> {
    match name {
        "artist" => Some(CoverartMethod::Artist),
        "song" => Some(CoverartMethod::Song),
        "album" => Some(CoverartMethod::Album),
        "url" => Some(CoverartMethod::Url),
        _ => None,
    }
}

fn parse_endpoint(value: &serde_json::Value) -> Option<EndpointConfig> {
    let name = value.get("name").and_then(|v| v.as_str())?.trim().to_string();
    if name.is_empty() {
        warn!("External cover art: an endpoint has an empty name; skipping it");
        return None;
    }

    let Some(url) = value.get("url").and_then(|v| v.as_str()) else {
        warn!("External cover art: endpoint '{}' has no url; skipping it", name);
        return None;
    };

    let display_name = value
        .get("display_name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| name.clone());

    let methods: Vec<CoverartMethod> = match value.get("methods").and_then(|v| v.as_array()) {
        Some(entries) => entries
            .iter()
            .filter_map(|entry| entry.as_str())
            .filter_map(|entry| match parse_method(entry) {
                Some(method) => Some(method),
                None => {
                    warn!(
                        "External cover art: endpoint '{}' names unknown method '{}'; ignoring it",
                        name, entry
                    );
                    None
                }
            })
            .collect(),
        // A song lookup is what the now-playing path needs, so it is the
        // useful default for an endpoint that does not say.
        None => vec![CoverartMethod::Song],
    };

    if methods.is_empty() {
        warn!(
            "External cover art: endpoint '{}' supports no known method; skipping it",
            name
        );
        return None;
    }

    let headers = value
        .get("headers")
        .and_then(|v| v.as_object())
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let trigger = match value.get("trigger").and_then(|v| v.as_str()) {
        Some("always") => Trigger::Always,
        Some("fallback") | None => Trigger::Fallback,
        Some(other) => {
            warn!(
                "External cover art: endpoint '{}' has unknown trigger '{}'; using fallback",
                name, other
            );
            Trigger::Fallback
        }
    };

    let number = |key: &str, default: u64| {
        value
            .get(key)
            .and_then(|v| v.as_u64())
            .filter(|v| *v > 0)
            .unwrap_or(default)
    };

    Some(EndpointConfig {
        name,
        display_name,
        url: url.to_string(),
        methods,
        headers,
        timeout_seconds: number("timeout_seconds", DEFAULT_TIMEOUT_SECONDS),
        trigger,
        cache_ttl_days: number("cache_ttl_days", DEFAULT_CACHE_TTL_DAYS),
        negative_cache_ttl_days: number(
            "negative_cache_ttl_days",
            DEFAULT_NEGATIVE_CACHE_TTL_DAYS,
        ),
        max_concurrent: number("max_concurrent", DEFAULT_MAX_CONCURRENT as u64) as usize,
    })
}

/// Read every configured endpoint.
///
/// A malformed entry is skipped with a warning rather than failing the whole
/// service: this daemon runs unattended on an appliance, where a typo in a
/// cover art endpoint must not be the reason nothing plays.
pub fn parse_endpoints(config: &serde_json::Value) -> Vec<EndpointConfig> {
    let Some(service) = get_service_config(config, "external_coverart") else {
        return Vec::new();
    };

    if !service.get("enable").and_then(|v| v.as_bool()).unwrap_or(false) {
        info!("External cover art providers are disabled");
        return Vec::new();
    }

    let Some(entries) = service.get("providers").and_then(|v| v.as_array()) else {
        warn!("External cover art is enabled but names no providers");
        return Vec::new();
    };

    let mut seen: HashSet<String> = HashSet::new();
    let mut endpoints = Vec::new();
    for entry in entries {
        let Some(endpoint) = parse_endpoint(entry) else {
            continue;
        };
        // The name is the cache key prefix and the cover_art_source value
        // clients see; two endpoints sharing one would be indistinguishable.
        if !seen.insert(endpoint.name.clone()) {
            warn!(
                "External cover art: endpoint name '{}' is used more than once; skipping the later one",
                endpoint.name
            );
            continue;
        }
        info!(
            "External cover art endpoint '{}' configured for {:?}, timeout {}s, trigger {:?}",
            endpoint.name, endpoint.methods, endpoint.timeout_seconds, endpoint.trigger
        );
        endpoints.push(endpoint);
    }
    endpoints
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(providers: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "services": {
                "external_coverart": {
                    "enable": true,
                    "providers": providers
                }
            }
        })
    }

    #[test]
    fn a_minimal_endpoint_parses_with_defaults() {
        let endpoints = parse_endpoints(&config_with(serde_json::json!([{
            "name": "llm",
            "url": "https://tools.example.com/coverart?artist={artist}"
        }])));

        assert_eq!(endpoints.len(), 1);
        let endpoint = &endpoints[0];
        assert_eq!(endpoint.name, "llm");
        // Absent display_name falls back to the name rather than being empty
        // in the API's provider list.
        assert_eq!(endpoint.display_name, "llm");
        assert_eq!(endpoint.timeout_seconds, 45);
        assert_eq!(endpoint.trigger, Trigger::Fallback);
        assert_eq!(endpoint.cache_ttl_days, 30);
        assert_eq!(endpoint.negative_cache_ttl_days, 7);
        assert_eq!(endpoint.max_concurrent, 1);
        // An endpoint that names no methods answers song lookups, which is
        // what the now-playing path needs.
        assert_eq!(endpoint.methods, vec![CoverartMethod::Song]);
    }

    #[test]
    fn every_field_is_read() {
        let endpoints = parse_endpoints(&config_with(serde_json::json!([{
            "name": "llm",
            "display_name": "AI Lookup",
            "url": "https://tools.example.com/coverart",
            "methods": ["song", "album", "artist"],
            "headers": { "Authorization": "Bearer sekrit" },
            "timeout_seconds": 90,
            "trigger": "always",
            "cache_ttl_days": 10,
            "negative_cache_ttl_days": 2,
            "max_concurrent": 3
        }])));

        let endpoint = &endpoints[0];
        assert_eq!(endpoint.display_name, "AI Lookup");
        assert_eq!(endpoint.timeout_seconds, 90);
        assert_eq!(endpoint.trigger, Trigger::Always);
        assert_eq!(endpoint.cache_ttl_days, 10);
        assert_eq!(endpoint.negative_cache_ttl_days, 2);
        assert_eq!(endpoint.max_concurrent, 3);
        assert_eq!(
            endpoint.headers.get("Authorization").map(String::as_str),
            Some("Bearer sekrit")
        );
        assert_eq!(
            endpoint.methods,
            vec![CoverartMethod::Song, CoverartMethod::Album, CoverartMethod::Artist]
        );
    }

    /// A typo in one endpoint must not take out the others. The daemon runs
    /// unattended on an appliance; refusing to start over a bad cover art
    /// entry would be a much worse failure than skipping it.
    #[test]
    fn a_malformed_endpoint_is_skipped_not_fatal() {
        let endpoints = parse_endpoints(&config_with(serde_json::json!([
            { "name": "broken" },
            { "name": "good", "url": "https://tools.example.com/coverart" }
        ])));

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].name, "good");
    }

    #[test]
    fn an_unknown_method_name_is_skipped() {
        let endpoints = parse_endpoints(&config_with(serde_json::json!([{
            "name": "llm",
            "url": "https://tools.example.com/coverart",
            "methods": ["song", "playlist"]
        }])));

        assert_eq!(endpoints[0].methods, vec![CoverartMethod::Song]);
    }

    /// An endpoint left with no usable method would be registered and never
    /// asked anything.
    #[test]
    fn an_endpoint_with_no_usable_method_is_skipped() {
        let endpoints = parse_endpoints(&config_with(serde_json::json!([{
            "name": "llm",
            "url": "https://tools.example.com/coverart",
            "methods": ["playlist"]
        }])));

        assert!(endpoints.is_empty());
    }

    #[test]
    fn disabling_the_service_yields_no_endpoints() {
        let config = serde_json::json!({
            "services": {
                "external_coverart": {
                    "enable": false,
                    "providers": [{ "name": "llm", "url": "https://tools.example.com/c" }]
                }
            }
        });
        assert!(parse_endpoints(&config).is_empty());
    }

    #[test]
    fn no_configuration_at_all_yields_no_endpoints() {
        assert!(parse_endpoints(&serde_json::json!({})).is_empty());
    }

    /// Two endpoints sharing a name would collide in the cache key space and
    /// in cover_art_source, where the name is the provenance clients see.
    #[test]
    fn a_duplicate_name_is_skipped() {
        let endpoints = parse_endpoints(&config_with(serde_json::json!([
            { "name": "llm", "url": "https://tools.example.com/one" },
            { "name": "llm", "url": "https://tools.example.com/two" }
        ])));

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].url, "https://tools.example.com/one");
    }
}
