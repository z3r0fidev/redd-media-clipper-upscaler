//! In-process media server for `<video>` playback (research.md §R8).
//!
//! WebKitGTK cannot feed `<video>` from Tauri's asset protocol, so media is served over
//! `http://127.0.0.1:<port>/<token>/<id>`. Security requirements (all from the R8 audit):
//! 1. bound to 127.0.0.1 on an ephemeral port, fail closed;
//! 2. 128-bit random session token in the path, compared in constant time;
//! 3. files served only by opaque id, registered by the app for user-chosen sources — URLs never
//!    carry paths; unknown ids are 404;
//! 4. every request re-resolves the registered path, refuses symlinks (O_NOFOLLOW) and non-files,
//!    and streams from the opened handle;
//! 5. GET/HEAD only; one strict byte range; 416 otherwise; streamed in 128 KiB chunks;
//! 6. `Host` must be `127.0.0.1:<port>`, `Origin` (if present) must be the app; no CORS grants;
//! 7. fixed video content types, `nosniff`, `no-store`;
//! 8. ≤ 8 concurrent connections, header and I/O timeouts, one request per connection;
//! 9. the token is never logged.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;

const MAX_CONNECTIONS: usize = 8;
const MAX_HEADER_BYTES: usize = 8 * 1024;
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK: usize = 128 * 1024;

/// Origins the app's webview uses (production on Linux/macOS, Windows), plus the Vite dev server
/// in debug builds.
fn allowed_origin(origin: &str) -> bool {
    matches!(origin, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost")
        || (cfg!(debug_assertions) && origin == "http://localhost:1420")
}

pub struct MediaServer {
    port: u16,
    token: String,
    registry: Arc<Mutex<Registry>>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Default)]
struct Registry {
    by_id: HashMap<String, PathBuf>,
    by_path: HashMap<PathBuf, String>,
}

/// The session token is never printed (R8 #9).
impl std::fmt::Debug for MediaServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaServer")
            .field("port", &self.port)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl Drop for MediaServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MediaServer {
    pub async fn start() -> std::io::Result<MediaServer> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let token = random_hex(16)?;
        let registry = Arc::new(Mutex::new(Registry::default()));
        let ctx = Arc::new(Ctx { port, token: token.clone(), registry: registry.clone() });
        let limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { continue };
                let Ok(permit) = limit.clone().try_acquire_owned() else {
                    drop(stream); // over the connection cap: refuse
                    continue;
                };
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = handle(stream, &ctx).await;
                });
            }
        });
        Ok(MediaServer { port, token, registry, task })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Forgets every registered file: existing URLs stop working (window close).
    pub fn revoke_all(&self) {
        let mut reg = self.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.by_id.clear();
        reg.by_path.clear();
    }

    /// Registers a regular file and returns its URL. Re-registering a path returns the same URL.
    pub fn register(&self, path: &Path) -> std::io::Result<String> {
        let canonical = std::fs::canonicalize(path)?;
        if !std::fs::metadata(&canonical)?.is_file() {
            return Err(std::io::Error::other("not a regular file"));
        }
        let mut reg = self.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = match reg.by_path.get(&canonical) {
            Some(id) => id.clone(),
            None => {
                let id = random_hex(16)?;
                reg.by_id.insert(id.clone(), canonical.clone());
                reg.by_path.insert(canonical, id.clone());
                id
            }
        };
        Ok(format!("http://127.0.0.1:{}/{}/{}", self.port, self.token, id))
    }
}

struct Ctx {
    port: u16,
    token: String,
    registry: Arc<Mutex<Registry>>,
}

fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

struct Request {
    method: String,
    target: String,
    headers: HashMap<String, String>,
}

async fn read_head(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 1024];
    let read = async {
        loop {
            let n = stream.read(&mut tmp).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                return Some(());
            }
            if buf.len() > MAX_HEADER_BYTES {
                return None;
            }
        }
    };
    tokio::time::timeout(HEADER_TIMEOUT, read).await.ok()??;
    let text = std::str::from_utf8(&buf).ok()?;
    let head = text.split("\r\n\r\n").next()?;
    let mut lines = head.split("\r\n");
    let mut parts = lines.next()?.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if !version.starts_with("HTTP/1.") || parts.next().is_some() {
        return None;
    }
    let mut headers = HashMap::new();
    for line in lines {
        let (k, v) = line.split_once(':')?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
    }
    Some(Request { method: method.to_owned(), target: target.to_owned(), headers })
}

async fn respond(stream: &mut TcpStream, status: &str, extra: &[(&str, String)]) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n"
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !extra.iter().any(|(k, _)| *k == "Content-Length") {
        head.push_str("Content-Length: 0\r\n");
    }
    head.push_str("\r\n");
    tokio::time::timeout(WRITE_TIMEOUT, stream.write_all(head.as_bytes()))
        .await
        .map_err(std::io::Error::other)?
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        Some("ogv") => "video/ogg",
        _ => "application/octet-stream",
    }
}

