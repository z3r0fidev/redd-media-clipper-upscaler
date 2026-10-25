//! Security requirements of the media server (research.md §R8), exercised over raw TCP.

use std::io::{Read, Write};
use std::net::TcpStream;

use rmedia_desktop::media_server::MediaServer;

fn request(port: u16, raw: &str) -> (u16, String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, head, buf[split + 4..].to_vec())
}

fn get(port: u16, path: &str, extra: &str) -> (u16, String, Vec<u8>) {
    request(port, &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}\r\n"))
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_only_registered_files_with_the_token() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("v.mp4");
    std::fs::write(&file, b"0123456789").unwrap();
    std::fs::write(dir.path().join("secret.txt"), b"secret").unwrap();
    let server = MediaServer::start().await.unwrap();
    let port = server.port();
    let url = server.register(&file).unwrap();
    let path = url.strip_prefix(&format!("http://127.0.0.1:{port}")).unwrap().to_owned();
    let (token, id) = {
        let mut p = path.trim_start_matches('/').split('/');
        (p.next().unwrap().to_owned(), p.next().unwrap().to_owned())
    };
    assert_eq!(token.len(), 32, "128-bit token");
    assert_eq!(server.register(&file).unwrap(), url, "stable URL per file");

    tokio::task::spawn_blocking(move || {
        // Full file and headers.
        let (st, head, body) = get(port, &path, "");
        assert_eq!((st, body.as_slice()), (200, &b"0123456789"[..]));
        for h in [
            "Content-Type: video/mp4",
            "Accept-Ranges: bytes",
            "Cache-Control: no-store",
            "X-Content-Type-Options: nosniff",
        ] {
            assert!(head.contains(h), "missing {h}: {head}");
        }
        assert!(!head.to_lowercase().contains("access-control-allow-origin"));
        // Ranges.
        let (st, head, body) = get(port, &path, "Range: bytes=2-5\r\n");
        assert_eq!((st, body.as_slice()), (206, &b"2345"[..]));
        assert!(head.contains("Content-Range: bytes 2-5/10"));
        assert_eq!(get(port, &path, "Range: bytes=-3\r\n").2, b"789");
        assert_eq!(get(port, &path, "Range: bytes=0-1,4-5\r\n").0, 416);
        assert_eq!(get(port, &path, "Range: bytes=20-\r\n").0, 416);
        // HEAD, other methods.
        let (st, _, body) = request(port, &format!("HEAD {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"));
        assert_eq!((st, body.len()), (200, 0));
        assert_eq!(request(port, &format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")).0, 405);
        // Token, id, traversal.
        assert_eq!(get(port, &format!("/{}/{id}", "0".repeat(32)), "").0, 404);
        assert_eq!(get(port, &format!("/{token}/{}", "f".repeat(32)), "").0, 404);
        assert_eq!(get(port, &format!("/{token}/../secret.txt"), "").0, 404);
        assert_eq!(get(port, &format!("/{token}/%2e%2e%2fsecret.txt"), "").0, 404);
        assert_eq!(get(port, &format!("/{token}"), "").0, 404);
        assert_eq!(get(port, "/", "").0, 404);
        // DNS rebinding / foreign pages.
        assert_eq!(
            request(port, &format!("GET {path} HTTP/1.1\r\nHost: evil.example:{port}\r\n\r\n")).0,
            403
        );
        assert_eq!(request(port, &format!("GET {path} HTTP/1.1\r\n\r\n")).0, 403, "Host is required");
        assert_eq!(get(port, &path, "Origin: https://evil.example\r\n").0, 403);
        assert_eq!(get(port, &path, "Origin: tauri://localhost\r\n").0, 200);
        // Garbage.
        assert_eq!(request(port, "NONSENSE\r\n\r\n").0, 400);
        // Oversized headers: rejected with 400, or the connection is dropped mid-request (reset).
        let huge =
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX: {}\r\n\r\n", "a".repeat(20_000));
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let _ = s.write_all(huge.as_bytes());
        let mut buf = Vec::new();
        let _ = s.read_to_end(&mut buf);
        let text = String::from_utf8_lossy(&buf);
        assert!(buf.is_empty() || text.starts_with("HTTP/1.1 400"), "oversized header accepted: {text}");
    })
    .await
    .unwrap();
}

/// T065: a finished GIF is served as `image/gif`, unsniffed, with ranges.
#[tokio::test(flavor = "multi_thread")]
async fn gifs_are_served_as_image_gif() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("out.gif");
    std::fs::write(&file, b"GIF89a0123").unwrap();
    let server = MediaServer::start().await.unwrap();
    let port = server.port();
    let url = server.register(&file).unwrap();
    let path = url.strip_prefix(&format!("http://127.0.0.1:{port}")).unwrap().to_owned();
    tokio::task::spawn_blocking(move || {
        let (st, head, body) = get(port, &path, "");
        assert_eq!((st, body.as_slice()), (200, &b"GIF89a0123"[..]));
        assert!(head.contains("Content-Type: image/gif"), "{head}");
        assert!(head.contains("X-Content-Type-Options: nosniff"), "{head}");
        let (st, _, body) = get(port, &path, "Range: bytes=0-5\r\n");
        assert_eq!((st, body.as_slice()), (206, &b"GIF89a"[..]));
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_registered_file_swapped_for_a_symlink_is_not_served() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("v.mp4");
    let secret = dir.path().join("secret.txt");
    std::fs::write(&file, b"video").unwrap();
    std::fs::write(&secret, b"secret").unwrap();
    let server = MediaServer::start().await.unwrap();
    let port = server.port();
    let url = server.register(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&secret, &file).unwrap();
    let path = url.strip_prefix(&format!("http://127.0.0.1:{port}")).unwrap().to_owned();
    let (status, _, body) = tokio::task::spawn_blocking(move || get(port, &path, "")).await.unwrap();
    assert_eq!(status, 404);
    assert!(body.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn binds_loopback_only() {
    let server = MediaServer::start().await.unwrap();
    let port = server.port();
    let lan = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|_| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip());
    if let Ok(ip) = lan
        && !ip.is_loopback()
        && !ip.is_unspecified()
    {
        assert!(TcpStream::connect((ip, port)).is_err(), "must not be reachable on {ip}");
    }
}
