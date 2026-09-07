//! Advisory nudge: ask this side to pull one player's library sooner than
//! its next periodic poll.
//!
//! Per `doc/specs/2026-09-04-player-metadata-split.md`, the player daemon may
//! call this after a library load to shorten the wait before enrichment
//! starts; a nudge that fails is ignored, because the periodic poll (every
//! 30 s) covers it regardless. That makes 202 the honest answer whether or
//! not anything is listening: the request is accepted for consideration, not
//! guaranteed to do anything before this call returns.

use rocket::http::Status;
use rocket::post;

/// Nudge library enrichment for one player.
///
/// Hands the name to the library puller, which pulls that player's library at
/// once instead of at its next poll. The answer is 202 whether or not a puller
/// is running to receive it, and whether or not the pull finds anything to do:
/// the nudge is a hint, the poll is the guarantee, and a caller must not be
/// given a status that invites it to retry.
#[post("/enrich/nudge?<player>")]
pub fn nudge(player: &str) -> Status {
    crate::library_puller::nudge(player);
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
