pub mod artist;
pub mod capabilities;
pub mod coverart;
pub mod enrich;
pub mod favourites;
pub mod lastfm;
pub mod resolve;
pub mod spotify;
pub mod theaudiodb;

/// The metadata routes and where each set mounts, relative to `/api`.
///
/// The order of the groups, and of the routes within each group, is the order
/// `src/api/server.rs` mounted them in before they moved out: Rocket resolves
/// a collision by rank and then by declaration order, so both have to be
/// carried across unchanged.
pub fn routes(spotify_api_enabled: bool) -> Vec<(String, Vec<rocket::Route>)> {
    // Spotify serves the authentication routes always and the playback and
    // search routes only when `spotify.api_enabled` is set. The two lists
    // share their first eight entries and `get_access_token`; the difference
    // is the four in the middle.
    let spotify_routes = if spotify_api_enabled {
        rocket::routes![
            spotify::store_tokens,
            spotify::token_status,
            spotify::logout,
            spotify::get_oauth_config,
            spotify::create_session,
            spotify::login,
            spotify::poll_session,
            spotify::check_server,
            spotify::spotify_command,
            spotify::get_playback,
            spotify::spotify_currently_playing,
            spotify::spotify_search,
            spotify::get_access_token
        ]
    } else {
        rocket::routes![
            spotify::store_tokens,
            spotify::token_status,
            spotify::logout,
            spotify::get_oauth_config,
            spotify::create_session,
            spotify::login,
            spotify::poll_session,
            spotify::check_server,
            spotify::get_access_token
        ]
    };

    vec![
        // Mounted at the bare API prefix, as it was when it sat inline in the
        // daemon's own `api_routes` list. `theaudiodb::lookup_artist_by_mbid`
        // came first; the four routes after it are new in this phase and are
        // appended rather than interleaved so its declaration position is
        // unchanged.
        (
            "".to_string(),
            rocket::routes![
                theaudiodb::lookup_artist_by_mbid,
                artist::get_artist,
                resolve::title_order,
                resolve::artist_split,
                enrich::nudge,
            ],
        ),
        (
            "/lastfm".to_string(),
            rocket::routes![
                lastfm::get_status,
                lastfm::get_auth_url_handler,
                lastfm::prepare_complete_auth,
                lastfm::complete_auth,
                lastfm::disconnect_handler,
            ],
        ),
        ("/spotify".to_string(), spotify_routes),
        ("/favourites".to_string(), favourites::routes()),
        (
            "/coverart".to_string(),
            rocket::routes![
                coverart::get_artist_coverart,
                coverart::get_song_coverart,
                coverart::get_album_coverart,
                coverart::get_album_coverart_with_year,
                coverart::get_url_coverart,
                coverart::get_coverart_methods,
                coverart::upload_artist_image,
                coverart::update_artist_image,
                coverart::get_artist_image,
                coverart::get_artist_images,
                coverart::get_artist_image_by_id,
                coverart::delete_artist_image_route,
            ],
        ),
    ]
}

/// Routes this crate serves only under its own prefix, never at the bare
/// `/api` the player daemon shares with it.
///
/// `capabilities::get_capabilities` cannot join [`routes`]: that function's
/// `""` group mounts at `/api` itself, where the player daemon already serves
/// `GET /capabilities` (`src/api/server.rs`'s own `api_routes`), and Rocket
/// refuses to ignite over an exact duplicate route rather than resolving it
/// by declaration order — the daemon would not start at all.
///
/// It is not unmounted, though. `src/main.rs` mounts this list under
/// `/api/metadata`, alongside a second copy of [`routes`], so the route
/// answers at `GET /api/metadata/capabilities` — the path the spec gives it,
/// and the one it keeps when this crate serves a Rocket of its own.
///
/// Shaped the same as [`routes`] (mount point, routes) rather than a bare
/// `Vec<Route>`, so a later addition to this list — this phase has exactly
/// one entry — needs no change to how a caller mounts it.
pub fn standalone_routes() -> Vec<(String, Vec<rocket::Route>)> {
    vec![("".to_string(), rocket::routes![capabilities::get_capabilities])]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::Status;
    use rocket::local::blocking::Client;

    /// `standalone_routes` mounted the way a standalone metadata Rocket
    /// would mount it -- proving the function itself wires the route through,
    /// not just that `capabilities::get_capabilities` works when mounted by
    /// hand (that is `capabilities`'s own test).
    #[test]
    fn standalone_routes_serve_capabilities() {
        let mut rocket = rocket::build();
        for (mount, routes) in standalone_routes() {
            rocket = rocket.mount(format!("/api{}", mount), routes);
        }
        let client = Client::tracked(rocket).unwrap();
        let response = client.get("/api/capabilities").dispatch();
        assert_eq!(response.status(), Status::Ok);
    }

    /// `routes(..)` never gains `capabilities::get_capabilities` back: that
    /// route mounting at the same rank the player daemon already serves it
    /// at is exactly the collision this split exists to avoid.
    #[test]
    fn routes_never_reclaims_the_capabilities_path() {
        for (_, group) in routes(false) {
            for route in group {
                assert_ne!(
                    (route.method, route.uri.path()),
                    (rocket::http::Method::Get, "/capabilities"),
                    "api::routes must not mount /capabilities -- it collides with the player daemon's own"
                );
            }
        }
    }
}
