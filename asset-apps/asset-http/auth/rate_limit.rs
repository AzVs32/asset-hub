use super::*;

const MAX_LOGIN_FAILURES: u8 = 5;
const LOGIN_FAILURE_WINDOW: Duration = Duration::from_secs(60);
pub(super) const MAX_LOGIN_FAILURE_ENTRIES: usize = 10_000;
pub(crate) const MAX_LOGIN_REQUEST_BYTES: usize = 16 * 1024;
pub(super) type LoginFailureKey = [u8; 32];

#[derive(Debug)]
pub(super) struct LoginFailureState {
    failures: u8,
    started_at: Instant,
}

#[derive(Debug, Default)]
pub(super) struct LoginFailureCache {
    pub(super) entries: HashMap<LoginFailureKey, LoginFailureState>,
    pub(super) order: VecDeque<(LoginFailureKey, Instant)>,
}

impl LoginFailureCache {
    pub(super) fn prune_expired(&mut self, now: Instant) {
        while let Some((key, started_at)) = self.order.front() {
            let current = self.entries.get(key);
            let stale = current.is_none_or(|state| state.started_at != *started_at);
            if !stale && now.duration_since(*started_at) < LOGIN_FAILURE_WINDOW {
                break;
            }
            let (key, started_at) = self.order.pop_front().expect("front entry should exist");
            if self
                .entries
                .get(&key)
                .is_some_and(|state| state.started_at == started_at)
            {
                self.entries.remove(&key);
            }
        }
    }

    pub(super) fn evict_oldest(&mut self) {
        while let Some((key, started_at)) = self.order.pop_front() {
            if self
                .entries
                .get(&key)
                .is_some_and(|state| state.started_at == started_at)
            {
                self.entries.remove(&key);
                break;
            }
        }
    }

    pub(super) fn check_allowed(&mut self, key: &LoginFailureKey) -> bool {
        self.prune_expired(Instant::now());
        self.entries
            .get(key)
            .is_none_or(|state| state.failures < MAX_LOGIN_FAILURES)
    }

    pub(super) fn record(&mut self, key: LoginFailureKey, succeeded: bool) {
        let now = Instant::now();
        self.prune_expired(now);
        if succeeded {
            self.entries.remove(&key);
            return;
        }
        if let Some(state) = self.entries.get_mut(&key) {
            state.failures = state.failures.saturating_add(1);
            return;
        }
        while self.entries.len() >= MAX_LOGIN_FAILURE_ENTRIES
            || self.order.len() >= MAX_LOGIN_FAILURE_ENTRIES
        {
            self.evict_oldest();
        }
        self.entries.insert(
            key,
            LoginFailureState {
                failures: 1,
                started_at: now,
            },
        );
        self.order.push_back((key, now));
    }
}

pub(super) fn login_failure_key(username: &str) -> LoginFailureKey {
    Sha256::digest(username.trim().to_ascii_lowercase().as_bytes()).into()
}
