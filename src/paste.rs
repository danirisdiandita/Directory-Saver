use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

/// Turns pasted text into an absolute path without touching the filesystem.
pub fn parse_pasted_path(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    let line = raw.trim().lines().next()?.trim();
    let text = strip_quotes(line);
    let path = if let Some(rest) = text.strip_prefix("file://") {
        let (host, path) = rest.split_at(rest.find('/')?);
        if !(host.is_empty() || host == "localhost") {
            return None;
        }
        PathBuf::from(OsString::from_vec(percent_decode(path)))
    } else if text == "~" {
        home?.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home?.join(rest)
    } else {
        PathBuf::from(text)
    };
    path.is_absolute().then_some(path)
}

fn strip_quotes(text: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|&q| text.strip_prefix(q)?.strip_suffix(q))
        .unwrap_or(text)
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .filter(|_| bytes[i] == b'%')
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Option<PathBuf> {
        parse_pasted_path(raw, Some(Path::new("/home/u")))
    }

    fn some(path: &str) -> Option<PathBuf> {
        Some(PathBuf::from(path))
    }

    #[test]
    fn plain_absolute_path() {
        assert_eq!(parse("/home/u/code"), some("/home/u/code"));
    }

    #[test]
    fn quoted_path_with_spaces() {
        assert_eq!(parse("\"/home/u/My Dir\""), some("/home/u/My Dir"));
        assert_eq!(parse("'/home/u/My Dir'"), some("/home/u/My Dir"));
    }

    #[test]
    fn file_uris() {
        assert_eq!(parse("file:///home/u/My%20Dir"), some("/home/u/My Dir"));
        assert_eq!(parse("file://localhost/tmp"), some("/tmp"));
        assert_eq!(parse("file://otherhost/x"), None);
    }

    #[test]
    fn tilde_expands_to_home() {
        assert_eq!(parse("~/x"), some("/home/u/x"));
        assert_eq!(parse("~"), some("/home/u"));
        assert_eq!(parse_pasted_path("~/x", None), None);
    }

    #[test]
    fn only_first_line_counts() {
        assert_eq!(parse("/tmp\n"), some("/tmp"));
        assert_eq!(parse("  /tmp/a\n/tmp/b\n"), some("/tmp/a"));
    }

    #[test]
    fn rejects_empty_and_relative() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("  \n "), None);
        assert_eq!(parse("code/project"), None);
        assert_eq!(parse("~other/x"), None);
    }
}
