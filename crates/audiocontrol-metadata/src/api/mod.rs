pub mod coverart;
pub mod favourites;
pub mod lastfm;
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
        // daemon's own `api_routes` list.
        ("".to_string(), rocket::routes![theaudiodb::lookup_artist_by_mbid]),
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
