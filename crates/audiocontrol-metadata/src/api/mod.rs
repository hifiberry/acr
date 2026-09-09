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
/// `/background` is here for the same reason, and it is the reason this list
/// is shaped as (mount point, routes) rather than a bare `Vec<Route>`.
/// `acr_store::backgroundjobs` is a per-*process* registry: `artistupdater`
/// and `albumupdater` register enrichment progress in this daemon's, and the
/// player daemon's `GET /api/background/jobs` answers from its own and cannot
/// see them. Mounting the same handlers here puts enrichment progress back on
/// the network at `GET /api/metadata/background/jobs`, which nginx already
/// forwards. It may not join [`routes`]: that set mounts at the bare `/api`,
/// where the player daemon serves `/background/jobs` itself, and two identical
/// routes at one mount stop Rocket igniting.
pub fn standalone_routes() -> Vec<(String, Vec<rocket::Route>)> {
    vec![
        ("".to_string(), rocket::routes![capabilities::get_capabilities]),
        ("/background".to_string(), acr_web::backgroundjobs::routes()),
    ]
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

    /// A job registered with the process-wide background registry is visible
    /// over `standalone_routes`.
    ///
    /// `artistupdater` and `albumupdater` register their progress with
    /// `acr_store::backgroundjobs`, and that registry is per *process*. Before
    /// this group existed, the only `/background` mount in the system was the
    /// player daemon's, so on a split installation enrichment reported into a
    /// registry with no HTTP surface at all: a client polling the player
    /// daemon got a successful, permanently empty answer.
    #[test]
    fn standalone_routes_report_a_registered_background_job() {
        let id = "test_metadata_standalone_background_job";
        acr_store::backgroundjobs::register_job(id.to_string(), "Test Job".to_string())
            .expect("could not register the job");

        let mut rocket = rocket::build();
        for (mount, routes) in standalone_routes() {
            rocket = rocket.mount(format!("/api{}", mount), routes);
        }
        let client = Client::tracked(rocket).unwrap();
        let response = client.get("/api/background/jobs").dispatch();

        assert_eq!(response.status(), Status::Ok);
        assert!(
            response.into_string().unwrap_or_default().contains(id),
            "the registered job is missing from the listing"
        );
    }

    /// The background group belongs to `standalone_routes` and must never move
    /// into `routes()`.
    ///
    /// Same collision as `/capabilities`, and worse in one way: the player
    /// daemon mounts `routes()` at the bare `/api`, where it already serves
    /// `GET /api/background/jobs` itself. Two identical routes at one mount
    /// make Rocket refuse to ignite, so the daemon would not start at all.
    #[test]
    fn routes_never_claims_the_background_paths() {
        for (mount, group) in routes() {
            for route in group {
                let path = format!("{}{}", mount, route.uri.path());
                assert!(
                    !path.starts_with("/background"),
                    "api::routes must not mount {} -- it collides with the player daemon's own",
                    path
                );
            }
        }
    }
}
