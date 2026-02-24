use crate::spotify_api::SpotifyCover;
use moka::future::Cache;
use std::sync::Arc;

#[derive(Clone)]
pub struct CachedCoverProvider<T>
where
    T: SpotifyCover + Send + Sync + 'static,
{
    inner: Arc<T>,
    cache: Arc<Cache<String, Vec<u8>>>,
}

impl<T> CachedCoverProvider<T>
where
    T: SpotifyCover + Send + Sync + 'static,
{
    pub fn new(inner: Arc<T>) -> Self {
        Self {
            cache: Arc::new(
                Cache::builder()
                    .max_capacity(1024)
                    .time_to_idle(std::time::Duration::from_secs(3600))
                    .build(),
            ),
            inner,
        }
    }
}

#[async_trait::async_trait]
impl<T> SpotifyCover for CachedCoverProvider<T>
where
    T: SpotifyCover + Send + Sync + 'static,
{
    async fn fetch_cover(&self, cover_id: &str) -> anyhow::Result<Vec<u8>> {
        let inner = self.inner.clone();
        let key = cover_id.to_string();
        self.cache
            .try_get_with(key.clone(), async move { inner.fetch_cover(&key).await })
            .await
            .map_err(|e| anyhow::Error::msg(format!("Failed to fetch cover {}: {}", cover_id, e)))
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify_api::SpotifyCover;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use anyhow::{Result, anyhow};

    #[derive(Clone)]
    struct MockFetcher {
        call_count: Arc<AtomicUsize>,
        fail_on: Option<String>,
    }

    #[async_trait::async_trait]
    impl SpotifyCover for MockFetcher {
        async fn fetch_cover(&self, cover_id: &str) -> Result<Vec<u8>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if let Some(ref fail_id) = self.fail_on {
                if fail_id == cover_id {
                    return Err(anyhow!("forced error for {cover_id}"));
                }
            }
            Ok(vec![cover_id.len() as u8])
        }
    }

    #[tokio::test]
    async fn test_cache_hit_and_miss() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let fetcher = Arc::new(MockFetcher { call_count: call_count.clone(), fail_on: None });
        let provider = CachedCoverProvider::new(fetcher);
        let cover_id = "abc";

        // First call: should miss cache and call inner
        let result1 = provider.fetch_cover(cover_id).await.unwrap();
        assert_eq!(result1, vec![3]);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Second call: should hit cache, not call inner
        let result2 = provider.fetch_cover(cover_id).await.unwrap();
        assert_eq!(result2, vec![3]);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_cache_error_propagation() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let fail_id = "fail".to_string();
        let fetcher = Arc::new(MockFetcher { call_count: call_count.clone(), fail_on: Some(fail_id.clone()) });
        let provider = CachedCoverProvider::new(fetcher);

        // First call: should error, not cache
        let result1 = provider.fetch_cover(&fail_id).await;
        assert!(result1.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Second call: should try again (not cached), error again
        let result2 = provider.fetch_cover(&fail_id).await;
        assert!(result2.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_cache_success_after_error() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let fail_id = "flip".to_string();
        // This fetcher will fail the first time, then succeed
        #[derive(Clone)]
        struct FlipFetcher {
            call_count: Arc<AtomicUsize>,
            fail_once: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl SpotifyCover for FlipFetcher {
            async fn fetch_cover(&self, cover_id: &str) -> Result<Vec<u8>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                if self.fail_once.fetch_sub(1, Ordering::SeqCst) > 0 {
                    return Err(anyhow!("fail once for {cover_id}"));
                }
                Ok(vec![cover_id.len() as u8])
            }
        }
        let fail_once = Arc::new(AtomicUsize::new(1));
        let fetcher = Arc::new(FlipFetcher { call_count: call_count.clone(), fail_once: fail_once.clone() });
        let provider = CachedCoverProvider::new(fetcher);

        // First call: should error
        let result1 = provider.fetch_cover(&fail_id).await;
        assert!(result1.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Second call: should succeed and cache
        let result2 = provider.fetch_cover(&fail_id).await.unwrap();
        assert_eq!(result2, vec![fail_id.len() as u8]);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);

        // Third call: should hit cache, not call inner
        let result3 = provider.fetch_cover(&fail_id).await.unwrap();
        assert_eq!(result3, vec![fail_id.len() as u8]);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}