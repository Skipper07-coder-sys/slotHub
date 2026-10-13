//! The real [`Cheevos`]: one `rc_client` for the whole process.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::{Arc, Mutex, OnceLock};

use crate::ffi::*;
use crate::net::{self, Wiped};
use crate::{Cheevos, Event, Region, Secret};

/// Events beyond this many, not yet taken by the app, are dropped.
const EVENTS_MAX: usize = 64;

static EVENTS: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

static CLIENT: OnceLock<Arc<RcClient>> = OnceLock::new();

static USER_AGENT: OnceLock<String> = OnceLock::new();

thread_local! {
    static MEMORY: RefCell<Vec<Region>> = const { RefCell::new(Vec::new()) };
}

struct RcClient(*mut rc_client_t);

// rc_client guards its own state with a mutex, and is made to be called from
// the emulator, UI and network threads.
unsafe impl Send for RcClient {}
unsafe impl Sync for RcClient {}

/// The process-wide client. `product` begins the User-Agent, such as
/// "slot/v1.2.3"; the first call's is kept.
pub fn client(product: &str) -> Arc<dyn Cheevos> {
    CLIENT
        .get_or_init(|| {
            let ptr = unsafe { rc_client_create(read_memory, server_call) };
            unsafe {
                rc_client_set_hardcore_enabled(ptr, 0);
                rc_client_set_allow_background_memory_reads(ptr, 0);
                rc_client_enable_logging(ptr, RC_CLIENT_LOG_LEVEL_WARN, log_message);
                rc_client_set_event_handler(ptr, event_handler);
            }
            let mut clause = [0 as c_char; 64];
            unsafe { rc_client_get_user_agent_clause(ptr, clause.as_mut_ptr(), clause.len()) };
            let clause = unsafe { text(clause.as_ptr()) };
            let _ = USER_AGENT.set(format!("{product} (Linux; aarch64) {clause}").trim().into());
            Arc::new(RcClient(ptr))
        })
        .clone()
}

fn push(event: Event) {
    let mut events = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    if events.len() < EVENTS_MAX {
        events.push_back(event);
    }
}

unsafe fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    CStr::from_ptr(p).to_string_lossy().into_owned()
}

/// A log line with anything that might be a password or token taken out.
pub fn redact(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if ["token", "password", "&p=", "&t=", "?p=", "?t="]
        .iter()
        .any(|w| lower.contains(w))
    {
        return "(a message about sign-in, not shown)".into();
    }
    message.into()
}

unsafe extern "C" fn log_message(message: *const c_char, _client: *const rc_client_t) {
    eprintln!("slot: cheevos: {}", redact(&text(message)));
}

unsafe extern "C" fn read_memory(
    address: u32,
    buffer: *mut u8,
    num_bytes: u32,
    _client: *mut rc_client_t,
) -> u32 {
    if buffer.is_null() {
        return 0;
    }
    let out = std::slice::from_raw_parts_mut(buffer, num_bytes as usize);
    MEMORY.with(|m| read_regions(&m.borrow(), address, out))
}

/// Copies from the regions starting at `address`, stopping at a gap.
fn read_regions(regions: &[Region], address: u32, out: &mut [u8]) -> u32 {
    let mut done = 0usize;
    while done < out.len() {
        let at = address as u64 + done as u64;
        let Some(r) = regions
            .iter()
            .find(|r| at >= r.start as u64 && at < r.start as u64 + r.len as u64)
        else {
            break;
        };
        let offset = (at - r.start as u64) as usize;
        let n = (r.len - offset).min(out.len() - done);
        if r.ptr.is_null() {
            break;
        }
        // The core keeps this memory for as long as the regions are set.
        unsafe { std::ptr::copy_nonoverlapping(r.ptr.add(offset), out[done..].as_mut_ptr(), n) };
        done += n;
    }
    done as u32
}

unsafe extern "C" fn server_call(
    request: *const rc_api_request_t,
    callback: rc_client_server_callback_t,
    callback_data: *mut c_void,
    _client: *mut rc_client_t,
) {
    let reply_to = (callback as usize, callback_data as usize);
    let reply = Box::new(move |response: net::Response| {
        let callback: rc_client_server_callback_t = std::mem::transmute(reply_to.0);
        let body: &[u8] = if response.body.is_empty() {
            b"\0"
        } else {
            &response.body
        };
        let raw = rc_api_server_response_t {
            body: body.as_ptr() as *const c_char,
            body_length: response.body.len(),
            http_status_code: response.status,
        };
        callback(&raw, reply_to.1 as *mut c_void);
    });
    let Some(request) = request.as_ref() else {
        return reply(net::Response {
            status: RC_API_SERVER_RESPONSE_CLIENT_ERROR,
            body: Vec::new(),
        });
    };
    let body = (!request.post_data.is_null())
        .then(|| Wiped(CStr::from_ptr(request.post_data).to_bytes().to_vec()));
    let request = net::Request {
        url: text(request.url),
        body,
        content_type: text(request.content_type),
        reply,
    };
    let agent = USER_AGENT.get().map_or("slot", String::as_str);
    net::send(agent, request);
}

