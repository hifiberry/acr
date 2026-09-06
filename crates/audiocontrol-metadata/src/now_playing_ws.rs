//! The metadata side's subscriber for the player daemon's event socket.
//!
//! This replaces the in-process bridge (`now_playing_bridge` on the player
//! side) with a WebSocket client: the same `NowPlayingEvent` channel the
//! enrichment workers already read, fed from `ws://.../api/events` instead of
//! from the event bus. Nothing downstream changes -- `now_playing::start`
//! takes the receiver either way.
//!
//! Three things make this seam different from the other two in this phase,
//! and all three are about the connection rather than the mapping:
//!
//! **It has to come back.** The socket will drop: the player daemon restarts,
//! a reverse proxy times an idle connection out, the machine suspends. A
//! subscriber that gave up would leave enrichment permanently deaf with no
//! error anywhere, so the loop reconnects with an exponential backoff capped
//! at [`Timings::max_backoff`]. The backoff is *not* reset merely because a
//! connect succeeded: a player that accepts a connection and immediately
//! closes it would then be retried once a second forever. It resets only once
//! a connection has proved useful -- see [`should_reset_backoff`].
//!
//! **What arrives during a gap is lost.** The player holds recent events for
//! 30 s and delivers them to each connected client from the point that client
//! registered; a reconnect registers afresh, so nothing from the gap is
//! replayed however short it was. Two things cover that, both deliberate: on every
//! (re)connect the subscriber asks `GET /now-playing` and emits one
//! `SongChanged` for whatever is playing now, and the Last.fm worker
//! reconciles the playback state against the player every 30 s through
//! `PlaybackStateSource` rather than trusting the stream. What is genuinely
//! lost is the *intermediate* history -- tracks that started and ended inside
//! the gap are never enriched and never scrobbled. That is a property of the
//! seam, not a bug to be fixed here: recovering it would need the player to
//! hold a durable per-subscriber queue, which it does not.
//!
//! **It must not park forever.** A blocking read on a socket that never
//! closes and never delivers is indistinguishable from a healthy idle
//! connection, and it also cannot notice that the receiver has gone. Every
//! connection therefore gets a read timeout of [`Timings::idle_tick`], so the
//! read loop wakes regularly, and each wake doubles as the keepalive below.
//!
//! ## Why an idle connection has to send something
//!
//! `WebSocketManager::prune_inactive_and_old` in `src/api/events.rs` drops a
//! client whose `last_activity` is older than an hour, and `last_activity` is
//! refreshed only by messages travelling *client to server*. A subscriber
//! that connects, subscribes and then only listens is therefore pruned after
//! an hour: the socket stays open and the daemon goes quietly deaf, which is
//! the worst possible failure shape. So the loop sends a WebSocket ping after
//! [`Timings::idle_tick`] without traffic, which the player's handler treats
//! as activity.
//!
//! Only `ws://` is supported. This seam is loopback in Phase 1 and a local
//! service in Phase 2; adding TLS would mean pulling a TLS stack into
//! `tungstenite` for a connection that never leaves the machine.

use acr_types::now_playing::NowPlayingEvent;
use acr_types::{PlaybackState, PlayerSource, Song};
use crossbeam::channel::{unbounded, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

use crate::core_client::CoreClient;

/// The subscription this client sends: every player, only the two event kinds
/// enrichment acts on. Narrowing it server-side is what keeps a
/// `position_changed` every second from crossing the seam at all.
const SUBSCRIPTION: &str = r#"{"players":null,"event_types":["song_changed","state_changed"]}"#;

/// The timings of the connection loop, in one place so tests can shrink them.
///
/// Nothing here is read from configuration: these are properties of the seam,
/// not of an installation, and the one that matters for correctness
/// (`idle_tick` versus the player's one-hour prune) has to stay far apart from
/// it by construction.
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    /// The first wait after a failed or lost connection.
    pub first_backoff: Duration,
    /// The ceiling the backoff doubles up to.
    pub max_backoff: Duration,
    /// How long a read may block before the loop wakes, and how long a
    /// connection may be silent before it is pinged.
    pub idle_tick: Duration,
    /// How long to wait for the TCP connect and for the WebSocket handshake.
    /// Without this a peer that accepts and never answers parks the thread.
    pub connect_timeout: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            first_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            idle_tick: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
        }
    }
}

