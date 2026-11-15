use std::io;
use std::path::{Path, PathBuf};

use bytes::Bytes;
use eyre::{WrapErr, eyre};
use http::{HeaderName, HeaderValue, StatusCode};
use sha2::{Digest, Sha256};
use url::Url;

/// Get the `{host}/request.{hash}` path of a request, relative to the directory holding responses.
///
/// I don't want to hit filename length limits, or encode a huge URL with query parameters in the
/// filename. So hash the request inputs, and use the hash as the filename.
pub(crate) fn relative_path(url: &Url, key: &str) -> eyre::Result<PathBuf> {
    let host = url.host_str().ok_or_else(|| eyre!("{url} has no host"))?;
    Ok(PathBuf::from(encode_host(host)).join(format!("request.{}", hash(key))))
}

/// Read the request key and the response from the file at `path`.
///
/// Return `None` if there is no file.
pub(crate) fn read(path: &Path) -> eyre::Result<Option<(String, http::Response<Bytes>)>> {
    let file = match std::fs::read(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("Failed to read {}", path.display()));
        }
    };
    let (key, response) =
        decode(&file).wrap_err_with(|| format!("Failed to read {}", path.display()))?;
    Ok(Some((key.to_owned(), response)))
}

/// Write a response to the file at `path`, creating its directory if it is missing.
pub(crate) fn write(path: &Path, key: &str, response: &http::Response<Bytes>) -> eyre::Result<()> {
    let write = || -> eyre::Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| eyre!("A response file has a parent"))?;
        std::fs::create_dir_all(dir)?;
        std::fs::write(path, encode(key, response))?;
        Ok(())
    };
    write().wrap_err_with(|| format!("Failed to write {}", path.display()))
}

/// Encode the host to a legal directory name
fn encode_host(host: &str) -> String {
    let mut encoded = String::with_capacity(host.len());
    for byte in host.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Enough of the SHA-256 of a request key to distinguish the requests one endpoint answers.
fn hash(key: &str) -> String {
    Sha256::digest(key.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Encode the file format, which is text so that any drift shows up as a readable diff
///
/// ```text
/// URL
///
/// STATUS
/// HEADERS
/// ...
///
/// RESPONSE
/// ```
fn encode(key: &str, response: &http::Response<Bytes>) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(key.as_bytes());
    file.extend_from_slice(b"\n\n");
    file.extend_from_slice(response.status().as_str().as_bytes());
    file.push(b'\n');
    for (name, value) in response.headers() {
        file.extend_from_slice(name.as_str().as_bytes());
        file.extend_from_slice(b": ");
        file.extend_from_slice(value.as_bytes());
        file.push(b'\n');
    }
    file.push(b'\n');
    file.extend_from_slice(response.body());
    file
}

fn decode(file: &[u8]) -> eyre::Result<(&str, http::Response<Bytes>)> {
    let (request, rest) = split_at_blank_line(file).ok_or_else(|| eyre!("No request block"))?;
    let request = std::str::from_utf8(request).wrap_err("The request block is not UTF-8")?;
    let (head, body) = split_at_blank_line(rest).ok_or_else(|| eyre!("No response headers"))?;

    let mut lines = head.split(|&byte| byte == b'\n');
    let status = lines.next().ok_or_else(|| eyre!("No status"))?;
    let mut response = http::Response::new(Bytes::copy_from_slice(body));
    *response.status_mut() = StatusCode::from_bytes(status).wrap_err("Bad status")?;
    for line in lines {
        let (name, value) = split_once(line, b':').ok_or_else(|| eyre!("Bad header: {line:?}"))?;
        let name = HeaderName::from_bytes(name).wrap_err("Bad header name")?;
        let value = HeaderValue::from_bytes(value.strip_prefix(b" ").unwrap_or(value))
            .wrap_err_with(|| format!("Bad value of the {name} header"))?;
        response.headers_mut().append(name, value);
    }
    Ok((request, response))
}

/// Splits at the first empty line, discarding it. `None` when there isn't one.
fn split_at_blank_line(file: &[u8]) -> Option<(&[u8], &[u8])> {
    let blank = file.windows(2).position(|pair| pair == b"\n\n")?;
    Some((&file[..blank], &file[blank + 2..]))
}

fn split_once(line: &[u8], separator: u8) -> Option<(&[u8], &[u8])> {
    let at = line.iter().position(|&byte| byte == separator)?;
    Some((&line[..at], &line[at + 1..]))
}

#[cfg(test)]
mod tests {
    use http::HeaderMap;

    use super::*;
    use crate::transport::request_key;

    /// If the hashing doesn't include all the inputs, that's a really painful bug.
    /// And it also potentially leads to fixture files that don't get overwritten when a new
    /// response is recorded.
    #[test]
    fn paths_are_the_host_and_the_hash_of_the_request_key() {
        let search = Url::parse("https://openlibrary.org/search.json?title=rondos").unwrap();
        let path =
            |headers: &HeaderMap| relative_path(&search, &request_key(&search, headers)).unwrap();
        let mut json = HeaderMap::new();
        json.insert(http::header::ACCEPT, HeaderValue::from_static("text/json"));

        let bare = path(&HeaderMap::new());

        assert_eq!(bare, Path::new("openlibrary.org/request.687f805feb938cfc"));
        assert_ne!(path(&json), bare);
    }
}
