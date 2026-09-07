//! What this side of the daemon can do, as opposed to which release it is.
//!
//! A copy of `src/api/capabilities.rs`'s route, not a shared one: the player
//! daemon already serves `GET /capabilities` at `API_PREFIX` in the shared
//! Rocket both halves currently run in, at the same rank this crate's own
//! copy would claim, and Rocket refuses to ignite over an exact duplicate
//! route. So `get_capabilities` here is deliberately left out of
//! [`super::routes`] and offered only through [`super::standalone_routes`],
//! for the Rocket this crate will serve on its own once the two halves are
//! separate processes. Until then it is exercised only against its own test
//! Rocket, below.

use rocket::get;
use rocket::serde::json::Json;

/// Included for convenience so a client needs one request, not two.
#[derive(serde::Serialize)]
pub struct CapabilitiesResponse {
    pub version: String,
    pub images: ImageCapabilities,
}

/// Image-serving capabilities.
///
/// Nested rather than flat, matching the player daemon's own shape, so a
/// client that already reads one can read the other unchanged.
#[derive(serde::Serialize)]
pub struct ImageCapabilities {
    /// Sizes accepted by the `size` parameter. Requests are rounded up to one of these.
    pub sizes: Vec<u32>,
}

/// Build the current capability set.
pub fn current_capabilities() -> CapabilitiesResponse {
    CapabilitiesResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        images: ImageCapabilities {
            sizes: acr_images::imageresize::sizes().to_vec(),
        },
    }
}

/// Report what this side of the daemon supports.
#[get("/capabilities")]
pub fn get_capabilities() -> Json<CapabilitiesResponse> {
    Json(current_capabilities())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::Status;
    use rocket::local::blocking::Client;

    fn test_client() -> Client {
        let rocket = rocket::build().mount("/api", rocket::routes![get_capabilities]);
        Client::tracked(rocket).unwrap()
    }

    #[test]
    fn capabilities_advertise_the_size_ladder() {
        let body = serde_json::to_value(current_capabilities()).unwrap();
        assert_eq!(body["images"]["sizes"], serde_json::json!([100, 140, 200, 280, 400, 800]));
    }

    #[test]
    fn capabilities_carry_the_version() {
        let body = serde_json::to_value(current_capabilities()).unwrap();
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_route_serves_the_same_shape_as_the_player_daemons() {
        let client = test_client();
        let r = client.get("/api/capabilities").dispatch();
        assert_eq!(r.status(), Status::Ok);
        let body = r.into_json::<serde_json::Value>().unwrap();
        assert_eq!(body["images"]["sizes"], serde_json::json!([100, 140, 200, 280, 400, 800]));
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }
}
