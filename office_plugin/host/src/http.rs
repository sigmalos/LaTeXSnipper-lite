//! Loopback HTTPS transport: one request per connection.
//!
//! Only `127.0.0.1` is bound and only `localhost` / `127.0.0.1` Host headers
//! are answered. `/api` calls must be same-origin and carry
//! `X-LaTeXSnipper-Office: 1`, which a cross-site page cannot send without a
//! CORS preflight this server never grants.

use crate::api::{content_type, ApiError, Host};
use crate::paths::API_HEADER;
use anyhow::{bail, Context, Result};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_BODY: usize = 4 * 1024 * 1024;
const MAX_HEAD: usize = 32 * 1024;

#[derive(Debug, Default, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub cache: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            cache: "no-store",
            body: serde_json::to_vec(value).unwrap_or_default(),
        }
    }

    fn error(e: &ApiError) -> Self {
        Self::json(e.status, &e.body())
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    }
}

fn parse_body(raw: &[u8]) -> Result<Value, ApiError> {
    if raw.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    let value: Value = std::str::from_utf8(raw)
        .ok()
        .and_then(|s| serde_json::from_str(s).ok())
        .ok_or_else(|| ApiError::new(400, "invalid_request", "请求不是有效的 JSON。"))?;
    if !value.is_object() {
        return Err(ApiError::new(
            400,
            "invalid_request",
            "请求必须是 JSON 对象。",
        ));
    }
    Ok(value)
}

pub fn route(req: &Request, port: u16, host: &Host) -> Response {
    let hosts = [format!("localhost:{port}"), format!("127.0.0.1:{port}")];
    if !req
        .header("host")
        .is_some_and(|h| hosts.iter().any(|ok| ok == h))
    {
        return Response::error(&ApiError::new(421, "misdirected", "请求主机无效。"));
    }
    if req.method == "OPTIONS" {
        return Response::error(&ApiError::new(403, "forbidden", "不允许跨源访问。"));
    }
    let path = req.path.split('?').next().unwrap_or_default();
    if path.starts_with("/api/") {
        if req.header(API_HEADER) != Some("1") {
            return Response::error(&ApiError::new(
                403,
                "forbidden",
                "缺少 Office 加载项请求标记。",
            ));
        }
        let origins = [
            format!("https://localhost:{port}"),
            format!("https://127.0.0.1:{port}"),
        ];
        if req
            .header("origin")
            .is_some_and(|o| !origins.iter().any(|ok| ok == o))
        {
            return Response::error(&ApiError::new(403, "forbidden", "不允许跨源访问。"));
        }
        return match host.handle(&req.method, path, parse_body(&req.body)) {
            Ok(value) => Response::json(200, &value),
            Err(e) => Response::error(&e),
        };
    }
    if req.method != "GET" && req.method != "HEAD" {
        return Response::error(&ApiError::new(
            405,
            "method_not_allowed",
            "不支持的请求方法。",
        ));
    }
    match host
        .assets
        .static_target(path)
        .filter(|p| p.is_file())
        .and_then(|p| std::fs::read(&p).ok().map(|b| (p, b)))
    {
        Some((file, bytes)) => Response {
            status: 200,
            content_type: content_type(&file),
            cache: "no-cache",
            body: bytes,
        },
        None => Response::error(&ApiError::new(404, "not_found", "文件不存在。")),
    }
}

pub enum ReadError {
    TooLarge,
    Other(anyhow::Error),
}