/// Subscribe to the player daemon's event socket and forward what enrichment
/// cares about.
///
/// The thread runs for the life of the process. Dropping the returned receiver
/// is how a caller says it wants nothing: the next event that cannot be
/// delivered ends the loop, the connection is closed properly and the player
/// drops the subscription. That is the same contract the in-process bridge
/// had, and it has the same limit -- an idle player produces nothing to fail
/// on, so the exit happens at the next event rather than immediately.
pub fn start(events_url: &str, core: Arc<CoreClient>) -> Receiver<NowPlayingEvent> {
    start_with(events_url, core, Timings::default(), None)
}

/// `stop` is a second way out, and the only one that does not wait for an
/// event: every wait in the loop -- the backoff between attempts and the idle
/// tick between reads -- is a wait on it, so dropping its sender ends the
/// thread promptly even while the socket is silent. `start` passes `None`,
/// because the daemon has nothing to stop this with yet; the tests below use
/// it so that none of them leaves a thread reconnecting to a port another test
/// may later be given.
fn start_with(
    events_url: &str,
    core: Arc<CoreClient>,
    timings: Timings,
    stop: Option<Receiver<()>>,
) -> Receiver<NowPlayingEvent> {
    let (tx, rx) = unbounded();
    let url = events_url.to_string();
    std::thread::Builder::new()
        .name("now-playing-ws".into())
        .spawn(move || run(&url, core, tx, timings, stop))
        .expect("spawn now-playing ws subscriber");
    rx
}

/// Why a connection ended.
#[derive(Debug)]
enum Outcome {
    /// Nothing is listening any more; stop.
    Shutdown,
    /// The connection ended and should be re-established. `frames` is how
    /// many messages it delivered and `uptime` how long it lasted, which
    /// together decide whether the backoff resets.
    Lost {
        frames: usize,
        uptime: Duration,
        reason: String,
    },
}

fn run(
    url: &str,
    core: Arc<CoreClient>,
    tx: Sender<NowPlayingEvent>,
    timings: Timings,
    stop: Option<Receiver<()>>,
) {
    let mut backoff = timings.first_backoff;
    loop {
        match connect(url, &timings) {
            Ok(mut ws) => {
                let started = Instant::now();
                match subscribe_and_read(&mut ws, &core, &tx, &timings, &stop, started) {
                    Outcome::Shutdown => {
                        log::debug!(
                            "Now-playing subscriber: nothing is listening any more, closing the event socket"
                        );
                        // Drives the close handshake far enough for the player
                        // to remove the subscription rather than wait an hour
                        // to prune it.
                        let _ = ws.close(None);
                        let _ = ws.flush();
                        return;
                    }
                    Outcome::Lost {
                        frames,
                        uptime,
                        reason,
                    } => {
                        if should_reset_backoff(frames, uptime, timings.first_backoff) {
                            backoff = timings.first_backoff;
                        }
                        log::info!(
                            "Player daemon event socket ended after {:?} and {} frame(s) ({}); reconnecting in {:?}",
                            uptime,
                            frames,
                            reason,
                            backoff
                        );
                    }
                }
            }
            Err(e) => log::debug!(
                "Player daemon event socket not available ({}); retrying in {:?}",
                e,
                backoff
            ),
        }
        if wait_or_stop(&stop, backoff) {
            log::debug!("Now-playing subscriber: asked to stop while waiting to reconnect");
            return;
        }
        backoff = next_backoff(backoff, timings.max_backoff);
    }
}

/// Wait `interval`, and report whether the loop was asked to stop instead.
///
/// With no stop channel this is a plain sleep. With one, the wait *is* the
/// check: a stop that arrives -- or a sender that is dropped -- cuts the wait
/// short instead of being noticed up to `max_backoff` later.
fn wait_or_stop(stop: &Option<Receiver<()>>, interval: Duration) -> bool {
    match stop {
        None => {
            std::thread::sleep(interval);
            false
        }
        Some(rx) => !matches!(rx.recv_timeout(interval), Err(RecvTimeoutError::Timeout)),
    }
}

