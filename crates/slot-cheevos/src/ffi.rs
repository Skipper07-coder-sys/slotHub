//! The parts of rcheevos slot calls, written out by hand from
//! `vendor/rcheevos/include/rc_client.h` (v12.5.0). Structs that slot only
//! reads through a pointer declare just the fields it reads, in order.

#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
pub struct rc_client_t {
    _private: [u8; 0],
}

#[repr(C)]
pub struct rc_client_async_handle_t {
    _private: [u8; 0],
}

#[repr(C)]
pub struct rc_api_request_t {
    pub url: *const c_char,
    pub post_data: *const c_char,
    pub content_type: *const c_char,
}

#[repr(C)]
pub struct rc_api_server_response_t {
    pub body: *const c_char,
    pub body_length: usize,
    pub http_status_code: c_int,
}

#[repr(C)]
pub struct rc_client_user_t {
    pub display_name: *const c_char,
    pub username: *const c_char,
    pub token: *const c_char,
}

#[repr(C)]
pub struct rc_client_game_t {
    pub id: u32,
    pub console_id: u32,
    pub title: *const c_char,
}

#[repr(C)]
pub struct rc_client_achievement_t {
    pub title: *const c_char,
    pub description: *const c_char,
    pub badge_name: [c_char; 8],
    pub measured_progress: [c_char; 24],
    pub measured_percent: f32,
    pub id: u32,
    pub points: u32,
}

#[repr(C)]
pub struct rc_client_server_error_t {
    pub error_message: *const c_char,
    pub api: *const c_char,
    pub result: c_int,
    pub related_id: u32,
}

#[repr(C)]
pub struct rc_client_event_t {
    pub kind: u32,
    pub achievement: *mut rc_client_achievement_t,
    pub leaderboard: *mut c_void,
    pub leaderboard_tracker: *mut c_void,
    pub leaderboard_scoreboard: *mut c_void,
    pub server_error: *mut rc_client_server_error_t,
    pub subset: *mut c_void,
}

#[repr(C)]
#[derive(Default)]
pub struct rc_client_user_game_summary_t {
    pub num_core_achievements: u32,
    pub num_unofficial_achievements: u32,
    pub num_unlocked_achievements: u32,
    pub num_unsupported_achievements: u32,
    pub points_core: u32,
    pub points_unlocked: u32,
    pub beaten_time: i64,
    pub completed_time: i64,
}

#[repr(C)]
pub struct md5_state_t {
    pub count: [u32; 2],
    pub abcd: [u32; 4],
    pub buf: [u8; 64],
}

pub type rc_client_read_memory_func_t = unsafe extern "C" fn(
    address: u32,
    buffer: *mut u8,
    num_bytes: u32,
    client: *mut rc_client_t,
) -> u32;
pub type rc_client_server_callback_t =
    unsafe extern "C" fn(response: *const rc_api_server_response_t, callback_data: *mut c_void);
pub type rc_client_server_call_t = unsafe extern "C" fn(
    request: *const rc_api_request_t,
    callback: rc_client_server_callback_t,
    callback_data: *mut c_void,
    client: *mut rc_client_t,
);
pub type rc_client_callback_t = unsafe extern "C" fn(
    result: c_int,
    error_message: *const c_char,
    client: *mut rc_client_t,
    userdata: *mut c_void,
);
pub type rc_client_message_callback_t =
    unsafe extern "C" fn(message: *const c_char, client: *const rc_client_t);
pub type rc_client_event_handler_t =
    unsafe extern "C" fn(event: *const rc_client_event_t, client: *mut rc_client_t);

pub const RC_OK: c_int = 0;
pub const RC_LOGIN_REQUIRED: c_int = -28;
pub const RC_ABORTED: c_int = -31;
pub const RC_NO_RESPONSE: c_int = -32;
pub const RC_ACCESS_DENIED: c_int = -33;
pub const RC_INVALID_CREDENTIALS: c_int = -34;
pub const RC_EXPIRED_TOKEN: c_int = -35;

pub const RC_API_SERVER_RESPONSE_CLIENT_ERROR: c_int = -1;
pub const RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR: c_int = -2;

pub const RC_CLIENT_LOG_LEVEL_WARN: c_int = 2;

pub const RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED: u32 = 1;
pub const RC_CLIENT_EVENT_GAME_COMPLETED: u32 = 15;
pub const RC_CLIENT_EVENT_SERVER_ERROR: u32 = 16;
pub const RC_CLIENT_EVENT_DISCONNECTED: u32 = 17;
pub const RC_CLIENT_EVENT_RECONNECTED: u32 = 18;

extern "C" {
    pub fn rc_client_create(
        read_memory: rc_client_read_memory_func_t,
        server_call: rc_client_server_call_t,
    ) -> *mut rc_client_t;
    pub fn rc_client_set_hardcore_enabled(client: *mut rc_client_t, enabled: c_int);
    pub fn rc_client_set_allow_background_memory_reads(client: *mut rc_client_t, allowed: c_int);
    pub fn rc_client_enable_logging(
        client: *mut rc_client_t,
        level: c_int,
        callback: rc_client_message_callback_t,
    );
    pub fn rc_client_set_event_handler(
        client: *mut rc_client_t,
        handler: rc_client_event_handler_t,
    );
    pub fn rc_client_get_user_agent_clause(
        client: *mut rc_client_t,
        buffer: *mut c_char,
        buffer_size: usize,
    ) -> usize;
    pub fn rc_client_begin_login_with_password(
        client: *mut rc_client_t,
        username: *const c_char,
        password: *const c_char,
        callback: rc_client_callback_t,
        userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_begin_login_with_token(
        client: *mut rc_client_t,
        username: *const c_char,
        token: *const c_char,
        callback: rc_client_callback_t,
        userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_logout(client: *mut rc_client_t);
    pub fn rc_client_get_user_info(client: *const rc_client_t) -> *const rc_client_user_t;
    pub fn rc_client_begin_load_game(
        client: *mut rc_client_t,
        hash: *const c_char,
        callback: rc_client_callback_t,
        userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_is_game_loaded(client: *const rc_client_t) -> c_int;
    pub fn rc_client_unload_game(client: *mut rc_client_t);
    pub fn rc_client_get_game_info(client: *const rc_client_t) -> *const rc_client_game_t;
    pub fn rc_client_get_user_game_summary(
        client: *const rc_client_t,
        summary: *mut rc_client_user_game_summary_t,
    );
    pub fn rc_client_do_frame(client: *mut rc_client_t);
    pub fn rc_client_idle(client: *mut rc_client_t);
    pub fn rc_client_deserialize_progress_sized(
        client: *mut rc_client_t,
        serialized: *const u8,
        serialized_size: usize,
    ) -> c_int;

    pub fn md5_init(pms: *mut md5_state_t);
    pub fn md5_append(pms: *mut md5_state_t, data: *const u8, nbytes: c_int);
    pub fn md5_finish(pms: *mut md5_state_t, digest: *mut u8);
}
