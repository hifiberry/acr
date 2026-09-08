//! The metadata daemon as a process, and the configuration file it ships with.
//!
//! Two kinds of test live here, and both need to be outside the library.
//!
//! The first read `configs/metadata.json`, the file the package installs to
//! `/etc/audiocontrol/metadata.json`. Nothing compiles that file, so every
//! mistake in it is a runtime one, and the two that matter are silent: a
//! missing `core.url` points the daemon at its own port (see
//! `startup::required_core_settings`), and a `security_store.path` that is not
//! the one `debian/postinst` migrates the credentials to leaves the daemon
//! reading an empty store with no error at all.
//!
//! The second start the built binary. They are here rather than in a unit
//! test because `CARGO_BIN_EXE_*` is set only for integration test targets,
//! and because "refuses to start" is a property of a process and not of a
//! function: a unit test on the gate cannot tell a daemon that stopped at the
//! gate from one that got past it and then failed to bind a port.

use std::io::Read;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use audiocontrol_metadata::startup;

/// The workspace root, from this crate's manifest directory rather than from
/// the working directory.
///
/// `cargo test` runs a test binary with its *package* directory as the working
/// directory, which for this crate is `crates/audiocontrol-metadata` and not
/// the workspace root -- so a relative `configs/metadata.json` would not
/// resolve, and a test that skipped itself when it could not find the file
/// would pass vacuously forever.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root should be two levels above this crate")
}

/// `configs/metadata.json`, parsed.
fn shipped_config() -> serde_json::Value {
    let path = workspace_root().join("configs/metadata.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} must be valid JSON: {}", path.display(), e))
}

/// What the shipped file says `webserver.port` is.
fn shipped_webserver_port(config: &serde_json::Value) -> u64 {
    acr_types::config::get_service_config(config, "webserver")
        .and_then(|ws| ws.get("port"))
        .and_then(|p| p.as_u64())
        .expect("the shipped configuration must name a webserver port")
}

/// The shipped file passes the gate that refuses to derive a URL, and names
/// the player daemon's own port while doing it.
#[test]
fn the_shipped_configuration_names_the_player_daemon_explicitly() {
    let settings = startup::required_core_settings(&shipped_config())
        .expect("configs/metadata.json must name core.url explicitly");

    assert_eq!(settings.url, "http://127.0.0.1:1080/api");
}

/// The failure the gate exists to catch, asserted against the shipped file
/// rather than against a hand-written one: whatever port this daemon binds,
/// the seam must not point back at it.
///
/// A `core.url` copied from the wrong file, or a `webserver.port` changed
/// without it, both land here. Neither would error at runtime -- the daemon
/// would subscribe to its own event stream and poll its own library, and
/// answer nothing on either.
#[test]
fn the_shipped_configuration_does_not_point_the_seam_at_itself() {
    let config = shipped_config();
    let settings = startup::required_core_settings(&config).expect("core.url must be present");
    let own_port = shipped_webserver_port(&config);

    assert!(
        !settings.url.contains(&format!(":{}", own_port)),
        "core.url {} names this daemon's own port {}",
        settings.url,
        own_port
    );
}

/// The credential store is the metadata daemon's own file, at the exact path
/// `debian/postinst` copies the existing store to.
///
/// Two processes writing one store lose credentials -- `SecurityStore::
/// save_to_file` truncates and rewrites from memory, so whichever daemon
/// writes last discards what the other learned. The split is the resolution,
/// and it only works if this path and the one the migration writes to are the
/// same string. A mismatch is silent: the daemon falls back to the relative
/// `secrets/security_store.json` and every credential lookup misses.
#[test]
fn the_shipped_configuration_keeps_the_credential_store_out_of_the_player_daemons_way() {
    let config = shipped_config();
    let path = acr_types::config::get_service_config(&config, "security_store")
        .and_then(|s| s.get("path"))
        .and_then(|p| p.as_str())
        .expect("the shipped configuration must name a security_store path");

    assert_eq!(path, "/var/lib/audiocontrol/metadata/security_store.json");
}

/// `settingsdb.path` is a *directory*: `SettingsDb::initialize` appends
/// `settings.db` to it.
///
/// A path written as `.../metadata/settings.db` -- which is how the earlier
/// spec's sketch of this file gave it, and the obvious thing to write -- makes
/// a *directory* of that name with `settings.db` inside it. Nothing errors. The
/// artist image selections and favourites then live somewhere no upgrade copies
/// data to and no other tool looks, which is a silent data loss on the first
/// start rather than a failure to start.
#[test]
fn the_shipped_configuration_points_the_settings_database_at_a_directory() {
    let config = shipped_config();
    let path = acr_types::config::get_service_config(&config, "settingsdb")
        .and_then(|s| s.get("path"))
        .and_then(|p| p.as_str())
        .expect("the shipped configuration must name a settingsdb path");

    assert!(
        !path.ends_with(".db"),
        "settingsdb.path is the directory settings.db is created in, not the file: {}",
        path
    );
    assert_eq!(path, "/var/lib/audiocontrol/metadata");
}

