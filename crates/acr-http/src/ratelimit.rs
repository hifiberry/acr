use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use parking_lot::{Condvar, Mutex};
use std::time::{Duration, Instant};
use once_cell::sync::Lazy;
use log::{debug, warn};

/// Minimum spacing applied to a service nobody registered.
///
/// One second, not the 500 ms this used to be. Every provider behind this
/// limiter is a courtesy-access API -- MusicBrainz publishes one request per
/// second, the others are no more generous -- so the safe assumption for an
/// unregistered name is the politest rate we ever need, not twice it.
const DEFAULT_RATE_LIMIT_MS: u64 = 1000;

/// Requests a service may have in flight at once when nobody said otherwise.
///
/// These are polite-client APIs, not throughput-oriented ones: they cap
/// concurrent connections per client and answer the surplus with 503.
const DEFAULT_MAX_CONCURRENT: usize = 1;

/// How long a thread waiting for a free slot sleeps before re-checking.
/// The wakeup on release is what normally moves it; this only bounds the
/// damage of a missed notification.
const SLOT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// What the limiter knows about one service.
struct ServiceLimit {
    /// When the most recent request was allowed to start
    last_start: Instant,
    /// Minimum delay between request starts in milliseconds
    minimum_delay_ms: u64,
    /// Maximum requests allowed in flight at once
    max_concurrent: usize,
    /// Requests currently in flight, i.e. permits outstanding
    in_flight: usize,
}

impl ServiceLimit {
    fn new(minimum_delay_ms: u64, max_concurrent: usize) -> Self {
        let now = Instant::now();
        ServiceLimit {
            // Let the first request through immediately.
            last_start: now
                .checked_sub(Duration::from_millis(minimum_delay_ms))
                .unwrap_or(now),
            minimum_delay_ms,
            max_concurrent: max_concurrent.max(1),
            in_flight: 0,
        }
    }
}

/// RateLimiter ensures that API calls to external services respect rate limits
pub struct RateLimiter {
    /// Maps service names to their state
    services: HashMap<String, ServiceLimit>,
}

impl RateLimiter {
    fn new() -> Self {
        RateLimiter {
            services: HashMap::new(),
        }
    }

    fn entry(&mut self, service_name: &str) -> &mut ServiceLimit {
        if !self.services.contains_key(service_name) {
            debug!(
                "Using default rate limit for unregistered service '{}': {} ms, {} concurrent",
                service_name, DEFAULT_RATE_LIMIT_MS, DEFAULT_MAX_CONCURRENT
            );
            self.services.insert(
                service_name.to_string(),
                ServiceLimit::new(DEFAULT_RATE_LIMIT_MS, DEFAULT_MAX_CONCURRENT),
            );
        }
        self.services
            .get_mut(service_name)
            .expect("service was just inserted")
    }
}

// Global singleton. The mutex guards only the bookkeeping above: a thread that
// has to wait does so on the condvar, which releases the mutex, so a service
// waiting on a slow provider never stalls a different service.
static RATE_LIMITER: Lazy<Mutex<RateLimiter>> = Lazy::new(|| Mutex::new(RateLimiter::new()));
static SLOT_RELEASED: Condvar = Condvar::new();

