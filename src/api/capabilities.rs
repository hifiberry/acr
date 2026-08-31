use rocket::get;
use rocket::serde::json::Json;

/// What this daemon can do, as opposed to which release it is.
///
/// A version string identifies a build; it does not say what that build supports,
/// and a client reasoning from it has to carry a table mapping versions to
/// behaviour. This endpoint answers the question directly.
///
/// Absence is a complete answer: a release without this endpoint returns 404, which
/// means no resizing, and a client can go straight to full-size images.
#[derive(serde::Serialize)]
pub struct CapabilitiesResponse {
    /// Included for convenience so a client needs one request, not two.
    pub version: String,
    pub images: ImageCapabilities,
}

/// Image-serving capabilities.
///
/// Nested rather than flat so the next capability - the per-backend event
/// vocabulary is the same problem waiting to be solved - can be added beside it
/// without renaming anything.
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
            sizes: crate::helpers::imageresize::SIZE_LADDER.to_vec(),
        },
    }
}

/// Report what this daemon supports.
#[get("/capabilities")]
pub fn get_capabilities() -> Json<CapabilitiesResponse> {
    Json(current_capabilities())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_advertise_the_size_ladder() {
        let body = serde_json::to_value(current_capabilities()).unwrap();
        assert_eq!(body["images"]["sizes"], serde_json::json!([100, 200, 400, 800]));
    }

    #[test]
    fn capabilities_carry_the_version() {
        let body = serde_json::to_value(current_capabilities()).unwrap();
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }
}