unsafe extern "C" fn event_handler(event: *const rc_client_event_t, _client: *mut rc_client_t) {
    let Some(event) = event.as_ref() else {
        return;
    };
    match event.kind {
        RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED => {
            if let Some(a) = event.achievement.as_ref() {
                push(Event::Unlocked {
                    title: text(a.title),
                    description: text(a.description),
                    points: a.points,
                });
            }
        }
        RC_CLIENT_EVENT_GAME_COMPLETED => {
            let game = rc_client_get_game_info(CLIENT.get().map_or(std::ptr::null(), |c| c.0));
            let title = game.as_ref().map(|g| text(g.title)).unwrap_or_default();
            push(Event::Completed { title });
        }
        RC_CLIENT_EVENT_SERVER_ERROR => {
            let message = event
                .server_error
                .as_ref()
                .map(|e| redact(&text(e.error_message)))
                .unwrap_or_default();
            eprintln!("slot: cheevos: server error: {message}");
            push(Event::ServerError { message });
        }
        RC_CLIENT_EVENT_DISCONNECTED => eprintln!("slot: cheevos: unlocks waiting for the network"),
        RC_CLIENT_EVENT_RECONNECTED => eprintln!("slot: cheevos: waiting unlocks sent"),
        _ => {}
    }
}

fn rejected(result: c_int) -> bool {
    matches!(
        result,
        RC_INVALID_CREDENTIALS | RC_EXPIRED_TOKEN | RC_ACCESS_DENIED
    )
}

unsafe extern "C" fn login_done(
    result: c_int,
    error_message: *const c_char,
    client: *mut rc_client_t,
    _userdata: *mut c_void,
) {
    if result != RC_OK {
        let message = redact(&text(error_message));
        eprintln!("slot: cheevos: sign-in failed ({result}): {message}");
        push(Event::SignInFailed {
            message,
            rejected: rejected(result),
        });
        return;
    }
    let Some(user) = rc_client_get_user_info(client).as_ref() else {
        return;
    };
    let username = text(user.username);
    eprintln!("slot: cheevos: signed in as {username}");
    push(Event::SignedIn {
        username,
        token: Secret::new(text(user.token)),
    });
}

unsafe extern "C" fn load_done(
    result: c_int,
    error_message: *const c_char,
    client: *mut rc_client_t,
    _userdata: *mut c_void,
) {
    if result == RC_ABORTED {
        return;
    }
    if result != RC_OK {
        let message = redact(&text(error_message));
        eprintln!("slot: cheevos: no game loaded ({result}): {message}");
        push(Event::GameFailed { message });
        return;
    }
    let title = rc_client_get_game_info(client)
        .as_ref()
        .map(|g| text(g.title))
        .unwrap_or_default();
    let mut summary = rc_client_user_game_summary_t::default();
    rc_client_get_user_game_summary(client, &mut summary);
    eprintln!(
        "slot: cheevos: {title}: {} of {} unlocked",
        summary.num_unlocked_achievements, summary.num_core_achievements
    );
    push(Event::GameLoaded {
        title,
        achievements: summary.num_core_achievements,
        unlocked: summary.num_unlocked_achievements,
    });
}

/// A C string that is wiped when dropped.
struct WipedC(Option<CString>);

impl WipedC {
    fn new(s: &str) -> Option<WipedC> {
        CString::new(s).ok().map(|c| WipedC(Some(c)))
    }

    fn as_ptr(&self) -> *const c_char {
        self.0.as_ref().map_or(std::ptr::null(), |c| c.as_ptr())
    }
}

impl Drop for WipedC {
    fn drop(&mut self) {
        if let Some(c) = self.0.take() {
            net::wipe(&mut c.into_bytes_with_nul());
        }
    }
}

