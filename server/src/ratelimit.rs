//! A small in-memory rate limiter for the credential-guessing endpoints
//! (login, setup). Keyed by what's being guessed (a username, or the fixed
//! string `"setup"`) rather than by source address: on a single-machine
//! tool the address is almost always 127.0.0.1 anyway, and what actually
//! matters is slowing repeated guesses against the same target.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    attempts: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        RateLimiter { attempts: Mutex::new(HashMap::new()) }
    }
}

impl RateLimiter {
    /// Records an attempt against `key`. `Err(seconds)` means the caller
    /// should wait that long before trying again; the attempt is still
    /// recorded so hammering the endpoint doesn't reset the window.
    pub fn check(&self, key: &str, max: usize, window: Duration) -> Result<(), u64> {
        let now = Instant::now();
        let mut attempts = self.attempts.lock().expect("rate limiter lock");
        let entry = attempts.entry(key.to_string()).or_default();
        while entry.front().is_some_and(|t| now.duration_since(*t) > window) {
            entry.pop_front();
        }
        entry.push_back(now);
        if entry.len() > max {
            let retry = window.saturating_sub(now.duration_since(entry[0]));
            return Err(retry.as_secs() + 1);
        }
        // Bound memory: an attacker cycling through usernames shouldn't be
        // able to grow this map without limit. Old, quiet keys age out.
        if attempts.len() > 10_000 {
            attempts.retain(|_, v| v.back().is_some_and(|t| now.duration_since(*t) < window));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_then_blocks() {
        let rl = RateLimiter::default();
        for _ in 0..5 {
            assert!(rl.check("bob", 5, Duration::from_secs(60)).is_ok());
        }
        assert!(rl.check("bob", 5, Duration::from_secs(60)).is_err());
        // A different key is unaffected.
        assert!(rl.check("alice", 5, Duration::from_secs(60)).is_ok());
    }

    #[test]
    fn old_attempts_age_out() {
        let rl = RateLimiter::default();
        for _ in 0..5 {
            assert!(rl.check("bob", 5, Duration::from_millis(20)).is_ok());
        }
        assert!(rl.check("bob", 5, Duration::from_millis(20)).is_err());
        std::thread::sleep(Duration::from_millis(40));
        assert!(rl.check("bob", 5, Duration::from_millis(20)).is_ok());
    }
}