/// Parses a single `bytes=` range against `len`. `Ok(None)` = whole file; `Err` = 416.
fn parse_range(header: Option<&String>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(h) = header else { return Ok(None) };
    let spec = h.strip_prefix("bytes=").ok_or(())?;
    if spec.contains(',') || len == 0 {
        return Err(());
    }
    let (a, b) = spec.split_once('-').ok_or(())?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => {
            let n: u64 = n.parse().map_err(|_| ())?;
            if n == 0 {
                return Err(());
            }
            (len.saturating_sub(n), len - 1)
        }
        (s, "") => (s.parse().map_err(|_| ())?, len - 1),
        (s, e) => (s.parse().map_err(|_| ())?, e.parse::<u64>().map_err(|_| ())?.min(len - 1)),
    };
    if start > end || start >= len {
        return Err(());
    }
    Ok(Some((start, end)))
}

fn open_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        opts.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    opts.open(path)
}

async fn handle(mut stream: TcpStream, ctx: &Ctx) -> std::io::Result<()> {
    let Some(req) = read_head(&mut stream).await else {
        return respond(&mut stream, "400 Bad Request", &[]).await;
    };
    // DNS-rebinding / cross-site defence.
    if req.headers.get("host").map(String::as_str) != Some(&format!("127.0.0.1:{}", ctx.port)) {
        return respond(&mut stream, "403 Forbidden", &[]).await;
    }
    if let Some(origin) = req.headers.get("origin")
        && !allowed_origin(origin)
    {
        return respond(&mut stream, "403 Forbidden", &[]).await;
    }
    let head_only = match req.method.as_str() {
        "GET" => false,
        "HEAD" => true,
        _ => return respond(&mut stream, "405 Method Not Allowed", &[("Allow", "GET, HEAD".into())]).await,
    };
    let mut parts = req.target.split('/');
    let (Some(""), Some(token), Some(id), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return respond(&mut stream, "404 Not Found", &[]).await;
    };
    if !constant_time_eq(token.as_bytes(), ctx.token.as_bytes()) {
        return respond(&mut stream, "404 Not Found", &[]).await;
    }
    let registered =
        ctx.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner).by_id.get(id).cloned();
    let Some(path) = registered else {
        return respond(&mut stream, "404 Not Found", &[]).await;
    };
    // The registered path was canonical (no symlinks). Re-check it still resolves to itself and
    // open without following links, then serve from that handle only.
    if std::fs::canonicalize(&path).ok().as_deref() != Some(path.as_path()) {
        return respond(&mut stream, "404 Not Found", &[]).await;
    }
    let file = match open_no_follow(&path).and_then(|f| f.metadata().map(|m| (f, m))) {
        Ok((f, m)) if m.is_file() => (f, m.len()),
        _ => return respond(&mut stream, "404 Not Found", &[]).await,
    };
    let (file, len) = file;
    let common = [
        ("Content-Type", content_type(&path).to_owned()),
        ("Accept-Ranges", "bytes".to_owned()),
        // <video> loads cross-origin (app origin → 127.0.0.1) in no-cors mode.
        ("Cross-Origin-Resource-Policy", "cross-origin".to_owned()),
    ];
    let (status, start, end) = match parse_range(req.headers.get("range"), len) {
        Ok(None) => ("200 OK", 0, len.saturating_sub(1)),
        Ok(Some((s, e))) => ("206 Partial Content", s, e),
        Err(()) => {
            return respond(
                &mut stream,
                "416 Range Not Satisfiable",
                &[("Content-Range", format!("bytes */{len}"))],
            )
            .await;
        }
    };
    let count = if len == 0 { 0 } else { end - start + 1 };
    let mut headers: Vec<(&str, String)> = common.to_vec();
    headers.push(("Content-Length", count.to_string()));
    if status.starts_with("206") {
        headers.push(("Content-Range", format!("bytes {start}-{end}/{len}")));
    }
    respond(&mut stream, status, &headers).await?;
    if head_only || count == 0 {
        return Ok(());
    }
    let mut file = tokio::fs::File::from_std(file);
    file.seek(SeekFrom::Start(start)).await?;
    let mut remaining = count;
    let mut buf = vec![0u8; CHUNK];
    while remaining > 0 {
        let want = remaining.min(CHUNK as u64) as usize;
        let n = file.read(&mut buf[..want]).await?;
        if n == 0 {
            break; // file shrank while streaming: end the response
        }
        tokio::time::timeout(WRITE_TIMEOUT, stream.write_all(&buf[..n]))
            .await
            .map_err(std::io::Error::other)??;
        remaining -= n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let r = |h: &str, len| parse_range(Some(&h.to_owned()), len);
        assert_eq!(parse_range(None, 10), Ok(None));
        assert_eq!(r("bytes=0-4", 10), Ok(Some((0, 4))));
        assert_eq!(r("bytes=5-", 10), Ok(Some((5, 9))));
        assert_eq!(r("bytes=-3", 10), Ok(Some((7, 9))));
        assert_eq!(r("bytes=8-100", 10), Ok(Some((8, 9))));
        for bad in ["bytes=0-1,3-4", "bytes=10-", "bytes=5-2", "items=0-1", "bytes=a-b", "bytes=-0"] {
            assert_eq!(r(bad, 10), Err(()), "{bad}");
        }
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
