//! Test plumbing shared by the end-to-end suites that look at the exact
//! bytes sent to the model: a recording proxy in front of the mock
//! provider, and a one-page site.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// Read one HTTP/1.1 request (head + Content-Length body) from `r`.
pub fn read_request(r: &mut BufReader<TcpStream>) -> Option<(Vec<String>, Vec<u8>)> {
    let mut head = Vec::new();
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        head.push(line);
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    Some((head, body))
}

/// A proxy to `upstream` that records every request body it forwards.
/// One request per connection, as the engine and the mock both use.
pub fn recording_proxy(upstream: u16) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = Arc::clone(&seen);
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let seen = Arc::clone(&seen2);
            std::thread::spawn(move || {
                let mut r = BufReader::new(client.try_clone().unwrap());
                let Some((head, body)) = read_request(&mut r) else {
                    return;
                };
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&body).into_owned());
                let mut up = TcpStream::connect(("127.0.0.1", upstream)).unwrap();
                for line in &head {
                    // The mock answers one request per connection either way.
                    if !line.to_ascii_lowercase().starts_with("connection:") {
                        up.write_all(line.as_bytes()).unwrap();
                    }
                }
                up.write_all(b"Connection: close\r\n\r\n").unwrap();
                up.write_all(&body).unwrap();
                let mut answer = Vec::new();
                let _ = up.read_to_end(&mut answer);
                let mut client = client;
                let _ = client.write_all(&answer);
            });
        }
    });
    (port, seen)
}

/// A one-page site: every GET returns `body`.
pub fn site(body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let mut r = BufReader::new(s.try_clone().unwrap());
            if read_request(&mut r).is_none() {
                continue;
            }
            let mut s = s;
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}