thread_local! {
    /// Services for which this thread already holds a permit. Used only to
    /// keep a nested acquisition from waiting on the thread itself.
    static HELD_BY_THIS_THREAD: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Register a rate limit for a specific service, with one request in flight
/// at a time.
///
/// Re-registering an already known service updates its limits and leaves any
/// requests currently in flight accounted for.
///
/// # Arguments
/// * `service_name` - Name of the service to register
/// * `minimum_delay_ms` - Minimum delay between request starts in milliseconds
pub fn register_service(service_name: &str, minimum_delay_ms: u64) {
    register_service_with_concurrency(service_name, minimum_delay_ms, DEFAULT_MAX_CONCURRENT);
}

/// Register a rate limit and a maximum number of concurrent requests.
///
/// # Arguments
/// * `service_name` - Name of the service to register
/// * `minimum_delay_ms` - Minimum delay between request starts in milliseconds
/// * `max_concurrent` - Requests this service may have in flight at once
pub fn register_service_with_concurrency(
    service_name: &str,
    minimum_delay_ms: u64,
    max_concurrent: usize,
) {
    let mut limiter = RATE_LIMITER.lock();
    match limiter.services.get_mut(service_name) {
        Some(existing) => {
            existing.minimum_delay_ms = minimum_delay_ms;
            existing.max_concurrent = max_concurrent.max(1);
        }
        None => {
            limiter.services.insert(
                service_name.to_string(),
                ServiceLimit::new(minimum_delay_ms, max_concurrent),
            );
        }
    }
    drop(limiter);
    SLOT_RELEASED.notify_all();
    debug!(
        "Registered rate limit for service '{}': {} ms, {} concurrent",
        service_name,
        minimum_delay_ms,
        max_concurrent.max(1)
    );
}

/// One in-flight request against a rate-limited service.
///
/// The slot is occupied for as long as this value lives and is given back when
/// it drops, so the request it stands for must happen while it is still in
/// scope. Bind it to a real name:
///
/// ```ignore
/// let _permit = ratelimit::rate_limit("musicbrainz");
/// let body = http_get(url)?;   // the permit is still held here
/// ```
///
/// A permit bound to the wildcard pattern -- `let _ = rate_limit(..)` -- is
/// dropped at once, which spaces the request starts but bounds nothing. That
/// is exactly the defect this type was introduced to fix: against a provider
/// answering in 15 s, one start per second put a dozen requests in flight and
/// the surplus came back 503.
#[must_use = "bind the permit for the whole request; dropping it immediately spaces the starts but leaves concurrency unbounded"]
pub struct Permit {
    service: String,
    /// False for a nested acquisition, which occupies no slot of its own.
    holds_slot: bool,
    /// Makes the permit `!Send`, so it cannot be dropped on a thread other
    /// than the one that took it.
    ///
    /// The re-entrancy marker cleared on drop lives in a thread-local. A
    /// permit released on a different thread would clear that thread's marker
    /// instead and leave the originating thread's set forever, so every later
    /// acquisition on it would be treated as nested and take no slot -- the
    /// concurrency bound silently gone for that thread and service, which is
    /// the original defect wearing a different hat. No call site does this
    /// today; the point is that none can, including by holding a permit
    /// across an `.await` if this ever meets async code.
    _not_send: PhantomData<*const ()>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if !self.holds_slot {
            return;
        }
        {
            let mut limiter = RATE_LIMITER.lock();
            if let Some(state) = limiter.services.get_mut(&self.service) {
                state.in_flight = state.in_flight.saturating_sub(1);
            }
        }
        HELD_BY_THIS_THREAD.with(|held| {
            held.borrow_mut().remove(&self.service);
        });
        SLOT_RELEASED.notify_all();
    }
}