impl Cheevos for RcClient {
    fn sign_in(&self, username: &str, password: Secret) {
        let (Ok(user), Some(pass)) = (CString::new(username), WipedC::new(password.expose()))
        else {
            return;
        };
        drop(password);
        unsafe {
            rc_client_begin_login_with_password(
                self.0,
                user.as_ptr(),
                pass.as_ptr(),
                login_done,
                std::ptr::null_mut(),
            )
        };
    }

    fn resume(&self, username: &str, token: &Secret) {
        let (Ok(user), Some(token)) = (CString::new(username), WipedC::new(token.expose())) else {
            return;
        };
        unsafe {
            rc_client_begin_login_with_token(
                self.0,
                user.as_ptr(),
                token.as_ptr(),
                login_done,
                std::ptr::null_mut(),
            )
        };
    }

    fn sign_out(&self) {
        unsafe { rc_client_logout(self.0) };
    }

    fn signed_in(&self) -> bool {
        !unsafe { rc_client_get_user_info(self.0) }.is_null()
    }

    fn load_game(&self, md5: &str) {
        let Ok(hash) = CString::new(md5) else {
            return;
        };
        unsafe {
            rc_client_begin_load_game(self.0, hash.as_ptr(), load_done, std::ptr::null_mut())
        };
    }

    fn unload_game(&self) {
        unsafe { rc_client_unload_game(self.0) };
    }

    fn set_memory(&self, regions: Vec<Region>) {
        MEMORY.with(|m| *m.borrow_mut() = regions);
    }

    fn do_frame(&self) {
        unsafe { rc_client_do_frame(self.0) };
    }

    fn idle(&self) {
        unsafe { rc_client_idle(self.0) };
    }

    fn reset_progress(&self) {
        if unsafe { rc_client_is_game_loaded(self.0) } != 0 {
            unsafe { rc_client_deserialize_progress_sized(self.0, std::ptr::null(), 0) };
        }
    }

    fn take_events(&self) -> Vec<Event> {
        EVENTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_find_the_region_and_cross_into_the_next() {
        let iwram = [1u8, 2, 3, 4];
        let ewram = [5u8, 6, 7, 8];
        let regions = [
            Region {
                start: 0,
                ptr: iwram.as_ptr(),
                len: 4,
            },
            Region {
                start: 4,
                ptr: ewram.as_ptr(),
                len: 4,
            },
        ];
        let mut out = [0u8; 4];
        assert_eq!(read_regions(&regions, 2, &mut out), 4);
        assert_eq!(out, [3, 4, 5, 6]);
        assert_eq!(read_regions(&regions, 7, &mut out), 1);
        assert_eq!(out[0], 8);
        assert_eq!(read_regions(&regions, 8, &mut out), 0);
        assert_eq!(read_regions(&[], 0, &mut out), 0);
    }

    #[test]
    fn log_lines_about_tokens_or_passwords_are_withheld() {
        for line in [
            "Login failed: Invalid User/Password combination",
            "token abc123 expired",
            "posting r=login2&u=me&p=hunter2",
            "r=awardachievement&t=abc123",
        ] {
            let shown = redact(line);
            assert!(
                !shown.contains("hunter2") && !shown.contains("abc123"),
                "{shown}"
            );
        }
        assert_eq!(redact("Game 123 loaded"), "Game 123 loaded");
    }

    #[test]
    fn the_client_starts_with_no_user_and_no_game() {
        let c = client("slot/test");
        assert!(USER_AGENT
            .get()
            .unwrap()
            .starts_with("slot/test (Linux; aarch64) rcheevos/"));
        c.do_frame();
        c.idle();
        c.reset_progress();
        c.unload_game();
    }

    #[test]
    fn a_password_sign_in_posts_on_stdin_and_hands_back_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("stdin");
        let script = format!(
            "cat > '{}'\nprintf '%s\\n200' '{{\"Success\":true,\"User\":\"me\",\"Token\":\"tok123\",\"Score\":1,\"SoftcoreScore\":2,\"Messages\":0,\"DisplayName\":\"me\"}}'\n",
            seen.display()
        );
        let c = client("slot/test");
        let events = crate::testing::with_curl(dir.path(), &script, || {
            c.sign_in("me", Secret::new("hunter2".into()));
            let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut events = Vec::new();
            while events.is_empty() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(10));
                events = c.take_events();
            }
            events
        });
        c.sign_out();
        assert_eq!(
            events,
            [Event::SignedIn {
                username: "me".into(),
                token: Secret::new("tok123".into())
            }]
        );
        let posted = std::fs::read_to_string(&seen).unwrap();
        assert!(
            posted.contains("u=me") && posted.contains("p=hunter2"),
            "{posted}"
        );
    }
}
