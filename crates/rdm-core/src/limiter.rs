//! Global token-bucket speed limiter shared by every connection.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub(crate) struct Limiter {
    /// Bytes per second, `0` = unlimited.
    rate: AtomicU64,
    bucket: Mutex<Bucket>,
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Limiter {
    pub fn new(rate: u64) -> Self {
        Self {
            rate: AtomicU64::new(rate),
            bucket: Mutex::new(Bucket {
                tokens: 0.0,
                last: Instant::now(),
            }),
        }
    }

    /// Bytes per second, `0` = unlimited.
    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    pub fn set_rate(&self, rate: u64) {
        self.rate.store(rate, Ordering::Relaxed);
    }

    /// Waits until `bytes` may be consumed.
    pub async fn acquire(&self, bytes: usize) {
        let rate = self.rate.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }
        let wait = {
            let mut b = self.bucket.lock().unwrap();
            let now = Instant::now();
            let elapsed = now.duration_since(b.last).as_secs_f64();
            b.last = now;
            // Allow at most a quarter second of burst.
            b.tokens = (b.tokens + elapsed * rate as f64).min(rate as f64 / 4.0);
            b.tokens -= bytes as f64;
            if b.tokens < 0.0 {
                -b.tokens / rate as f64
            } else {
                0.0
            }
        };
        if wait > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(wait)).await;
        }
    }
}
