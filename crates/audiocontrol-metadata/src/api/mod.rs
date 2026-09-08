pub mod artist;
pub mod capabilities;
pub mod coverart;
pub mod favourites;
pub mod lastfm;
pub mod resolve;
pub mod theaudiodb;

/// The metadata routes and where each set mounts, relative to `/api`.
///
/// The order of the groups, and of the routes within each group, is the order
/// `src/api/server.rs` mounted them in before they moved out: Rocket resolves
/// a collision by rank and then by declaration order, so both have to be
/// carried across unchanged.
///
/// **There is no `/enrich` route any more.** `POST /enrich/nudge` was how the
/// player daemon asked this side to look at a library it had just loaded, and
/// nothing on this daemon may be called by the player daemon after the one-way
/// seam. The `library_changed` event replaced it, travelling the other way over
/// the socket this side already holds open; see `crate::library_puller`. The
/// route is deleted rather than deprecated because it was both introduced and
/// removed within 0.22.0, and its only caller shipped in that same release, so
/// there is no stale caller a deprecation window could protect.
///
/// **There is no `/spotify` group any more.** The account moved to the player
/// daemon with the one-way seam, and all thirteen of its routes went with it
/// (`src/api/spotify.rs` there). The client-visible paths are unchanged; what
/// changed is which process serves them, and this side must not serve any of
/// them a second time -- the daemon mounts both sets at `/api`, and two
/// identical routes at one mount make Rocket refuse to ignite.
pub fn routes() -> Vec<(String, Vec<rocket::Route>)> {
    vec![
        // Mounted at the bare API prefix, as it was when it sat inline in the
        // daemon's own `api_routes` list. `theaudiodb::lookup_artist_by_mbid`
        // came first; the three routes after it are new in this phase and are
        // appended rather than interleaved so its declaration position is
        // unchanged.
        (
            "".to_string(),
            rocket::routes![
                theaudiodb::lookup_artist_by_mbid,
                artist::get_artist,
                resolve::title_order,
                resolve::artist_split,
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

    /// `routes()` never gains `capabilities::get_capabilities` back: that
    /// route mounting at the same rank the player daemon already serves it
    /// at is exactly the collision this split exists to avoid.
    #[test]
    fn routes_never_reclaims_the_capabilities_path() {
        for (_, group) in routes() {
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