/// Take a permit for one request against a service, waiting as long as the
/// service's limits require.
///
/// This blocks the calling thread until the service has both a free slot and
/// enough time elapsed since the last request start. The returned permit holds
/// the slot until it drops, so **bind it to a named variable that lives for the
/// whole request** -- see [`Permit`] for what happens if you do not.
///
/// A service that has not been registered gets the defaults: one request per
/// second, one at a time.
///
/// The permit is `!Send` on purpose. Moving one to another thread and dropping
/// it there would leak the taking thread's re-entrancy marker and quietly
/// disable its concurrency bound, so the compiler refuses:
///
/// ```compile_fail
/// fn assert_send<T: Send>(_: T) {}
/// let permit = acr_http::ratelimit::rate_limit("doctest.not.send");
/// assert_send(permit);
/// ```
///
/// # Arguments
/// * `service_name` - Name of the service to rate limit
pub fn rate_limit(service_name: &str) -> Permit {
    // A thread that already holds a permit for this service must not wait for
    // a slot it is itself occupying. Such a call site should scope its permits
    // instead, so say so, but keep the daemon running: spacing still applies,
    // and only the concurrency bound is briefly exceeded.
    let nested = HELD_BY_THIS_THREAD.with(|held| held.borrow().contains(service_name));
    if nested {
        warn!(
            "Nested rate-limit permit for service '{}'; the outer permit should be scoped to its own request",
            service_name
        );
    }

    loop {
        let mut limiter = RATE_LIMITER.lock();
        let state = limiter.entry(service_name);

        if !nested && state.in_flight >= state.max_concurrent {
            debug!(
                "Service '{}' has {} request(s) in flight, waiting for a free slot",
                service_name, state.in_flight
            );
            SLOT_RELEASED.wait_for(&mut limiter, SLOT_POLL_INTERVAL);
            continue;
        }

        let now = Instant::now();
        let earliest = state.last_start + Duration::from_millis(state.minimum_delay_ms);
        if now < earliest {
            let remaining = earliest - now;
            debug!(
                "Rate limiting service '{}': waiting {} ms",
                service_name,
                remaining.as_millis()
            );
            SLOT_RELEASED.wait_for(&mut limiter, remaining);
            continue;
        }

        state.last_start = now;
        if !nested {
            state.in_flight += 1;
        }
        break;
    }

    if !nested {
        HELD_BY_THIS_THREAD.with(|held| {
            held.borrow_mut().insert(service_name.to_string());
        });
    }

    Permit {
        service: service_name.to_string(),
        holds_slot: !nested,
        _not_send: PhantomData,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;

    /// Records how many threads were inside the "request" at the same time.
    #[derive(Default)]
    struct ConcurrencyWitness {
        current: AtomicUsize,
        max: AtomicUsize,
    }

    impl ConcurrencyWitness {
        fn enter(&self) {
            let now = self.current.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            self.max.fetch_max(now, AtomicOrdering::SeqCst);
        }
        fn leave(&self) {
            self.current.fetch_sub(1, AtomicOrdering::SeqCst);
        }
        fn peak(&self) -> usize {
            self.max.load(AtomicOrdering::SeqCst)
        }
    }

    /// Run `threads` simulated requests against `service`, each holding its
    /// permit for `hold_ms`, and report the peak simultaneous holders.
    fn peak_concurrency(service: &'static str, threads: usize, hold_ms: u64) -> usize {
        let witness = Arc::new(ConcurrencyWitness::default());
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let witness = Arc::clone(&witness);
                thread::spawn(move || {
                    let _permit = rate_limit(service);
                    witness.enter();
                    thread::sleep(Duration::from_millis(hold_ms));
                    witness.leave();
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("worker thread panicked");
        }
        witness.peak()
    }

    /// The defect this module exists to prevent: the limiter spaced request
    /// starts but bounded nothing, so a slow provider accumulated a dozen
    /// concurrent requests and answered the surplus with 503.
    #[test]
    fn a_service_never_has_more_than_one_request_in_flight() {
        register_service_with_concurrency("test.concurrency.one", 1, 1);

        // Spacing is 1 ms and each request takes 100 ms, so without a permit
        // held across the request all six threads overlap.
        let peak = peak_concurrency("test.concurrency.one", 6, 100);

        assert_eq!(
            peak, 1,
            "expected one request in flight at a time, saw {peak} at once"
        );
    }

    /// Control for the test above: the witness really does observe overlap
    /// when the configured bound allows it, so `peak == 1` there is the
    /// limiter working and not the harness failing to notice.
    #[test]
    fn a_service_configured_for_two_reaches_two_and_stops_there() {
        register_service_with_concurrency("test.concurrency.two", 1, 2);

        let peak = peak_concurrency("test.concurrency.two", 6, 100);

        assert_eq!(
            peak, 2,
            "expected exactly two requests in flight at a time, saw {peak}"
        );
    }

    /// Spacing is the property the limiter already had; bounding concurrency
    /// must not cost it.
    #[test]
    fn request_starts_stay_at_least_the_minimum_delay_apart() {
        register_service_with_concurrency("test.spacing", 60, 1);

        let mut starts = Vec::new();
        for _ in 0..4 {
            let _permit = rate_limit("test.spacing");
            starts.push(Instant::now());
        }

        for pair in starts.windows(2) {
            let gap = pair[1].duration_since(pair[0]);
            assert!(
                gap >= Duration::from_millis(55),
                "request starts were only {gap:?} apart, expected at least 60ms"
            );
        }
    }

    /// The old implementation slept while holding the one global mutex, so a
    /// slow MusicBrainz stalled Last.fm as well. Waiting must be per service.
    #[test]
    fn a_saturated_service_does_not_delay_a_different_service() {
        register_service_with_concurrency("test.independent.slow", 1, 1);
        register_service_with_concurrency("test.independent.fast", 1, 1);

        // One thread occupies the slow service, a second queues behind it.
        let blocker = thread::spawn(|| {
            let _permit = rate_limit("test.independent.slow");
            thread::sleep(Duration::from_millis(600));
        });
        thread::sleep(Duration::from_millis(50));
        let waiter = thread::spawn(|| {
            let _permit = rate_limit("test.independent.slow");
        });
        thread::sleep(Duration::from_millis(50));

        let started = Instant::now();
        let permit = rate_limit("test.independent.fast");
        let waited = started.elapsed();
        drop(permit);

        assert!(
            waited < Duration::from_millis(200),
            "an unrelated service waited {waited:?} behind a saturated one"
        );

        blocker.join().expect("blocker panicked");
        waiter.join().expect("waiter panicked");
    }

    /// `search_release_group_genres` takes a permit, then takes a second one
    /// for the follow-up request. With a bound of one and no re-entrancy
    /// escape that thread would wait on itself forever.
    #[test]
    fn a_nested_permit_on_the_same_thread_still_spaces_instead_of_deadlocking() {
        register_service_with_concurrency("test.reentrant", 60, 1);

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let started = Instant::now();
            let _outer = rate_limit("test.reentrant");
            let _inner = rate_limit("test.reentrant");
            let _ = tx.send(started.elapsed());
        });

        let elapsed = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a nested permit on the same thread deadlocked");
        assert!(
            elapsed >= Duration::from_millis(55),
            "the nested request was not spaced from the outer one: {elapsed:?}"
        );
    }
}
