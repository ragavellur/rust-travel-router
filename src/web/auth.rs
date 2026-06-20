use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

static PASSWORD_PROTECTED: AtomicBool = AtomicBool::new(false);
static WEB_PASSWORD: Mutex<String> = Mutex::new(String::new());

thread_local! {
    static SESSIONS: Mutex<HashMap<String, Instant>> = Mutex::new(HashMap::new());
}

const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

pub fn set_password(pass: &str) {
    *WEB_PASSWORD.lock().unwrap() = pass.to_string();
    PASSWORD_PROTECTED.store(true, Ordering::SeqCst);
}

pub fn is_password_set() -> bool {
    PASSWORD_PROTECTED.load(Ordering::SeqCst)
}

pub fn verify_password(pass: &str) -> bool {
    *WEB_PASSWORD.lock().unwrap() == pass
}

pub fn create_session() -> String {
    let token = Uuid::new_v4().to_string();
    SESSIONS.with(|s| {
        let mut map = s.lock().unwrap();
        map.insert(token.clone(), Instant::now());
        // Clean expired
        map.retain(|_, v| v.elapsed() < SESSION_TTL);
        token
    })
}

pub fn validate_session(token: &str) -> bool {
    SESSIONS.with(|s| {
        let mut map = s.lock().unwrap();
        if let Some(created) = map.get(token) {
            if created.elapsed() < SESSION_TTL {
                true
            } else {
                map.remove(token);
                false
            }
        } else {
            false
        }
    })
}

pub fn clear_session(token: &str) {
    SESSIONS.with(|s| {
        s.lock().unwrap().remove(token);
    });
}

pub fn clear_all_sessions() {
    SESSIONS.with(|s| {
        s.lock().unwrap().clear();
    });
}