/// The Last.fm worker is configured from the `action_plugins` array, not from
/// `services.lastfm`, and a file with only the latter scrobbles nothing.
///
/// `services.lastfm.enable` brings up the *client* (`initialize_in_process`);
/// the `action_plugins` entry is what `now_playing::start` needs to run the
/// scrobbler and the now-playing update. With the entry absent it starts the
/// other workers and says so -- `musicbrainz.enable` alone supplies one, so the
/// log reads "Now-playing enrichment started with 1 worker(s)" either way. So
/// there is nothing in the log to notice: enrichment continues, the daemon
/// reports itself healthy, and scrobbling is simply absent.
#[test]
fn the_shipped_configuration_configures_the_lastfm_worker() {
    let lastfm = startup::lastfm_worker_config(&shipped_config())
        .expect("configs/metadata.json must carry the action_plugins lastfm entry");

    assert!(lastfm.enabled, "the entry should be enabled");
    assert!(lastfm.scrobble, "scrobbling should be on");
}

/// The daemon is reachable only through nginx, so it binds loopback.
#[test]
fn the_shipped_configuration_binds_loopback_on_the_metadata_port() {
    let config = shipped_config();
    let webserver = acr_types::config::get_service_config(&config, "webserver")
        .expect("the shipped configuration must have a webserver section");

    assert_eq!(webserver.get("host").and_then(|h| h.as_str()), Some("127.0.0.1"));
    assert_eq!(shipped_webserver_port(&config), 1084);
}

// ---------------------------------------------------------------------------
// The daemon as a process.

/// A port nothing is listening on, taken by binding and releasing it.
///
/// Racy in principle and not in practice: the tests below are the only things
/// in this binary that bind, and a port the kernel just handed out is not
/// handed out again immediately.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("the kernel should have a spare port");
    listener
        .local_addr()
        .expect("a bound listener has an address")
        .port()
}

/// A configuration for a daemon that touches nothing outside `dir`.
///
/// Everything the daemon opens is redirected under the temporary directory, so
/// a test run neither reads nor writes the paths the package owns -- which is
/// also what keeps this passing as an unprivileged user, the way CI runs it.
fn sandbox_config(dir: &std::path::Path, port: u16) -> serde_json::Value {
    serde_json::json!({
        "services": {
            "webserver": { "enable": true, "host": "127.0.0.1", "port": port },
            "datastore": {
                "attribute_cache": { "dbfile": dir.join("attributes.db").to_str().unwrap() },
                "image_cache_path": dir.join("images").to_str().unwrap(),
                "user_image_path": dir.join("user-images").to_str().unwrap(),
                "artist_store": { "cache_dir": dir.join("artists").to_str().unwrap() }
            },
            "settingsdb": { "path": dir.join("db").to_str().unwrap() },
            "security_store": { "path": dir.join("security_store.json").to_str().unwrap() },
            "musicbrainz": { "enable": false },
            "theaudiodb": { "enable": false },
            "fanarttv": { "enable": false },
            "lastfm": { "enable": false },
            "external_coverart": { "enable": false }
        }
    })
}

/// Write `config` into `dir` and return its path.
fn write_config(dir: &std::path::Path, config: &serde_json::Value) -> PathBuf {
    let path = dir.join("metadata.json");
    std::fs::write(&path, serde_json::to_vec_pretty(config).unwrap()).expect("write the config");
    path
}

/// The daemon, started against `config_path`.
fn spawn_daemon(config_path: &std::path::Path) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_audiocontrol-metadata"))
        .arg("-c")
        .arg(config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the metadata daemon binary should be runnable")
}

/// A configuration with no `core.url` must stop the daemon at start-up, with a
/// message naming the key.
///
/// The pairing with `a_core_url_lets_the_daemon_start` below is what makes this
/// meaningful: on its own, a daemon that exited non-zero because a path was
/// unwritable or a port was taken would pass it just as well. That test uses
/// the *same* configuration with `core.url` added, so the only difference
/// between a refusal and a start is the key this one is about.
///
/// The exit is waited for with a bound rather than with `Command::output`, and
/// that is not defensive tidiness. Removing the gate was tried, to check this
/// test catches it: the daemon then *starts* -- deriving its own port, which is
/// the whole failure -- and `output()` waits for a process that never exits.
/// The test hung instead of failing, which in CI reads as a stuck runner rather
/// than as a broken daemon.
#[test]
fn a_configuration_without_a_core_url_refuses_to_start() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let config = sandbox_config(dir.path(), free_port());
    let path = write_config(dir.path(), &config);

    let mut child = spawn_daemon(&path);

    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        match child.try_wait().expect("waiting on the child") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => break None,
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };

    // Killed *before* the pipes are read, and that order is the whole of why
    // this is not `Command::output()`: `read_to_string` on a child's stderr
    // waits for EOF, and a daemon that is still running never sends one. Read
    // first and the bound above buys nothing -- the test hangs one line later
    // instead.
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let said = read_output(&mut child);

    let status = status.unwrap_or_else(|| {
        panic!("the daemon is still running without a core.url; it is talking to itself. It said:\n{said}")
    });
    assert!(
        !status.success(),
        "the daemon exited cleanly without a core.url. It said:\n{said}"
    );
    assert!(
        said.contains("core.url"),
        "the daemon must say which key is missing, not just fail. It said:\n{said}"
    );
}

