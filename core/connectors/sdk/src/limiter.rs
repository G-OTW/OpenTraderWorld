//! The hook every outbound request passes through. The engine supplies the real token
//! buckets; the SDK only knows the interface.

#[async_trait::async_trait]
pub trait Limiter: Send + Sync {
    /// Wait until `weight` tokens are available for `endpoint_id`.
    async fn acquire(&self, endpoint_id: &str, weight: u32);
    /// The venue asked us to back off (429 + Retry-After).
    fn penalize(&self, _retry_after_ms: u64) {}
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoLimit;

#[async_trait::async_trait]
impl Limiter for NoLimit {
    async fn acquire(&self, _endpoint_id: &str, _weight: u32) {}
}
