use super::*;
use asset_plugin_api::protocol::PluginResourceActionRequest;
use extism_pdk::FnResult;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

static BOOK_CACHE: OnceLock<Mutex<VecDeque<Arc<CachedBook>>>> = OnceLock::new();
static COVER_CACHE: OnceLock<Mutex<VecDeque<CachedCover>>> = OnceLock::new();

pub(super) fn resource_cache_key(request: &PluginResourceActionRequest) -> String {
    let mut key = format!(
        "{}:{}:{}",
        request.resource.id,
        request.resource.updated_at,
        request
            .resource
            .content
            .as_ref()
            .map_or(0, |value| value.size)
    );
    if let Some(checksum) = request
        .resource
        .content
        .as_ref()
        .and_then(|content| content.checksum.as_ref())
    {
        key.push(':');
        key.push_str(&checksum.kind);
        key.push(':');
        key.push_str(&checksum.value);
    }
    key
}

pub(super) fn cached_book(key: &str) -> Option<Arc<CachedBook>> {
    let cache = BOOK_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    let mut cache = cache.lock().ok()?;
    let position = cache.iter().position(|entry| entry.key == key)?;
    let book = cache.remove(position)?;
    cache.push_front(book.clone());
    Some(book)
}

pub(super) fn load_cached_book(request: &PluginResourceActionRequest) -> FnResult<Arc<CachedBook>> {
    let key = resource_cache_key(request);
    if let Some(book) = cached_book(&key) {
        return Ok(book);
    }

    let bytes = Arc::new(epub_content_bytes(request)?);
    let book = Arc::new(parse_book(key, bytes.clone())?);
    if bytes.len() <= MAX_CACHED_BOOK_BYTES {
        let cache = BOOK_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
        if let Ok(mut cache) = cache.lock() {
            cache.retain(|entry| entry.key != book.key);
            cache.push_front(book.clone());
            while cache.len() > BOOK_CACHE_CAPACITY
                || cache.iter().map(|entry| entry.bytes.len()).sum::<usize>() > MAX_BOOK_CACHE_BYTES
            {
                cache.pop_back();
            }
        }
    }
    store_cover(book.key.clone(), book.index.cover.clone());
    Ok(book)
}

pub(super) fn cached_cover(key: &str) -> Option<Option<String>> {
    let cache = COVER_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    let mut cache = cache.lock().ok()?;
    let position = cache.iter().position(|entry| entry.key == key)?;
    let entry = cache.remove(position)?;
    let cover = entry.cover.clone();
    cache.push_front(entry);
    Some(cover)
}

pub(super) fn store_cover(key: String, cover: Option<String>) {
    let cache = COVER_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.retain(|entry| entry.key != key);
        cache.push_front(CachedCover { key, cover });
        while cache.len() > COVER_CACHE_CAPACITY
            || cache
                .iter()
                .map(|entry| entry.cover.as_ref().map_or(0, String::len))
                .sum::<usize>()
                > MAX_COVER_CACHE_BYTES
        {
            cache.pop_back();
        }
    }
}