/// Whether the loop has been asked to stop, without waiting.
fn stop_requested(stop: &Option<Receiver<()>>) -> bool {
    match stop {
        None => false,
        Some(rx) => !matches!(rx.try_recv(), Err(TryRecvError::Empty)),
    }
}

/// Open one connection, with a bounded connect and handshake.
///
/// `tungstenite::connect` is not used: it hands back a stream wrapped for TLS
/// that may or may not be present, and it applies no timeout to either the
/// TCP connect or the handshake read -- a peer that accepts the connection and
/// then says nothing would park this thread for good. Doing the connect here
/// also means the read timeout is in place before the first byte.
fn connect(url: &str, timings: &Timings) -> Result<WebSocket<TcpStream>, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("{} is not a URL: {}", url, e))?;
    if parsed.scheme() != "ws" {
        return Err(format!(
            "{} is not a ws:// URL; this seam is plaintext over loopback",
            url
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("{} has no host", url))?;
    let port = parsed.port_or_known_default().unwrap_or(80);

    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("{}:{} does not resolve: {}", host, port, e))?
        .collect();
    let mut last_error = format!("{}:{} resolved to no addresses", host, port);
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timings.connect_timeout) {
            Ok(stream) => {
                // The handshake is a request and a reply, so the connect
                // timeout is the right bound for it too. It is replaced by
                // `idle_tick` once the connection is up.
                stream
                    .set_read_timeout(Some(timings.connect_timeout))
                    .map_err(|e| format!("read timeout not settable: {}", e))?;
                let _ = stream.set_nodelay(true);
                let (ws, _response) = tungstenite::client(url, stream)
                    .map_err(|e| format!("handshake failed: {}", e))?;
                ws.get_ref()
                    .set_read_timeout(Some(timings.idle_tick))
                    .map_err(|e| format!("read timeout not settable: {}", e))?;
                return Ok(ws);
            }
            Err(e) => last_error = format!("{}: {}", addr, e),
        }
    }
    Err(last_error)
}

/// Send the subscription, seed the current song, then forward frames until the
/// connection ends or nothing is listening.
fn subscribe_and_read(
    ws: &mut WebSocket<TcpStream>,
    core: &CoreClient,
    tx: &Sender<NowPlayingEvent>,
    timings: &Timings,
    stop: &Option<Receiver<()>>,
    started: Instant,
) -> Outcome {
    let mut frames = 0usize;
    if let Err(e) = ws.send(Message::Text(SUBSCRIPTION.to_string())) {
        return Outcome::Lost {
            frames,
            uptime: started.elapsed(),
            reason: format!("the subscription could not be sent: {}", e),
        };
    }

    // The gap-recovery step. On the first connect this also means a daemon
    // restart mid-track enriches that track instead of waiting for the next
    // one, which the in-process bridge never did. The cost is one possibly
    // redundant `SongChanged` per connect; the workers already tolerate that,
    // since a player that re-announces the same song produces the same thing.
    match core.now_playing() {
        Ok(Some((source, song))) => {
            if tx
                .send(NowPlayingEvent::SongChanged {
                    source,
                    song: Some(song),
                })
                .is_err()
            {
                return Outcome::Shutdown;
            }
        }
        Ok(None) => {}
        Err(e) => log::debug!(
            "No now-playing seed on connect; the player daemon did not answer ({})",
            e
        ),
    }

    let mut last_sent = Instant::now();
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                frames += 1;
                if let Some(event) = parse_frame(&text) {
                    if tx.send(event).is_err() {
                        return Outcome::Shutdown;
                    }
                }
            }
            // Pongs answering our keepalive, and the player's own pings, which
            // tungstenite has already queued a reply to.
            Ok(_) => frames += 1,
            Err(tungstenite::Error::Io(e)) if is_timeout(&e) => {}
            Err(e) => {
                return Outcome::Lost {
                    frames,
                    uptime: started.elapsed(),
                    reason: e.to_string(),
                }
            }
        }

        if stop_requested(stop) {
            return Outcome::Shutdown;
        }

        // Checked on every pass, not only after a timeout: a player changing
        // songs more often than `idle_tick` would otherwise never let the read
        // time out, and the player counts only what we send as activity.
        if last_sent.elapsed() >= timings.idle_tick {
            if let Err(e) = ws.send(Message::Ping(Vec::new())) {
                return Outcome::Lost {
                    frames,
                    uptime: started.elapsed(),
                    reason: format!("keepalive ping failed: {}", e),
                };
            }
            last_sent = Instant::now();
        }
    }
}

