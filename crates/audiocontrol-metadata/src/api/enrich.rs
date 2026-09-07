//! Advisory nudge: ask this side to pull one player's library sooner than
//! its next periodic poll.
//!
//! Per `doc/specs/2026-09-04-player-metadata-split.md`, the player daemon may
//! call this after a library load to shorten the wait before enrichment
//! starts; a nudge that fails is ignored, because the periodic poll (every
//! 30 s) covers it regardless. That makes 202 the honest answer whether or
//! not anything is listening: the request is accepted for consideration, not
//! guaranteed to do anything before this call returns.

use log::debug;
use rocket::http::Status;
use rocket::post;

/// Nudge library enrichment for one player.
///
/// There is no puller yet — a later task adds one and this becomes a call
/// into it — so today this only logs the request at debug and answers 202.
/// Nothing here needs a placeholder to call: an empty body that still
/// answers 202 is the honest state of "accepted, nothing acts on it yet".
#[post("/enrich/nudge?<player>")]
pub fn nudge(player: &str) -> Status {
    debug!("enrichment nudge requested for player '{}'; no puller wired up yet", player);
    Status::Accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::local::blocking::Client;

    fn test_client() -> Client {
        let rocket = rocket::build().mount("/api", rocket::routes![nudge]);
        Client::tracked(rocket).unwrap()
    }

    #[test]
    fn a_nudge_is_accepted() {
        let client = test_client();
        let r = client.post("/api/enrich/nudge?player=mpd").dispatch();
        assert_eq!(r.status(), Status::Accepted);
    }

    #[test]
    fn a_nudge_without_a_player_is_rejected_by_rocket() {
        // `player` is a required query parameter -- a request missing it
        // fails Rocket's own query guard before the handler runs, so this
        // proves the route is declared to require it rather than defaulting
        // silently. Rocket answers a failed query guard 422, not 404: the
        // route was matched, its query just didn't.
        let client = test_client();
        let r = client.post("/api/enrich/nudge").dispatch();
        assert_eq!(r.status(), Status::UnprocessableEntity);
    }
}
