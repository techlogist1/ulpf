// What the served UI offers as a file.
//
// Two links in the web UI hand the user a file: Integrity's attestation
// (`<a href="/api/integrity/attestation" target="_blank">`) and Live's export
// (`<a download href="/api/export?…" target="_blank">`). A browser saves both; the webview
// saves neither -- an anchor download is dropped and a `target="_blank"` navigation opens
// nothing, and neither reaches a webview hook to be rescued from -- so in the app both
// buttons were inert. The interceptor `lib.rs` injects turns such a click into a navigation
// on `SAVE_SCHEME`, which lands here: one GET over loopback, written where a browser would
// write it, under the name the server asked for. The web UI is not touched, so the app and
// the browser show the same page.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager, Url};

/// Fetches `url` and writes it into the user's downloads directory, then says so in the
/// shell's notice. On its own thread: the caller is a webview callback.
pub(crate) fn save(app: &AppHandle, url: &Url) {
    let (app, url) = (app.clone(), url.clone());
    thread::spawn(move || {
        let dir = match app.path().download_dir() {
            Ok(d) => d,
            Err(e) => return crate::toast(&app, &format!("Cannot find the downloads folder: {e}")),
        };
        let Some(addr) = url.socket_addrs(|| None).ok().and_then(|a| a.into_iter().next()) else {
            return crate::toast(&app, &format!("Cannot reach {url}"));
        };
        let path = match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().to_string(),
        };
        let written = get(addr, &path).and_then(|(offered, body)| {
            let name = name_for(&url, offered.as_deref());
            let bytes = body.len();
            fs::write(dir.join(&name), &body).map(|()| (name, bytes)).map_err(|e| e.to_string())
        });
        // `~/Downloads` rather than the absolute path: it is where the user will look.
        let shown = app
            .path()
            .home_dir()
            .ok()
            .and_then(|h| dir.strip_prefix(h).ok().map(|rest| format!("~/{}", rest.display())))
            .unwrap_or_else(|| dir.display().to_string());
        match written {
            Ok((name, bytes)) => crate::toast(&app, &format!("Saved {name} ({bytes} bytes) to {shown}")),
            Err(e) => crate::toast(&app, &format!("Could not save that file: {e}")),
        }
    });
}

/// One GET over loopback: the filename the server asked for in `Content-Disposition`, and
/// the body bytes. The server honours `Connection: close`, so the body is everything after
/// the header, de-chunked when it streamed the answer.
// ponytail: hand-rolled HTTP/1.1 (status line, one header, identity or chunked body);
// switch to ureq if this ever needs redirects, keep-alive or content encodings.
pub(crate) fn get(addr: SocketAddr, path: &str) -> Result<(Option<String>, Vec<u8>), String> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or("the server sent no headers")?;
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.lines();
    let status = lines.next().unwrap_or_default();
    if !status.starts_with("HTTP/1.1 200") {
        return Err(status.to_string());
    }
    let mut name = None;
    let mut chunked = false;
    for line in lines {
        name = name.or_else(|| offered_name(line));
        chunked |= line.to_ascii_lowercase().starts_with("transfer-encoding") && line.to_ascii_lowercase().contains("chunked");
    }
    let body = &buf[end + 4..];
    Ok((name, if chunked { dechunk(body)? } else { body.to_vec() }))
}

/// `Transfer-Encoding: chunked` unwrapped: the export streams from the output file, so its
/// body arrives as size lines and blocks and the size lines are not part of the file.
fn dechunk(mut body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(body.len());
    loop {
        let eol = body.windows(2).position(|w| w == b"\r\n").ok_or("a chunk without a size line")?;
        let head = String::from_utf8_lossy(&body[..eol]);
        // A chunk extension (`1f4;name=value`) is allowed after the size and is not part of it.
        let size = usize::from_str_radix(head.split(';').next().unwrap_or_default().trim(), 16).map_err(|e| format!("chunk size {head:?}: {e}"))?;
        if size == 0 {
            return Ok(out);
        }
        let (start, stop) = (eol + 2, eol + 2 + size);
        if stop > body.len() {
            return Err(format!("a chunk of {size} bytes with only {} to read", body.len() - start.min(body.len())));
        }
        out.extend_from_slice(&body[start..stop]);
        body = &body[(stop + 2).min(body.len())..];
    }
}

