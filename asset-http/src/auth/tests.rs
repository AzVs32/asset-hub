use super::*;

#[test]
fn login_failure_cache_is_bounded_and_evicts_oldest_entries() {
    let mut cache = LoginFailureCache::default();
    for index in 0..(MAX_LOGIN_FAILURE_ENTRIES + 100) {
        cache.record(login_failure_key(&format!("user-{index}")), false);
    }

    assert_eq!(cache.entries.len(), MAX_LOGIN_FAILURE_ENTRIES);
    assert_eq!(cache.order.len(), MAX_LOGIN_FAILURE_ENTRIES);
    assert!(!cache.entries.contains_key(&login_failure_key("user-0")));
    assert!(cache.entries.contains_key(&login_failure_key("user-10099")));
}

#[test]
fn successful_login_removes_failure_state() {
    let mut cache = LoginFailureCache::default();
    let key = login_failure_key("alice");
    cache.record(key, false);
    cache.record(key, true);

    assert!(cache.check_allowed(&key));
    assert!(!cache.entries.contains_key(&key));
}
