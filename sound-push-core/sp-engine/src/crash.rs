//! Local crash reports for Rust panics (plan §28.2). Nothing is ever uploaded.
//!
//! [`install`] adds a panic hook (keeping the previous one) that writes a short, redacted report
//! into `<data dir>/crash-reports`. The engine surfaces reports once on the next start
//! ([`take_unseen`]) and diagnostics exports include them ([`recent`]).
//!
//! Redaction: IP addresses, long hexadecimal strings (keys, fingerprints, device ids beyond an
//! 8-character prefix) and the user's home directory are removed from the message. Reports never
//! contain audio, keys or pairing secrets because panic messages do not carry them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const DIR: &str = "crash-reports";
const SEEN_MARKER: &str = "last-seen";
/// Reports kept on disk; older ones are deleted.
const KEEP: usize = 10;
const MAX_MESSAGE: usize = 4096;
const MAX_BACKTRACE: usize = 16 * 1024;

static INSTALLED: OnceLock<PathBuf> = OnceLock::new();

/// Install the panic hook once per process. Later calls are ignored.
pub fn install(data_dir: &Path, app_version: &str) {
    let dir = data_dir.join(DIR);
    if INSTALLED.set(dir.clone()).is_err() {
        return;
    }
    let version = app_version.to_string();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Never let reporting itself fail loudly: a panic inside a panic hook aborts.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            write_report(&dir, &version, info)
        }));
        previous(info);
    }));
}

fn write_report(dir: &Path, version: &str, info: &std::panic::PanicHookInfo<'_>) {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string());
    let location = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_default();
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let report = format!(
        "SoundPush crash report (stored locally, never uploaded)\nVersion: {version}\nOS: {} {}\nTime (unix): {now}\nThread: {thread}\nLocation: {location}\nMessage: {}\n\nBacktrace:\n{}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        redact(&truncate(&message, MAX_MESSAGE)),
        redact(&truncate(&backtrace, MAX_BACKTRACE)),
    );
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let name = format!("panic-{now:012}-{:08}.txt", std::process::id());
    let _ = fs::write(dir.join(name), report);
    prune(dir);
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn reports(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("panic-") && n.ends_with(".txt"))
                .collect()
        })
        .unwrap_or_default();
    // Names embed a zero-padded timestamp, so they sort chronologically.
    names.sort();
    names
}

fn prune(dir: &Path) {
    let names = reports(dir);
    for old in names.iter().take(names.len().saturating_sub(KEEP)) {
        let _ = fs::remove_file(dir.join(old));
    }
}

/// Number of reports written since the last call, marking them seen.
pub fn take_unseen(data_dir: &Path) -> usize {
    let dir = data_dir.join(DIR);
    let names = reports(&dir);
    let Some(newest) = names.last() else {
        return 0;
    };
    let seen = fs::read_to_string(dir.join(SEEN_MARKER)).unwrap_or_default();
    let unseen = names.iter().filter(|n| n.as_str() > seen.trim()).count();
    if unseen > 0 {
        let _ = fs::write(dir.join(SEEN_MARKER), newest);
    }
    unseen
}

/// The newest reports, newest first, as (file name, contents).
pub fn recent(data_dir: &Path, max: usize) -> Vec<(String, String)> {
    let dir = data_dir.join(DIR);
    reports(&dir)
        .into_iter()
        .rev()
        .take(max)
        .filter_map(|name| fs::read_to_string(dir.join(&name)).ok().map(|c| (name, c)))
        .collect()
}

/// Remove addresses, long hex identifiers and the home directory from free text.
pub fn redact(text: &str) -> String {
    let mut text = text.to_string();
    for var in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(var) {
            if home.len() > 3 {
                text = text.replace(&home, "~");
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    for c in text.chars() {
        if c.is_ascii_hexdigit() || c == '.' || c == ':' {
            token.push(c);
        } else {
            flush_token(&mut token, &mut out);
            out.push(c);
        }
    }
    flush_token(&mut token, &mut out);
    out
}

fn flush_token(token: &mut String, out: &mut String) {
    if token.is_empty() {
        return;
    }
    let t = token.trim_matches(|c| c == '.' || c == ':');
    let lead = &token[..token.find(t).unwrap_or(0)];
    let trail = &token[lead.len() + t.len()..];
    let colons = t.matches(':').count();
    // "a.b.c.d" or "a.b.c.d:port".
    let host = if colons == 1 {
        t.split(':').next().unwrap_or(t)
    } else {
        t
    };
    let is_ipv4 = {
        let parts: Vec<&str> = host.split('.').collect();
        parts.len() == 4
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
    };
    let is_ipv6 = colons >= 2
        && t.chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        && t.len() >= 6;
    let hex_run = t.len() >= 16 && t.chars().all(|c| c.is_ascii_hexdigit());
    out.push_str(lead);
    if is_ipv4 || is_ipv6 {
        out.push_str("<ip>");
    } else if hex_run {
        out.push_str(&t[..8]);
        out.push('…');
    } else {
        out.push_str(t);
    }
    out.push_str(trail);
    token.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_removes_addresses_and_identifiers() {
        let text = "dial 192.168.1.20:47650 failed for peer 0123456789abcdef0123456789abcdef, v6 [fe80::1:2]:5 at line 42 in 3.14";
        let r = redact(text);
        assert!(!r.contains("192.168"), "{r}");
        assert!(!r.contains("fe80"), "{r}");
        assert!(r.contains("01234567…"), "{r}");
        assert!(!r.contains("89abcdef0123"), "{r}");
        assert!(r.contains("line 42 in 3.14"), "{r}");
        assert_eq!(redact("plain words"), "plain words");
    }

    #[test]
    fn reports_are_listed_pruned_and_seen_once() {
        let dir = tempfile::tempdir().unwrap();
        let reports_dir = dir.path().join(DIR);
        fs::create_dir_all(&reports_dir).unwrap();
        assert_eq!(take_unseen(dir.path()), 0);
        for i in 0..12 {
            fs::write(
                reports_dir.join(format!("panic-{i:012}-00000001.txt")),
                format!("report {i}"),
            )
            .unwrap();
        }
        prune(&reports_dir);
        assert_eq!(reports(&reports_dir).len(), KEEP);
        assert_eq!(take_unseen(dir.path()), KEEP);
        assert_eq!(take_unseen(dir.path()), 0, "surfaced once");
        fs::write(reports_dir.join("panic-000000000099-00000001.txt"), "new").unwrap();
        assert_eq!(take_unseen(dir.path()), 1);
        let recent = recent(dir.path(), 2);
        assert_eq!(recent[0].1, "new");
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        let s = "ééééé";
        assert!(truncate(s, 3).starts_with('é'));
    }
}