/// `content-disposition: attachment; filename="out-first-last.jsonl"` -> the quoted name.
fn offered_name(line: &str) -> Option<String> {
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("content-disposition") {
        return None;
    }
    let at = value.to_ascii_lowercase().find("filename=")? + "filename=".len();
    Some(value[at..].trim().trim_matches('"').to_string())
}

/// The name to write under: what the server asked for, else the URL's last path segment
/// (`/api/integrity/attestation` -> `attestation.json`, since these endpoints answer JSON
/// when they name no file). A header and a URL are both input, so only the file-name
/// component of either is taken and nothing can be written outside the downloads folder.
fn name_for(url: &Url, offered: Option<&str>) -> String {
    let raw = offered.map_or_else(|| url.path().rsplit('/').next().unwrap_or("").to_string(), str::to_string);
    let base = Path::new(raw.trim()).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if base.is_empty() || base.starts_with('.') {
        return "ulpf-download.json".to_string();
    }
    if Path::new(&base).extension().is_none() {
        return format!("{base}.json");
    }
    base
}

#[cfg(test)]
mod tests {
    use tauri::Url;

    fn url(s: &str) -> Url {
        s.parse().unwrap()
    }

    #[test]
    fn the_server_names_the_file_and_the_url_is_the_fallback() {
        // What the export answers with, header case as the server writes it and as it does not.
        assert_eq!(super::offered_name("content-disposition: attachment; filename=\"out-first-last.jsonl\""), Some("out-first-last.jsonl".into()));
        assert_eq!(super::offered_name("Content-Disposition: attachment; filename=\"out-1-9.csv\""), Some("out-1-9.csv".into()));
        assert_eq!(super::offered_name("content-length: 599"), None);
        assert_eq!(super::offered_name("content-disposition: inline"), None);

        let export = url("http://127.0.0.1:7916/api/export?format=csv&from=1");
        assert_eq!(super::name_for(&export, Some("out-first-last.jsonl")), "out-first-last.jsonl");
        // The attestation names no file, so the last segment does, with the extension it answers in.
        assert_eq!(super::name_for(&url("http://127.0.0.1:7916/api/integrity/attestation"), None), "attestation.json");
        // Only the file-name component of either, so a name from the wire cannot escape the folder.
        assert_eq!(super::name_for(&export, Some("../../../etc/passwd")), "passwd.json");
        assert_eq!(super::name_for(&export, Some("/tmp/out.jsonl")), "out.jsonl");
        assert_eq!(super::name_for(&url("http://127.0.0.1:7916/"), None), "ulpf-download.json");
        assert_eq!(super::name_for(&export, Some("  ")), "ulpf-download.json");
        assert_eq!(super::name_for(&export, Some(".bashrc")), "ulpf-download.json");
    }

    #[test]
    fn a_streamed_body_loses_its_chunk_lines_and_nothing_else() {
        // What the export answered with (measured: the first size line was 7DCE), including
        // a chunk extension, which is legal and is not part of the size.
        assert_eq!(super::dechunk(b"4\r\nabcd\r\n3;x=y\r\nefg\r\n0\r\n\r\n").unwrap(), b"abcdefg");
        assert_eq!(super::dechunk(b"0\r\n\r\n").unwrap(), b"");
        // A line the writer was mid-way through: refused, never returned half.
        assert!(super::dechunk(b"9\r\nab\r\n").is_err());
        assert!(super::dechunk(b"not-hex\r\nab\r\n").is_err());
        assert!(super::dechunk(b"4").is_err());
    }
}
