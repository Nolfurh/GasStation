// Rate limiting module for DoS protection

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct RateLimitEntry {
    count: u32,
    window_start: Instant,
}

/// Глобальний rate limiter
static RATE_LIMITER: Lazy<Mutex<HashMap<String, RateLimitEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));


// true якщо запит дозволено, false якщо перевищено ліміт
pub fn check_rate_limit(ip: &str, max_requests: u32, window: Duration) -> bool {
    let mut limiter = RATE_LIMITER.lock().unwrap();
    let now = Instant::now();

    match limiter.get_mut(ip) {
        Some(entry) => {
            if now.duration_since(entry.window_start) > window {
                entry.count = 1;
                entry.window_start = now;
                true
            } else if entry.count < max_requests {
                entry.count += 1;
                true
            } else {
                false
            }
        }
        None => {
            limiter.insert(
                ip.to_string(),
                RateLimitEntry {
                    count: 1,
                    window_start: now,
                },
            );
            true
        }
    }
}

// Очищення старих записів (викликати періодично)
pub fn cleanup_old_entries(max_age: Duration) {
    let mut limiter = RATE_LIMITER.lock().unwrap();
    let now = Instant::now();

    limiter.retain(|_, entry| now.duration_since(entry.window_start) < max_age);
}
