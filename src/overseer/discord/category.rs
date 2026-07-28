//! Resolves whether a Discord channel sits under one of the operator's
//! configured chat categories. A `MESSAGE_CREATE` payload carries no
//! `parent_id`, so membership means an HTTP `GET /channels/:id` — cached,
//! since a busy category would otherwise fetch on every message.

use std::{
    collections::HashMap,
    future::Future,
    time::{Duration, Instant},
};

const DEFAULT_TTL: Duration = Duration::from_secs(300);
/// Bounds cache growth in a guild with many distinct channels. Eviction is
/// oldest-first, not LRU — simple, and fine for a cache whose whole point is
/// "don't re-fetch within the last few minutes," not "keep the hottest set."
const DEFAULT_CAP: usize = 500;

struct CacheEntry {
    parent_id: Option<String>,
    stored_at: Instant,
}

pub(super) struct CategoryCache {
    entries: HashMap<String, CacheEntry>,
    ttl: Duration,
    cap: usize,
}

impl Default for CategoryCache {
    fn default() -> Self {
        Self::with_bounds(DEFAULT_TTL, DEFAULT_CAP)
    }
}

enum Lookup {
    Fresh(Option<String>),
    Stale,
}

impl CategoryCache {
    fn with_bounds(ttl: Duration, cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
            cap,
        }
    }

    fn get(&self, channel_id: &str, now: Instant) -> Lookup {
        match self.entries.get(channel_id) {
            Some(entry) if now.saturating_duration_since(entry.stored_at) < self.ttl => {
                Lookup::Fresh(entry.parent_id.clone())
            }
            _ => Lookup::Stale,
        }
    }

    fn store(&mut self, channel_id: &str, parent_id: Option<String>, now: Instant) {
        if !self.entries.contains_key(channel_id) && self.entries.len() >= self.cap {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.stored_at)
                .map(|(channel_id, _)| channel_id.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(
            channel_id.to_string(),
            CacheEntry {
                parent_id,
                stored_at: now,
            },
        );
    }
}

/// Whether `channel_id` sits under one of `category_ids`. Empty
/// `category_ids` short-circuits before touching the cache or `lookup` at
/// all — the feature is off, and behavior must be byte-identical to before
/// it existed. On a cache miss or expiry, `lookup` stands in for the live
/// Discord channel fetch; tests pass a canned double instead.
pub(super) async fn is_in_category<F, Fut>(
    cache: &mut CategoryCache,
    lookup: F,
    channel_id: &str,
    category_ids: &[String],
    now: Instant,
) -> bool
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Option<String>, String>>,
{
    if category_ids.is_empty() {
        return false;
    }
    let parent_id = match cache.get(channel_id, now) {
        Lookup::Fresh(parent_id) => parent_id,
        Lookup::Stale => {
            let parent_id = lookup(channel_id.to_string()).await.ok().flatten();
            cache.store(channel_id, parent_id.clone(), now);
            parent_id
        }
    };
    parent_id.is_some_and(|parent_id| category_ids.contains(&parent_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    async fn fails(_channel_id: String) -> Result<Option<String>, String> {
        Err("should not be called".into())
    }

    #[tokio::test]
    async fn an_empty_allow_list_never_calls_lookup_or_touches_the_cache() {
        let mut cache = CategoryCache::with_bounds(Duration::from_secs(60), 10);
        let member = is_in_category(&mut cache, fails, "1", &[], Instant::now()).await;
        assert!(!member);
        assert!(cache.entries.is_empty());
    }

    #[tokio::test]
    async fn a_cache_miss_calls_lookup_and_stores_the_result() {
        let mut cache = CategoryCache::with_bounds(Duration::from_secs(60), 10);
        let calls = Cell::new(0);
        let member = is_in_category(
            &mut cache,
            |_| {
                calls.set(calls.get() + 1);
                async { Ok(Some("cat-1".to_string())) }
            },
            "channel-1",
            &["cat-1".into()],
            Instant::now(),
        )
        .await;
        assert!(member);
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn a_fresh_cache_hit_never_calls_lookup() {
        let mut cache = CategoryCache::with_bounds(Duration::from_secs(60), 10);
        let now = Instant::now();
        cache.store("channel-1", Some("cat-1".into()), now);
        let member = is_in_category(
            &mut cache,
            fails,
            "channel-1",
            &["cat-1".into()],
            now + Duration::from_secs(1),
        )
        .await;
        assert!(member);
    }

    #[tokio::test]
    async fn an_expired_entry_is_treated_as_a_miss_and_refetched() {
        let mut cache = CategoryCache::with_bounds(Duration::from_secs(60), 10);
        let now = Instant::now();
        cache.store("channel-1", Some("cat-old".into()), now);
        let calls = Cell::new(0);
        let member = is_in_category(
            &mut cache,
            |_| {
                calls.set(calls.get() + 1);
                async { Ok(Some("cat-new".to_string())) }
            },
            "channel-1",
            &["cat-new".into()],
            now + Duration::from_secs(61),
        )
        .await;
        assert!(member);
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn a_channel_outside_every_configured_category_is_not_a_member() {
        let mut cache = CategoryCache::with_bounds(Duration::from_secs(60), 10);
        let member = is_in_category(
            &mut cache,
            |_| async { Ok(Some("cat-other".to_string())) },
            "channel-1",
            &["cat-1".into()],
            Instant::now(),
        )
        .await;
        assert!(!member);
    }

    #[tokio::test]
    async fn a_failed_lookup_is_treated_as_not_a_member_and_still_cached() {
        let mut cache = CategoryCache::with_bounds(Duration::from_secs(60), 10);
        let member = is_in_category(
            &mut cache,
            |_| async { Err("http error".to_string()) },
            "channel-1",
            &["cat-1".into()],
            Instant::now(),
        )
        .await;
        assert!(!member);
        assert!(matches!(
            cache.get("channel-1", Instant::now()),
            Lookup::Fresh(None)
        ));
    }

    #[test]
    fn the_cache_evicts_the_oldest_entry_once_at_capacity() {
        let mut cache = CategoryCache::with_bounds(Duration::from_secs(60), 2);
        let base = Instant::now();
        cache.store("a", Some("cat".into()), base);
        cache.store("b", Some("cat".into()), base + Duration::from_secs(1));
        cache.store("c", Some("cat".into()), base + Duration::from_secs(2));
        assert!(matches!(cache.get("a", base), Lookup::Stale));
        assert!(matches!(cache.get("b", base), Lookup::Fresh(_)));
        assert!(matches!(cache.get("c", base), Lookup::Fresh(_)));
    }
}
