//! Mock HTTP integration tests for the shared download pipeline.
//!
//! The tests use a real loopback TCP server so redirects, status codes, timeouts,
//! disconnects, size checks and cancellation are exercised over an actual HTTP exchange.
//! The server is plain HTTP on an ephemeral port and is reachable only through
//! `Allowlist::for_local_test`; production allowlists still require HTTPS on port 443.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use download_pipeline::{
    Allowlist, Checksum, DownloadClient, DownloadPhase, DownloadSpec, NetworkConfig, PipelineError,
};

enum Reply {
    Response {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
    },
    Close,
    Delay(Duration, Box<Reply>),
}

impl Reply {
    fn response(status: u16, body: &[u8]) -> Self {
        Reply::Response {
            status,
            headers: Vec::new(),
            body: body.to_vec(),
        }
    }

    fn redirect(location: &str) -> Self {
        Reply::Response {
            status: 302,
            headers: vec![("Location", location.to_owned())],
            body: Vec::new(),
        }
    }

    fn delay(delay: Duration, reply: Reply) -> Self {
        Reply::Delay(delay, Box::new(reply))
    }
}

struct Server {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    _thread: thread::JoinHandle<()>,
}

impl Server {
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock HTTP server");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("mock server address")
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_thread = Arc::clone(&requests);
        let total = replies.len();
        let thread = thread::spawn(move || {
            let mut replies = replies.into_iter();
            for _ in 0..total {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let request = read_request(&mut stream);
                if let Ok(mut recorded) = requests_thread.lock() {
                    recorded.push(request);
                }
                let Some(reply) = replies.next() else {
                    break;
                };
                serve(&mut stream, reply);
            }
        });
        Self {
            base_url,
            requests,
            _thread: thread,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }

    fn request_count(&self) -> usize {
        self.requests.lock().expect("requests").len()
    }
}

fn read_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                data.extend_from_slice(&buffer[..read]);
                if data.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