/// A port it cannot have must be a non-zero exit, and a prompt one.
///
/// This is what `Restart=on-failure` in the systemd unit turns on. Exit 0 and
/// systemd leaves the daemon dead, which to whoever is using the device looks
/// like metadata that has stopped working rather than like a service that
/// failed to start. The ordinary cause is a second copy of the daemon, or
/// something else already on 1084.
///
/// **Not a racy test.** The listener is bound before the daemon starts and held
/// until after it has exited, so the port is occupied for the whole of the
/// window that matters -- rather than bound, released, and hoped to be still
/// free at the moment it counts.
///
/// The 15 s bound is doing two jobs. It fails a daemon that never exits, and it
/// fails one that exits only after `start_after_core_is_listening` has probed
/// the player daemon for its full 30 s: a failure announced half a minute late
/// is a `systemctl start` that has already failed while still looking like one
/// that is starting.
#[test]
fn a_port_it_cannot_bind_is_a_failure_and_not_a_clean_exit() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let occupied = TcpListener::bind("127.0.0.1:0").expect("a spare port to sit on");
    let port = occupied
        .local_addr()
        .expect("a bound listener has an address")
        .port();

    let mut config = sandbox_config(dir.path(), port);
    config["services"]["core"] = serde_json::json!({ "url": "http://127.0.0.1:1/api" });
    let path = write_config(dir.path(), &config);

    let mut child = spawn_daemon(&path);

    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait().expect("waiting on the child") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => break None,
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let said = read_output(&mut child);

    let status = status.unwrap_or_else(|| {
        panic!("the daemon was still running 15 s after failing to bind {port}. It said:\n{said}")
    });
    assert!(
        !status.success(),
        "the daemon exited 0 having never bound {port}, so Restart=on-failure will not \
         restart it and systemd will leave it dead. It said:\n{said}"
    );
    assert!(
        said.contains(&port.to_string()),
        "the failure must name the port it could not have. It said:\n{said}"
    );

    // Held until here on purpose: released earlier and the daemon might have
    // bound the port after all, which is the race this test is written to avoid.
    drop(occupied);
}

/// Everything the child has written so far, both streams.
///
/// Read after the process has ended, or after it has been given up on: the
/// pipes are small enough for what these daemons say at start-up, and a daemon
/// that had filled one would have blocked rather than exited.
fn read_output(child: &mut std::process::Child) -> String {
    let mut said = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut said);
    }
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut said);
    }
    said
}

/// The same configuration with `core.url` added starts, serves the
/// capabilities route on its own port, and stops when asked.
///
/// This is the falsification of the test above kept as a test, and it asserts
/// three things that would otherwise be checked by hand: that the daemon
/// ignites (no route collision between the bare and `/metadata` mounts), that
/// `/api/metadata/capabilities` is where the spec puts it, and that a SIGTERM
/// arriving while the daemon is still waiting for the player daemon's API
/// stops it promptly instead of holding the process for the whole 30 s probe.
///
/// `core.url` names port 1 deliberately: nothing can be listening there, so
/// the connection is refused at once and this test never depends on a player
/// daemon existing.
#[test]
fn a_core_url_lets_the_daemon_start() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let port = free_port();
    let mut config = sandbox_config(dir.path(), port);
    config["services"]["core"] = serde_json::json!({ "url": "http://127.0.0.1:1/api" });
    let path = write_config(dir.path(), &config);

    let mut child = spawn_daemon(&path);

    let capabilities = format!("http://127.0.0.1:{}/api/metadata/capabilities", port);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut answered = false;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("waiting on the child") {
            let said = read_output(&mut child);
            panic!("the daemon exited early ({status}):\n{said}");
        }
        if ureq::get(&capabilities).call().is_ok() {
            answered = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let stopped = if answered {
        // SIGTERM, the signal systemd sends. The daemon is inside its wait for
        // the player daemon's API at this point, which is exactly the window
        // that used to cost the player daemon its whole force-exit budget.
        //
        // Sent with `kill(1)` rather than through a crate: neither this
        // package nor its dev-dependencies carry `libc`, and adding one to
        // send one signal is a poor trade. `Child::kill` is no use here --
        // that is SIGKILL, which would prove nothing about the shutdown path.
        let killed = Command::new("kill")
            .arg("-TERM")
            .arg(child.id().to_string())
            .status()
            .expect("kill(1) should be available");
        assert!(killed.success(), "could not signal the daemon");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut exited = false;
        while Instant::now() < deadline {
            if child.try_wait().expect("waiting on the child").is_some() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        exited
    } else {
        false
    };

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        answered,
        "the daemon never answered {} -- with core.url given it must start and serve",
        capabilities
    );
    assert!(
        stopped,
        "the daemon did not stop within 10 s of a SIGTERM; systemd would SIGKILL it"
    );
}
