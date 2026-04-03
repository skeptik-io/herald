use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker that trips after consecutive failures and recovers after a cooldown.
///
/// - **Closed**: all calls pass through. Failures increment counter.
/// - **Open**: all calls rejected immediately. After cooldown, transitions to HalfOpen.
/// - **HalfOpen**: one call allowed through. Success → Closed, failure → Open.
pub struct CircuitBreaker {
    name: String,
    inner: Mutex<Inner>,
    failure_threshold: u32,
    cooldown: Duration,
    probe_in_progress: AtomicBool,
}

struct Inner {
    state: State,
    failures: u32,
    last_failure: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(name: &str, failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            name: name.to_string(),
            inner: Mutex::new(Inner {
                state: State::Closed,
                failures: 0,
                last_failure: None,
            }),
            failure_threshold,
            cooldown,
            probe_in_progress: AtomicBool::new(false),
        }
    }

    /// Check if a call is allowed. Returns Ok(()) if allowed, Err with reason if rejected.
    pub fn check(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        match inner.state {
            State::Closed => Ok(()),
            State::Open => {
                // Check if cooldown has elapsed
                if let Some(last) = inner.last_failure {
                    if last.elapsed() >= self.cooldown {
                        inner.state = State::HalfOpen;
                        self.probe_in_progress.store(true, AtomicOrdering::SeqCst);
                        tracing::info!(engine = %self.name, "circuit breaker: open → half-open");
                        Ok(())
                    } else {
                        Err(format!(
                            "{} circuit open ({}s remaining)",
                            self.name,
                            (self.cooldown - last.elapsed()).as_secs()
                        ))
                    }
                } else {
                    // No last failure recorded — shouldn't happen, but allow
                    inner.state = State::HalfOpen;
                    self.probe_in_progress.store(true, AtomicOrdering::SeqCst);
                    Ok(())
                }
            }
            State::HalfOpen => {
                // Only one probe call allowed in half-open
                if self
                    .probe_in_progress
                    .compare_exchange(false, true, AtomicOrdering::SeqCst, AtomicOrdering::SeqCst)
                    .is_ok()
                {
                    Ok(())
                } else {
                    Err(format!(
                        "{} circuit half-open (probe in progress)",
                        self.name
                    ))
                }
            }
        }
    }

    /// Record a successful call.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.state == State::HalfOpen {
            tracing::info!(engine = %self.name, "circuit breaker: half-open → closed");
            self.probe_in_progress.store(false, AtomicOrdering::SeqCst);
        }
        inner.state = State::Closed;
        inner.failures = 0;
    }

    /// Record a failed call.
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.failures += 1;
        inner.last_failure = Some(Instant::now());

        if inner.state == State::HalfOpen {
            inner.state = State::Open;
            self.probe_in_progress.store(false, AtomicOrdering::SeqCst);
            tracing::warn!(engine = %self.name, "circuit breaker: half-open → open (probe failed)");
        } else if inner.failures >= self.failure_threshold {
            if inner.state != State::Open {
                tracing::warn!(
                    engine = %self.name,
                    failures = inner.failures,
                    "circuit breaker: closed → open"
                );
            }
            inner.state = State::Open;
        }
    }

    pub fn state(&self) -> State {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_lifecycle() {
        let cb = CircuitBreaker::new("test", 3, Duration::from_millis(100));

        // Closed initially
        assert_eq!(cb.state(), State::Closed);
        assert!(cb.check().is_ok());

        // 2 failures — still closed
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), State::Closed);
        assert!(cb.check().is_ok());

        // 3rd failure — trips to open
        cb.record_failure();
        assert_eq!(cb.state(), State::Open);
        assert!(cb.check().is_err());

        // Wait for cooldown
        std::thread::sleep(Duration::from_millis(150));

        // Should transition to half-open
        assert!(cb.check().is_ok());
        assert_eq!(cb.state(), State::HalfOpen);

        // Success in half-open → closed
        cb.record_success();
        assert_eq!(cb.state(), State::Closed);
    }

    #[test]
    fn test_circuit_breaker_halfopen_failure() {
        let cb = CircuitBreaker::new("test", 2, Duration::from_millis(50));

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), State::Open);

        std::thread::sleep(Duration::from_millis(60));
        assert!(cb.check().is_ok()); // half-open

        // Failure in half-open → back to open
        cb.record_failure();
        assert_eq!(cb.state(), State::Open);
    }

    #[test]
    fn test_halfopen_allows_only_one_probe() {
        let cb = CircuitBreaker::new("test", 2, Duration::from_millis(50));

        // Trip to open
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), State::Open);

        // Wait for cooldown
        std::thread::sleep(Duration::from_millis(60));

        // First check transitions to half-open and allows
        assert!(cb.check().is_ok());
        assert_eq!(cb.state(), State::HalfOpen);

        // Second check while probe is in progress should be rejected
        assert!(cb.check().is_err());

        // Third check also rejected
        assert!(cb.check().is_err());

        // Probe succeeds — circuit closes, probe flag reset
        cb.record_success();
        assert_eq!(cb.state(), State::Closed);

        // Now all calls should pass again
        assert!(cb.check().is_ok());
        assert!(cb.check().is_ok());
    }

    #[test]
    fn test_success_resets_counter() {
        let cb = CircuitBreaker::new("test", 3, Duration::from_secs(30));

        cb.record_failure();
        cb.record_failure();
        cb.record_success(); // Resets
        cb.record_failure();
        cb.record_failure();

        // Only 2 failures since last success — still closed
        assert_eq!(cb.state(), State::Closed);
    }
}
