//! HTTP for rcheevos: one worker thread that runs curl for each request.
//! The POST body goes to curl's stdin, never its arguments, because `ps`
//! output reaches bug reports. Bodies are never logged, either way.

use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

use crate::ffi::{
    RC_API_SERVER_RESPONSE_CLIENT_ERROR, RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR,
};

/// Bytes that are overwritten when they are dropped: request bodies carry
/// passwords and tokens.
pub struct Wiped(pub Vec<u8>);

impl Drop for Wiped {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

pub(crate) fn wipe(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // Volatile so the compiler cannot drop a write to memory about to be freed.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
}

pub struct Request {
    pub url: String,
    pub body: Option<Wiped>,
    pub content_type: String,
    pub reply: Box<dyn FnOnce(Response) + Send>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Response {
    /// The HTTP status, or one of rcheevos's client error codes.
    pub status: i32,
    pub body: Vec<u8>,
}

static QUEUE: OnceLock<Mutex<Option<Sender<Request>>>> = OnceLock::new();

/// Queues a request for the worker, starting it the first time.
pub fn send(user_agent: &str, request: Request) {
    let queue = QUEUE.get_or_init(|| Mutex::new(None));
    let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
    if queue.is_none() {
        let (tx, rx) = channel::<Request>();
        let agent = user_agent.to_string();
        let spawned = std::thread::Builder::new()
            .name("slot-cheevos-net".into())
            .spawn(move || {
                for request in rx {
                    let response = fetch(&agent, &request);
                    (request.reply)(response);
                }
            });
        match spawned {
            Ok(_) => *queue = Some(tx),
            Err(e) => eprintln!("slot: cheevos: the network thread failed to start: {e}"),
        }
    }
    match queue.as_ref() {
        Some(tx) => {
            if let Err(e) = tx.send(request) {
                *queue = None;
                (e.0.reply)(failed(true));
            }
        }
        None => (request.reply)(failed(true)),
    }
}

fn curl() -> OsString {
    std::env::var_os("SLOT_CURL").unwrap_or_else(|| "curl".into())
}

/// The curl command for one request. The body is not in it: it goes to stdin.
pub fn command(curl: OsString, user_agent: &str, url: &str, content_type: Option<&str>) -> Command {
    let mut cmd = Command::new(curl);
    cmd.args(["-sS", "--connect-timeout", "10", "--max-time", "30", "-A"])
        .arg(user_agent);
    if let Some(ct) = content_type {
        cmd.arg("-H")
            .arg(format!("Content-Type: {ct}"))
            .args(["--data-binary", "@-"]);
    }
    cmd.args(["-w", "\n%{http_code}"]).arg(url);
    cmd
}

fn fetch(user_agent: &str, request: &Request) -> Response {
    let content_type = request
        .body
        .as_ref()
        .map(|_| match request.content_type.as_str() {
            "" => "application/x-www-form-urlencoded",
            ct => ct,
        });
    let mut cmd = command(curl(), user_agent, &request.url, content_type);
    cmd.stdin(if request.body.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("slot: cheevos: curl would not start: {}", e.kind());
            return failed(false);
        }
    };
    if let (Some(mut stdin), Some(body)) = (child.stdin.take(), request.body.as_ref()) {
        // A write that fails shows up as curl's exit status.
        let _ = stdin.write_all(&body.0);
    }
    let out = match child.wait_with_output() {
        Ok(out) => out,
        Err(e) => {
            eprintln!("slot: cheevos: curl did not finish: {}", e.kind());
            return failed(true);
        }
    };
    if !out.status.success() {
        eprintln!(
            "slot: cheevos: a request failed, curl exit {:?}",
            out.status.code()
        );
        return failed(true);
    }
    parse(out.stdout)
}

fn failed(retryable: bool) -> Response {
    Response {
        status: if retryable {
            RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR
        } else {
            RC_API_SERVER_RESPONSE_CLIENT_ERROR
        },
        body: Vec::new(),
    }
}

/// Splits curl's output into the body and the status `-w` wrote after it.
pub fn parse(mut out: Vec<u8>) -> Response {
    let Some(at) = out.iter().rposition(|&b| b == b'\n') else {
        return failed(false);
    };
    let status = std::str::from_utf8(&out[at + 1..])
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok());
    match status {
        Some(0) => failed(true),
        Some(status) => {
            out.truncate(at);
            Response { status, body: out }
        }
        None => failed(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::with_curl;

    #[test]
    fn the_status_curl_writes_last_is_split_from_the_body() {
        let r = parse(b"{\"Success\":true}\n200".to_vec());
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"{\"Success\":true}");
        let r = parse(b"line one\nline two\n401".to_vec());
        assert_eq!(r.status, 401);
        assert_eq!(r.body, b"line one\nline two");
        assert_eq!(parse(b"\n503".to_vec()).body, b"");
    }

    #[test]
    fn output_without_a_status_is_a_client_error() {
        assert_eq!(
            parse(Vec::new()).status,
            RC_API_SERVER_RESPONSE_CLIENT_ERROR
        );
        assert_eq!(
            parse(b"body\nnope".to_vec()).status,
            RC_API_SERVER_RESPONSE_CLIENT_ERROR
        );
        assert_eq!(
            parse(b"\n000".to_vec()).status,
            RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR
        );
    }

    #[test]
    fn the_body_never_goes_on_curls_command_line() {
        let cmd = command(
            "curl".into(),
            "slot/v1 (Linux; aarch64)",
            "https://retroachievements.org/dorequest.php",
            Some("application/x-www-form-urlencoded"),
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w == ["--data-binary", "@-"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["-A", "slot/v1 (Linux; aarch64)"]));
        assert_eq!(
            args.last().unwrap(),
            "https://retroachievements.org/dorequest.php"
        );
        assert!(!args.iter().any(|a| a.contains("p=") || a.contains("t=")));
    }

    #[test]
    fn a_request_runs_curl_with_the_body_on_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("stdin");
        let script = format!("cat > '{}'\nprintf 'ok\\n200'\n", seen.display());
        let response = with_curl(dir.path(), &script, || {
            fetch("ua", &post(b"r=login2&u=me&p=secret"))
        });
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
        assert_eq!(std::fs::read(&seen).unwrap(), b"r=login2&u=me&p=secret");
    }

    #[test]
    fn a_curl_that_fails_is_worth_trying_again() {
        let dir = tempfile::tempdir().unwrap();
        let response = with_curl(dir.path(), "cat > /dev/null\nexit 28\n", || {
            fetch("ua", &post(b"x"))
        });
        assert_eq!(
            response.status,
            RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR
        );
    }

    #[test]
    fn wiped_bytes_are_zeroed() {
        let mut v = b"hunter2".to_vec();
        wipe(&mut v);
        assert_eq!(v, [0u8; 7]);
    }

    fn post(body: &[u8]) -> Request {
        Request {
            url: "http://example.invalid/".into(),
            body: Some(Wiped(body.to_vec())),
            content_type: String::new(),
            reply: Box::new(|_| {}),
        }
    }
}