/// A read that hit its timeout rather than a broken connection. Linux reports
/// `SO_RCVTIMEO` as `WouldBlock`, Windows as `TimedOut`; tungstenite treats
/// both as retryable and keeps any half-read frame in its own buffer, so the
/// next read resumes where this one stopped.
fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// The next wait after a failure: double, up to the ceiling.
fn next_backoff(current: Duration, max: Duration) -> Duration {
    current.saturating_mul(2).min(max)
}

/// Whether a connection that has just ended earns a fresh backoff.
///
/// It has to have delivered something -- the player sends a `welcome` frame
/// immediately, so any working connection clears this at once -- *and* to have
/// lasted at least as long as the first backoff. Without the second condition
/// a peer that accepts, welcomes and hangs up would be retried once a second
/// for as long as it kept doing it, which is the busy loop this backoff exists
/// to prevent.
fn should_reset_backoff(frames: usize, uptime: Duration, first_backoff: Duration) -> bool {
    frames > 0 && uptime >= first_backoff
}

/// Map one server frame onto a `NowPlayingEvent`, or `None` for a frame that
/// is not one.
///
/// `player_name` and `player_id` are read from the top level, not from
/// `source`: `convert_to_websocket_message` in `src/api/events.rs` builds
/// `source.player_id` by appending the hardcoded MPD port to the player name,
/// so it is wrong for every other player. `doc/websocket.md` documents that
/// and tells clients to do exactly this.
pub(crate) fn parse_frame(text: &str) -> Option<NowPlayingEvent> {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            log::debug!("Unparseable frame on the player event socket: {}", e);
            return None;
        }
    };
    // `welcome`, `subscription_updated` and `error` carry no player, and
    // neither does `volume_changed`; none of them is a now-playing event.
    let event_type = v.get("type").and_then(|t| t.as_str())?;
    if event_type != "song_changed" && event_type != "state_changed" {
        log::trace!("Ignoring {} on the player event socket", event_type);
        return None;
    }
    let player_name = v.get("player_name").and_then(|n| n.as_str())?;
    let player_id = v
        .get("player_id")
        .and_then(|i| i.as_str())
        .unwrap_or_default();
    let source = PlayerSource::new(player_name.to_string(), player_id.to_string());

    match event_type {
        // A null song is what a stopping player reports, and the Last.fm
        // worker clears its track data on it, so it has to survive as
        // `Some(SongChanged { song: None })` rather than be dropped.
        "song_changed" => Some(NowPlayingEvent::SongChanged {
            source,
            song: v
                .get("song")
                .filter(|s| !s.is_null())
                .and_then(|s| match serde_json::from_value::<Song>(s.clone()) {
                    Ok(song) => Some(song),
                    Err(e) => {
                        log::debug!("song_changed carried an unreadable song: {}", e);
                        None
                    }
                }),
        }),
        // `state` is the `Display` form of `PlaybackState`, which is also its
        // serde representation, so one `from_value` covers both.
        "state_changed" => match serde_json::from_value::<PlaybackState>(
            v.get("state").cloned().unwrap_or(serde_json::Value::Null),
        ) {
            Ok(state) => Some(NowPlayingEvent::StateChanged { source, state }),
            Err(e) => {
                log::debug!("state_changed carried an unreadable state: {}", e);
                None
            }
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use tungstenite::accept;

    /// Long enough that a loaded machine does not fail the test, short enough
    /// that a hung one does not hang the suite. Nothing asserts on elapsed
    /// time; every wait here is on a signal that either arrives or does not.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// A player daemon that is not there. `now_playing` fails at once against
    /// a closed loopback port, so the seed is skipped and only what the test
    /// server sends reaches the channel.
    fn absent_core() -> Arc<CoreClient> {
        Arc::new(CoreClient::new("http://127.0.0.1:1/api"))
    }

    /// Small enough that a test never waits on the clock, still far enough
    /// from zero that nothing spins.
    fn quick() -> Timings {
        Timings {
            first_backoff: Duration::from_millis(5),
            max_backoff: Duration::from_millis(500),
            idle_tick: Duration::from_millis(50),
            connect_timeout: Duration::from_secs(5),
        }
    }

    /// A subscriber that stops when the returned sender is dropped.
    ///
    /// Every test uses this rather than `start`: an ephemeral port that a test
    /// server has finished with can be handed to a later test, and a
    /// subscriber still reconnecting to it would steal that test's connection.
    /// Holding the sender for the length of the test and dropping it at the end
    /// makes each one self-contained.
    fn subscriber(addr: std::net::SocketAddr, core: Arc<CoreClient>) -> (Receiver<NowPlayingEvent>, Sender<()>) {
        let (stop_tx, stop_rx) = unbounded();
        let rx = start_with(
            &format!("ws://{}/api/events", addr),
            core,
            quick(),
            Some(stop_rx),
        );
        (rx, stop_tx)
    }

    /// The frame `convert_to_websocket_message` actually produces, `source`
    /// and its wrong `player_id` included.
    fn song_changed_frame(title: &str) -> String {
        serde_json::json!({
            "type": "song_changed",
            "player_name": "mpd",
            "player_id": "mpd:1",
            "song": {"title": title, "artist": "Nightwish"},
            "source": {"player_name": "mpd", "player_id": "mpd:6600"}
        })
        .to_string()
    }

    #[test]
    fn a_song_changed_frame_becomes_an_event_and_the_subscription_is_sent_first() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = accept(stream).unwrap();
            let first = ws.read().unwrap().into_text().unwrap();
            // Parsed, not matched as a substring: `serde_json::Value` is
            // BTreeMap-backed and would reorder any literal comparison.
            let subscription: serde_json::Value = serde_json::from_str(&first).unwrap();
            assert!(
                subscription["players"].is_null(),
                "all players, not a filtered list: {}",
                first
            );
            let types: Vec<&str> = subscription["event_types"]
                .as_array()
                .expect("event_types must be an array")
                .iter()
                .map(|t| t.as_str().unwrap())
                .collect();
            assert_eq!(types, vec!["song_changed", "state_changed"]);

            ws.send(Message::Text(
                r#"{"type":"welcome","client_id":1,"message":"Connected"}"#.to_string(),
            ))
            .unwrap();
            ws.send(Message::Text(song_changed_frame("Nemo"))).unwrap();
            ws
        });

        let (rx, _stop) = subscriber(addr, absent_core());
        let event = rx.recv_timeout(PATIENCE).expect("an event should arrive");
        match event {
            NowPlayingEvent::SongChanged { source, song } => {
                assert_eq!(source.player_name, "mpd");
                // The top-level player_id, not source.player_id.
                assert_eq!(source.player_id, "mpd:1");
                assert_eq!(song.unwrap().title.as_deref(), Some("Nemo"));
            }
            other => panic!("unexpected {:?}", other),
        }
        // The welcome frame must not have produced an event of its own.
        assert!(rx.try_recv().is_err(), "welcome must not become an event");
        let _ws = server.join().unwrap();
    }

    /// The point of the whole module: a connection that drops has to come
    /// back. The proof is an event delivered over a *second* connection, so
    /// nothing here depends on how long the reconnect took.
    #[test]
    fn a_dropped_connection_is_re_established_and_events_flow_again() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for title in ["first", "second"] {
                let (stream, _) = listener.accept().unwrap();
                let mut ws = accept(stream).unwrap();
                ws.read().unwrap(); // the subscription
                ws.send(Message::Text(song_changed_frame(title))).unwrap();
                // Dropping `ws` closes the TCP connection with no close
                // handshake -- what a killed daemon looks like from here.
            }
        });

        let (rx, _stop) = subscriber(addr, absent_core());

        for expected in ["first", "second"] {
            match rx.recv_timeout(PATIENCE) {
                Ok(NowPlayingEvent::SongChanged { song, .. }) => {
                    assert_eq!(song.unwrap().title.as_deref(), Some(expected));
                }
                other => panic!("expected {} over its own connection, got {:?}", expected, other),
            }
        }
        server.join().unwrap();
    }

    /// An idle connection has to send something, or the player prunes the
    /// subscription after an hour and this side goes silently deaf. The
    /// assertion is on what the server receives next, not on when.
    #[test]
    fn an_idle_connection_is_pinged_so_the_player_keeps_the_subscription() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            // So that a subscriber which never pings fails this test rather
            // than hanging it.
            stream.set_read_timeout(Some(PATIENCE)).unwrap();
            let mut ws = accept(stream).unwrap();
            ws.read().unwrap(); // the subscription
            // Nothing is sent to the client at all, so the next thing it sends
            // can only be the keepalive.
            let next = ws.read();
            (ws, next)
        });

        let (_rx, _stop) = subscriber(addr, absent_core());
        let (_ws, next) = server.join().expect("the server thread should finish");
        assert!(
            matches!(next, Ok(Message::Ping(_))),
            "an idle subscriber must ping, or the player prunes the subscription after an hour; got {:?}",
            next
        );
    }

    /// Dropping the receiver is how the daemon says nothing wants these
    /// events, and the subscriber has to notice rather than hold a
    /// subscription open. Observed by the thread finishing, not by a timer.
    #[test]
    fn dropping_the_receiver_ends_the_subscriber() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (subscribed_tx, subscribed_rx) = unbounded::<()>();
        let (go_tx, go_rx) = unbounded::<()>();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = accept(stream).unwrap();
            ws.read().unwrap(); // the subscription
            subscribed_tx.send(()).unwrap();
            go_rx.recv().unwrap(); // the receiver has been dropped by now
            ws.send(Message::Text(song_changed_frame("Nemo"))).unwrap();
            // The connection is deliberately left open and healthy: the
            // subscriber must leave of its own accord.
            while let Ok(message) = ws.read() {
                if matches!(message, Message::Close(_)) {
                    break;
                }
            }
        });

        let (tx, rx) = unbounded();
        let (done_tx, done_rx) = unbounded();
        let url = format!("ws://{}/api/events", addr);
        // No stop channel here: the dropped receiver has to be enough on its
        // own, which is the whole point of the test.
        std::thread::spawn(move || {
            run(&url, absent_core(), tx, quick(), None);
            let _ = done_tx.send(());
        });

        subscribed_rx
            .recv_timeout(PATIENCE)
            .expect("the subscriber should subscribe");
        drop(rx);
        go_tx.send(()).unwrap();
        done_rx
            .recv_timeout(PATIENCE)
            .expect("the subscriber must stop once nothing is listening");
        server.join().unwrap();
    }

    /// The seed that covers a gap. A player that is playing something when the
    /// connection comes up produces one `SongChanged` before any frame does.
    #[test]
    fn the_current_song_is_emitted_on_connect() {
        use crate::external_coverart::stub_server::StubServer;

        let core = StubServer::serving(
            200,
            r#"{"player":{"name":"spotify","id":"spotify:1"},"song":{"title":"Ghost Love Score"},"state":"playing","shuffle":false,"loop_mode":"none","position":3.0}"#,
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = accept(stream).unwrap();
            ws.read().unwrap(); // the subscription
            ws
        });

        let (rx, _stop) = subscriber(addr, Arc::new(CoreClient::new(&core.base_url())));
        match rx.recv_timeout(PATIENCE) {
            Ok(NowPlayingEvent::SongChanged { source, song }) => {
                assert_eq!(source.player_name, "spotify");
                assert_eq!(song.unwrap().title.as_deref(), Some("Ghost Love Score"));
            }
            other => panic!("expected the current song on connect, got {:?}", other),
        }
        let _ws = server.join().unwrap();
    }

    #[test]
    fn a_state_changed_frame_carries_the_state_as_the_player_writes_it() {
        let frame = serde_json::json!({
            "type": "state_changed",
            "player_name": "mpd",
            "player_id": "mpd:1",
            "state": "paused",
            "source": {"player_name": "mpd", "player_id": "mpd:6600"}
        })
        .to_string();
        assert_eq!(
            parse_frame(&frame),
            Some(NowPlayingEvent::StateChanged {
                source: PlayerSource::new("mpd".into(), "mpd:1".into()),
                state: PlaybackState::Paused,
            })
        );
    }

    /// The frames the player sends that are not events, plus the one event
    /// that has no player at all.
    #[test]
    fn frames_that_are_not_now_playing_events_are_ignored() {
        for frame in [
            r#"{"type":"welcome","client_id":3,"message":"Connected to ACR WebSocket API"}"#,
            r#"{"type":"subscription_updated","message":"Subscription updated successfully"}"#,
            r#"{"type":"error","message":"Invalid message format"}"#,
            r#"{"type":"position_changed","player_name":"mpd","player_id":"mpd:1","position":12.0}"#,
            r#"{"type":"volume_changed","control_name":"Digital","percentage":40}"#,
            r#"{"type":"song_changed","player_id":"mpd:1","song":{"title":"Nemo"}}"#,
            r#"{"type":"state_changed","player_name":"mpd","player_id":"mpd:1","state":"levitating"}"#,
            "not json at all",
            "{}",
        ] {
            assert_eq!(parse_frame(frame), None, "should be ignored: {}", frame);
        }
    }

    /// A stopping player reports a null song, and the Last.fm worker clears
    /// its track data on it, so the event has to arrive rather than be
    /// dropped as unparseable.
    #[test]
    fn a_song_changed_to_null_is_still_an_event() {
        let frame = r#"{"type":"song_changed","player_name":"mpd","player_id":"mpd:1","song":null}"#;
        assert_eq!(
            parse_frame(frame),
            Some(NowPlayingEvent::SongChanged {
                source: PlayerSource::new("mpd".into(), "mpd:1".into()),
                song: None,
            })
        );
    }

    /// The backoff schedule, asserted on the function rather than on elapsed
    /// time: 1, 2, 4, ... capped, and never zero.
    #[test]
    fn the_backoff_doubles_up_to_the_ceiling() {
        let max = Duration::from_secs(30);
        let mut waits = Vec::new();
        let mut current = Duration::from_secs(1);
        for _ in 0..8 {
            waits.push(current);
            current = next_backoff(current, max);
        }
        assert_eq!(
            waits,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
                Duration::from_secs(16),
                max,
                max,
                max,
            ]
        );
        // Whatever it is handed, it never returns a zero wait -- that would be
        // the busy loop this exists to prevent.
        assert!(next_backoff(Duration::from_millis(1), max) > Duration::ZERO);
        assert_eq!(next_backoff(Duration::from_secs(3600), max), max);
    }

    /// A connection has to earn a fresh backoff. Accept-welcome-hang-up must
    /// not, or it becomes a one-second reconnect loop forever.
    #[test]
    fn only_a_connection_that_lasted_resets_the_backoff() {
        let first = Duration::from_secs(1);
        assert!(!should_reset_backoff(0, Duration::from_secs(600), first));
        assert!(!should_reset_backoff(1, Duration::from_millis(3), first));
        assert!(should_reset_backoff(1, first, first));
        assert!(should_reset_backoff(12, Duration::from_secs(3600), first));
    }

    /// A URL this seam cannot serve fails as an error, not as a panic and not
    /// as a silent no-op.
    #[test]
    fn a_url_that_is_not_ws_is_refused() {
        let timings = quick();
        assert!(connect("wss://127.0.0.1:1/api/events", &timings)
            .unwrap_err()
            .contains("loopback"));
        assert!(connect("http://127.0.0.1:1/api/events", &timings).is_err());
        assert!(connect("not a url", &timings).is_err());
        assert!(connect("ws://127.0.0.1:1/api/events", &timings).is_err());
    }
}