/// Parse one HTTP/1.1 request with a Content-Length body.
pub fn read_request(stream: &mut impl Read) -> Result<Request, ReadError> {
    let other = |e: anyhow::Error| ReadError::Other(e);
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(other(anyhow::anyhow!("request head too large")));
        }
        let n = stream.read(&mut chunk).map_err(|e| other(e.into()))?;
        if n == 0 {
            return Err(other(anyhow::anyhow!("connection closed")));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|e| other(e.into()))?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let method = first.next().unwrap_or_default().to_string();
    let path = first
        .next()
        .filter(|p| p.starts_with('/'))
        .ok_or_else(|| other(anyhow::anyhow!("bad request line")))?
        .to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let mut req = Request {
        method,
        path,
        headers,
        body: Vec::new(),
    };
    if req
        .header("transfer-encoding")
        .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
    {
        return Err(other(anyhow::anyhow!(
            "chunked request bodies are not supported"
        )));
    }
    let len: usize = match req.header("content-length") {
        None => 0,
        Some(v) => v
            .parse()
            .map_err(|_| other(anyhow::anyhow!("bad Content-Length")))?,
    };
    if len > MAX_BODY {
        return Err(ReadError::TooLarge);
    }
    let mut body = buf.split_off(head_end + 4);
    body.truncate(len);
    if body.len() < len {
        let start = body.len();
        body.resize(len, 0);
        stream
            .read_exact(&mut body[start..])
            .map_err(|e| other(e.into()))?;
    }
    req.body = body;
    Ok(req)
}

pub fn write_response(
    out: &mut impl Write,
    res: &Response,
    head_only: bool,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        res.status,
        reason(res.status),
        res.content_type,
        res.body.len(),
        res.cache,
    );
    out.write_all(head.as_bytes())?;
    if !head_only {
        out.write_all(&res.body)?;
    }
    out.flush()
}

fn serve(tcp: TcpStream, tls: Arc<ServerConfig>, port: u16, host: &Host) -> Result<()> {
    // Recognition and Word scripts can take a while; reads must not hang forever.
    tcp.set_read_timeout(Some(Duration::from_secs(60)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(60)))?;
    let mut stream = StreamOwned::new(ServerConnection::new(tls)?, tcp);
    let (res, head_only) = match read_request(&mut stream) {
        Ok(req) => (route(&req, port, host), req.method == "HEAD"),
        Err(ReadError::TooLarge) => (
            Response::error(&ApiError::new(413, "payload_too_large", "请求内容过大。")),
            false,
        ),
        Err(ReadError::Other(e)) => return Err(e),
    };
    write_response(&mut stream, &res, head_only)?;
    stream.conn.send_close_notify();
    let _ = stream.flush();
    Ok(())
}

/// Accept connections until the process exits.
pub fn spawn(listener: TcpListener, tls: Arc<ServerConfig>, port: u16, host: Arc<Host>) {
    std::thread::spawn(move || {
        for tcp in listener.incoming().flatten() {
            let (tls, host) = (tls.clone(), host.clone());
            std::thread::spawn(move || {
                // Probes and closed tabs end handshakes early; that is routine.
                let _ = serve(tcp, tls, port, &host);
            });
        }
    });
}

pub fn bind(port: u16) -> Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("PORT_IN_USE:无法监听 127.0.0.1:{port}，端口可能已被占用。"))
}

/// `GET /api/health` over HTTPS, trusting exactly `cert` (used by `--status`).
pub fn health(cert: rustls::pki_types::CertificateDer<'static>, port: u16) -> Result<bool> {
    let (status, body) = get(cert, port, "/api/health")?;
    Ok(status == 200 && body.contains("\"ok\":true"))
}

/// A GET over HTTPS to the local host, trusting exactly `cert`: (status, body).
pub fn get(
    cert: rustls::pki_types::CertificateDer<'static>,
    port: u16,
    path: &str,
) -> Result<(u16, String)> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert)?;
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let tcp = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2))?;
    tcp.set_read_timeout(Some(Duration::from_secs(3)))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), "localhost".try_into()?)?;
    let mut stream = StreamOwned::new(conn, tcp);
    write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\n{API_HEADER}: 1\r\nConnection: close\r\n\r\n")?;
    let mut response = Vec::new();
    match stream.read_to_end(&mut response) {
        Ok(_) => {}
        // Some peers close without close_notify; the response is complete anyway.
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {}
        Err(e) => bail!(e),
    }
    let text = String::from_utf8_lossy(&response);
    let status = text.get(9..12).and_then(|s| s.parse().ok()).unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok((status, body))
}
