//! Token buckets built from the declared rate limits. One limiter per venue host is shared by
//! every connector on it, since most venues count by IP, not by key.
//!
//! Time is tokio's, so tests run it paused and step it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use otw_connector_model::{RateLimit, RateScope};
use otw_connector_sdk::Limiter;
use tokio::time::Instant;

#[derive(Debug)]
struct Bucket {
    capacity: Millitokens,
    tokens: Millitokens,
    refill: Duration,
    last: Instant,
}

/// Token counts in thousandths, so the arithmetic stays integral.
type Millitokens = u64;
const MILLI: u64 = 1000;

impl Bucket {
    fn new(capacity: u32, refill_ms: u64, now: Instant) -> Self {
        let c = u64::from(capacity) * MILLI;
        Bucket { capacity: c, tokens: c, refill: Duration::from_millis(refill_ms.max(1)), last: now }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last);
        let refill_ns = self.refill.as_nanos().max(1);
        let add = (u128::from(self.capacity) * elapsed.as_nanos() / refill_ns).min(u128::from(u64::MAX));
        let add = u64::try_from(add).unwrap_or(u64::MAX);
        if add > 0 {
            self.tokens = (self.tokens.saturating_add(add)).min(self.capacity);
            self.last = now;
        }
    }

    /// Time until `need` tokens are there; zero if they already are.
    fn wait_for(&self, need: u64) -> Duration {
        if self.tokens >= need {
            return Duration::ZERO;
        }
        let missing = u128::from(need.min(self.capacity) - self.tokens.min(need));
        let ns = missing * self.refill.as_nanos() / u128::from(self.capacity.max(1));
        Duration::from_nanos(u64::try_from(ns).unwrap_or(u64::MAX)) + Duration::from_millis(1)
    }
}

pub struct TokenBucketLimiter {
    shared: Vec<Mutex<Bucket>>,
    per_endpoint: HashMap<String, Mutex<Bucket>>,
    blocked_until: Mutex<Option<Instant>>,
}

impl TokenBucketLimiter {
    pub fn new(limits: &[RateLimit]) -> Self {
        let now = Instant::now();
        let mut shared = Vec::new();
        let mut per_endpoint = HashMap::new();
        for l in limits {
            let b = Bucket::new(l.capacity, l.refill_ms, now);
            match (l.scope, &l.endpoint) {
                (RateScope::Endpoint, Some(e)) => {
                    per_endpoint.insert(e.clone(), Mutex::new(b));
                }
                _ => shared.push(Mutex::new(b)),
            }
        }
        TokenBucketLimiter { shared, per_endpoint, blocked_until: Mutex::new(None) }
    }

    /// Take `weight` from every bucket that applies, or say how long to wait.
    fn try_take(&self, endpoint_id: &str, weight: u32) -> Duration {
        let now = Instant::now();
        if let Ok(b) = self.blocked_until.lock() {
            if let Some(until) = *b {
                if until > now {
                    return until - now;
                }
            }
        }
        let need = u64::from(weight) * MILLI;
        let mut buckets: Vec<&Mutex<Bucket>> = self.shared.iter().collect();
        if let Some(b) = self.per_endpoint.get(endpoint_id) {
            buckets.push(b);
        }
        let mut guards = Vec::with_capacity(buckets.len());
        for b in buckets {
            match b.lock() {
                Ok(g) => guards.push(g),
                Err(_) => return Duration::ZERO,
            }
        }
        let mut wait = Duration::ZERO;
        for g in guards.iter_mut() {
            g.refill(now);
            wait = wait.max(g.wait_for(need));
        }
        if wait.is_zero() {
            for g in guards.iter_mut() {
                g.tokens = g.tokens.saturating_sub(need);
            }
        }
        wait
    }
}

#[async_trait::async_trait]
impl Limiter for TokenBucketLimiter {
    async fn acquire(&self, endpoint_id: &str, weight: u32) {
        loop {
            let wait = self.try_take(endpoint_id, weight);
            if wait.is_zero() {
                return;
            }
            tokio::time::sleep(wait).await;
        }
    }

    fn penalize(&self, retry_after_ms: u64) {
        let until = Instant::now() + Duration::from_millis(retry_after_ms.min(3_600_000));
        if let Ok(mut b) = self.blocked_until.lock() {
            if b.is_none_or(|x| x < until) {
                *b = Some(until);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn lim(capacity: u32, refill_ms: u64) -> TokenBucketLimiter {
        TokenBucketLimiter::new(&[RateLimit { scope: RateScope::Ip, endpoint: None, capacity, refill_ms }])
    }

    #[tokio::test(start_paused = true)]
    async fn bursts_to_capacity_then_paces() {
        let l = lim(10, 1000);
        let t0 = Instant::now();
        for _ in 0..10 {
            l.acquire("x", 1).await;
        }
        assert!(t0.elapsed() < Duration::from_millis(5), "a full bucket bursts");
        l.acquire("x", 5).await;
        let waited = t0.elapsed();
        assert!(waited >= Duration::from_millis(500) && waited < Duration::from_millis(600), "{waited:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn retry_after_blocks_everyone() {
        let l = lim(100, 1000);
        l.penalize(3000);
        let t0 = Instant::now();
        l.acquire("x", 1).await;
        assert!(t0.elapsed() >= Duration::from_millis(3000));
    }

    #[tokio::test(start_paused = true)]
    async fn endpoint_bucket_and_shared_bucket_both_apply() {
        let l = TokenBucketLimiter::new(&[
            RateLimit { scope: RateScope::Ip, endpoint: None, capacity: 100, refill_ms: 1000 },
            RateLimit { scope: RateScope::Endpoint, endpoint: Some("orders".into()), capacity: 1, refill_ms: 1000 },
        ]);
        let t0 = Instant::now();
        l.acquire("orders", 1).await;
        l.acquire("klines", 1).await;
        assert!(t0.elapsed() < Duration::from_millis(5));
        l.acquire("orders", 1).await;
        assert!(t0.elapsed() >= Duration::from_millis(990));
    }
}
