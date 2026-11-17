//! Shared raw HTTP/1.0 request writer for the local ge-bridge server (used by the ACP client and the CLI).
//! The server ends the body by closing the connection, so callers read to the end themselves.

use crate::{config, state};
use serde_json::Value;
use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

/// Which step failed; callers turn each into their own user-facing text.
#[derive(Debug)]
pub enum SendError {
    Connect(std::io::Error),
    Write(std::io::Error),
}

/// Port and pairing token of the local server, from the runtime config and stored state.
pub fn local_endpoint() -> (u16, String) {
    config::init(config::RuntimeConfig::from_env());
    (state::PersistedPrefs::load().port, state::load_or_create_token())
}

/// Connect to 127.0.0.1:`port`, set the read timeout and write one request; the caller reads the answer.
pub fn send(
    port: u16,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
    read_timeout: Duration,
) -> Result<TcpStream, SendError> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(SendError::Connect)?;
    stream.set_read_timeout(Some(read_timeout)).ok();
    let payload = body.map(Value::to_string).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(req.as_bytes()).map_err(SendError::Write)?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn writes_exact_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = json!({"a": 1});
        let stream = send(port, "tok", "POST", "/x", Some(&body), Duration::from_secs(1)).unwrap();
        drop(stream);
        let (mut conn, _) = listener.accept().unwrap();
        let mut got = String::new();
        conn.read_to_string(&mut got).unwrap();
        assert_eq!(
            got,
            format!("POST /x HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer tok\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{{\"a\":1}}")
        );
    }

    #[test]
    fn closed_port_is_connect_error() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let err = send(port, "t", "GET", "/", None, Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, SendError::Connect(_)));
    }
}