fn serve(stream: &mut TcpStream, reply: Reply) {
    match reply {
        Reply::Close => {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        Reply::Response {
            status,
            headers,
            body,
        } => write_response(stream, status, &headers, &body),
        Reply::Delay(delay, reply) => {
            thread::sleep(delay);
            serve(stream, *reply);
        }
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&'static str, String)],
    body: &[u8],
) {
    let reason = match status {
        200 => "OK",
        302 => "Found",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Status",
    };
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-mock-http-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp directory");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn allowlist() -> Allowlist {
    Allowlist::for_local_test(&["127.0.0.1"], "local mock HTTP")
}

fn client(attempts: u32) -> DownloadClient {
    let mut config = NetworkConfig::from_lookup(|_| None);
    config.attempts = attempts;
    config.retry_base_delay = Duration::from_millis(1);
    config.connect_timeout = Duration::from_millis(500);
    config.read_timeout = Duration::from_millis(500);
    DownloadClient::new(config)
}

fn sha1(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    use std::fmt::Write as _;
    let digest = Sha1::digest(bytes);
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

#[test]
fn fetches_metadata_and_publishes_a_verified_artifact() {
    let payload = b"verified artifact";
    let server = Server::start(vec![
        Reply::response(200, b"manifest"),
        Reply::response(200, payload),
    ]);
    let client = client(1);

    let manifest = client
        .get_bytes(&server.url("/manifest"), &allowlist(), 1024)
        .expect("metadata");
    assert_eq!(manifest, b"manifest");

    let dir = TempDir::new("success");
    let destination = dir.path.join("client.jar");
    let events = std::cell::RefCell::new(Vec::new());
    let outcome = client
        .download(
            &DownloadSpec {
                url: &server.url("/client.jar"),
                allowlist: &allowlist(),
                destination: &destination,
                expected_size: payload.len() as u64,
                checksum: Checksum::Sha1(&sha1(payload)),
                max_size: 1024,
            },
            &AtomicBool::new(false),
            &|event| events.borrow_mut().push(event),
        )
        .expect("verified download");

    assert_eq!(outcome.bytes, payload.len() as u64);
    assert_eq!(fs::read(&destination).expect("artifact"), payload);
    assert!(events
        .borrow()
        .iter()
        .any(|event| event.phase == DownloadPhase::Verified));
    assert_eq!(server.request_count(), 2);
}

#[test]
fn not_found_is_reported_and_not_retried() {
    let server = Server::start(vec![Reply::response(404, b"missing")]);
    let error = client(3)
        .get_bytes(&server.url("/missing"), &allowlist(), 1024)
        .expect_err("404");
    assert!(matches!(error, PipelineError::Http { status: 404, .. }));
    assert_eq!(server.request_count(), 1, "404 must not be retried");
}

#[test]
fn timeout_is_reported_as_a_transport_error() {
    let server = Server::start(vec![Reply::delay(
        Duration::from_millis(300),
        Reply::response(200, b"late"),
    )]);
    let mut config = NetworkConfig::from_lookup(|_| None);
    config.attempts = 1;
    config.read_timeout = Duration::from_millis(50);
    let error = DownloadClient::new(config)
        .get_bytes(&server.url("/slow"), &allowlist(), 1024)
        .expect_err("timeout");
    assert!(matches!(error, PipelineError::Transport(_)), "{error:?}");
}

#[test]
fn follows_a_relative_redirect_on_the_same_host() {
    let server = Server::start(vec![
        Reply::redirect("/final"),
        Reply::response(200, b"redirected"),
    ]);
    let body = client(1)
        .get_bytes(&server.url("/start"), &allowlist(), 1024)
        .expect("redirect");
    assert_eq!(body, b"redirected");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /start "), "{:?}", requests[0]);
    assert!(requests[1].starts_with("GET /final "), "{:?}", requests[1]);
}

#[test]
fn refuses_a_redirect_to_a_disallowed_host() {
    let server = Server::start(vec![Reply::redirect("http://evil.example/x")]);
    let error = client(1)
        .get_bytes(&server.url("/start"), &allowlist(), 1024)
        .expect_err("disallowed redirect");
    assert!(
        matches!(error, PipelineError::DisallowedUrl(_)),
        "{error:?}"
    );
    assert_eq!(server.request_count(), 1);
}

#[test]
fn checksum_mismatch_is_definitive_and_cleans_up() {
    let payload = b"payload";
    let server = Server::start(vec![Reply::response(200, payload)]);
    let dir = TempDir::new("checksum");
    let destination = dir.path.join("client.jar");
    let wrong = "b".repeat(40);
    let error = client(3)
        .download(
            &DownloadSpec {
                url: &server.url("/client.jar"),
                allowlist: &allowlist(),
                destination: &destination,
                expected_size: payload.len() as u64,
                checksum: Checksum::Sha1(&wrong),
                max_size: 1024,
            },
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("checksum mismatch");
    assert!(matches!(error, PipelineError::Checksum { .. }), "{error:?}");
    assert_eq!(server.request_count(), 1, "checksum errors are not retried");
    assert!(!destination.exists());
    assert_eq!(fs::read_dir(&dir.path).expect("dir").count(), 0);
}

#[test]
fn wrong_size_is_reported_before_any_bytes_are_published() {
    let payload = b"short";
    let server = Server::start(vec![Reply::response(200, payload)]);
    let dir = TempDir::new("size");
    let destination = dir.path.join("client.jar");
    let expected = b"short!";
    let error = client(3)
        .download(
            &DownloadSpec {
                url: &server.url("/client.jar"),
                allowlist: &allowlist(),
                destination: &destination,
                expected_size: expected.len() as u64,
                checksum: Checksum::Sha1(&sha1(expected)),
                max_size: 1024,
            },
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("size mismatch");
    assert!(
        matches!(
            error,
            PipelineError::Size {
                expected: 6,
                actual: 5
            }
        ),
        "{error:?}"
    );
    assert_eq!(server.request_count(), 1);
    assert!(!destination.exists());
}

#[test]
fn cancellation_removes_the_partial_file() {
    let payload = vec![7u8; 4096];
    let server = Server::start(vec![Reply::response(200, &payload)]);
    let dir = TempDir::new("cancel");
    let destination = dir.path.join("client.jar");
    let cancel = AtomicBool::new(false);
    let checksum = sha1(&payload);
    let error = client(1)
        .download(
            &DownloadSpec {
                url: &server.url("/client.jar"),
                allowlist: &allowlist(),
                destination: &destination,
                expected_size: payload.len() as u64,
                checksum: Checksum::Sha1(&checksum),
                max_size: 1024 * 1024,
            },
            &cancel,
            &|event| {
                if event.phase == DownloadPhase::Downloading {
                    cancel.store(true, Ordering::SeqCst);
                }
            },
        )
        .expect_err("cancelled");
    assert!(matches!(error, PipelineError::Cancelled), "{error:?}");
    assert!(!destination.exists());
    assert!(
        !fs::read_dir(&dir.path)
            .expect("dir")
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".download-")),
        "the temporary file must be removed"
    );
}

#[test]
fn disconnect_is_reported_as_a_transport_error() {
    let server = Server::start(vec![Reply::Close]);
    let error = client(1)
        .get_bytes(&server.url("/gone"), &allowlist(), 1024)
        .expect_err("disconnect");
    assert!(matches!(error, PipelineError::Transport(_)), "{error:?}");
}

#[test]
fn retries_a_transient_status_then_succeeds() {
    let server = Server::start(vec![
        Reply::response(503, b"busy"),
        Reply::response(200, b"ok"),
    ]);
    let body = client(2)
        .get_bytes(&server.url("/retry"), &allowlist(), 1024)
        .expect("retry succeeds");
    assert_eq!(body, b"ok");
    assert_eq!(server.request_count(), 2);
}
