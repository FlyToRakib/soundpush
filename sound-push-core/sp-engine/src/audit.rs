//! Local audit log of security-relevant events (plan §21.1 "Repudiation", §28.2).
//!
//! Pairing attempts and results, trust and permission changes, route approvals and denials,
//! route starts and stops, and connections refused from blocked devices. Stored as JSON Lines
//! in `<data dir>/audit.log`, appended as events happen. Local only: never uploaded, and not
//! part of anything exported automatically.
//!
//! Bounded: the newest [`MAX_ENTRIES`] entries of the last [`RETENTION_SECS`]. Older entries are
//! dropped on load and when the file is compacted. Entries hold device names, short device codes
//! and, for pairing attempts, the remote address; never keys, pairing codes or secrets.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sp_security::PermissionKind;
use sp_security::secretbox::write_atomic;
use tracing::warn;

use crate::state::RouteKind;

pub const MAX_ENTRIES: usize = 1000;
/// 30 days (plan §28.2).
pub const RETENTION_SECS: u64 = 30 * 24 * 3600;
const FILE: &str = "audit.log";
/// Names come from other devices: keep only this many characters.
const MAX_FIELD_CHARS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditKind {
    /// A device that is not paired connected to pair (`detail`: its address).
    PairingAttempt,
    PairingSucceeded,
    /// `detail`: `proof` (wrong or expired QR code), `closed` (pairing mode was not open),
    /// `local` (refused on this device) or `peer` (refused on the other device).
    PairingRejected,
    /// Too many pairing attempts from one address (`detail`: the address).
    PairingRateLimited,
    DeviceForgotten,
    DeviceBlocked,
    DeviceUnblocked,
    /// `detail`: `<permission>=<policy>`, e.g. `useMyMicrophone=deny`.
    PermissionChanged,
    /// A route request was allowed, by the user or by a stored permission.
    RouteApproved,
    /// A route request was refused, by the user or by a stored permission.
    RouteDenied,
    RouteStarted,
    /// `detail`: the stop reason.
    RouteStopped,
    /// A blocked device, or a paired device presenting another key, tried to connect
    /// (`detail`: `blocked` or `keyChanged`).
    ConnectionRefused,
    /// The user cleared the log; always the first entry afterwards.
    LogCleared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub time_unix: u64,
    pub kind: AuditKind,
    /// Device name at the time. For devices that are not paired, an untrusted label.
    #[serde(default)]
    pub peer_name: String,
    /// Short device code (`SP-XXXX-XXXX-XXXX`); empty when no device is involved.
    #[serde(default)]
    pub peer_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<RouteKind>,
    #[serde(default)]
    pub detail: String,
}

impl AuditEntry {
    pub fn new(time_unix: u64, kind: AuditKind) -> Self {
        Self {
            time_unix,
            kind,
            peer_name: String::new(),
            peer_code: String::new(),
            route: None,
            detail: String::new(),
        }
    }

    #[must_use]
    pub fn peer(mut self, name: &str, code: String) -> Self {
        self.peer_name = name.to_string();
        self.peer_code = code;
        self
    }

    #[must_use]
    pub fn route(mut self, kind: RouteKind) -> Self {
        self.route = Some(kind);
        self
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

/// Stable names used in `PermissionChanged` details (the same strings the apps use).
pub fn permission_name(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::ReceiveMyAudio => "receiveMyAudio",
        PermissionKind::UseMyMicrophone => "useMyMicrophone",
        PermissionKind::SendAudioToMe => "sendAudioToMe",
        PermissionKind::ControlMe => "controlMe",
    }
}

/// Policy names used in `PermissionChanged` details.
pub fn policy_name(policy: sp_security::Policy) -> &'static str {
    match policy {
        sp_security::Policy::Allow => "allow",
        sp_security::Policy::Ask => "ask",
        sp_security::Policy::Deny => "deny",
    }
}

pub struct AuditLog {
    path: PathBuf,
    entries: VecDeque<AuditEntry>,
    max_entries: usize,
    /// Lines in the file. The file is compacted when it holds twice the entry limit.
    lines_on_disk: usize,
}

impl AuditLog {
    /// Load the log from `dir`, dropping entries older than the retention period.
    pub fn open(dir: &Path, now_unix: u64) -> Self {
        Self::open_with_limit(dir, now_unix, MAX_ENTRIES)
    }

    fn open_with_limit(dir: &Path, now_unix: u64, max_entries: usize) -> Self {
        let path = dir.join(FILE);
        let bytes = fs::read(&path).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        let cutoff = now_unix.saturating_sub(RETENTION_SECS);
        let mut entries = VecDeque::new();
        let mut lines = 0;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            lines += 1;
            // Damaged lines, and kinds written by a newer version, are skipped.
            let Ok(entry) = serde_json::from_str::<AuditEntry>(line) else {
                continue;
            };
            if entry.time_unix >= cutoff {
                entries.push_back(entry);
                if entries.len() > max_entries {
                    entries.pop_front();
                }
            }
        }
        let mut log = Self {
            path,
            entries,
            max_entries,
            lines_on_disk: lines,
        };
        if lines != log.entries.len() {
            log.compact();
        }
        log
    }

    pub fn record(&mut self, mut entry: AuditEntry) {
        entry.peer_name = clip(&entry.peer_name);
        entry.detail = clip(&entry.detail);
        let cutoff = entry.time_unix.saturating_sub(RETENTION_SECS);
        while self.entries.front().is_some_and(|e| e.time_unix < cutoff) {
            self.entries.pop_front();
        }
        let Ok(line) = serde_json::to_string(&entry) else {
            return;
        };
        self.entries.push_back(entry);
        while self.entries.len() > self.max_entries {
            self.entries.pop_front();
        }
        if self.lines_on_disk + 1 >= 2 * self.max_entries {
            return self.compact();
        }
        let appended = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
        match appended {
            Ok(()) => self.lines_on_disk += 1,
            Err(e) => warn!(error = %e, "could not write the audit log"),
        }
    }

    /// Entries, newest first.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.iter().rev().cloned().collect()
    }

    /// Delete every entry. A `LogCleared` entry records that it happened.
    pub fn clear(&mut self, now_unix: u64) -> std::io::Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        self.entries.clear();
        self.lines_on_disk = 0;
        self.record(AuditEntry::new(now_unix, AuditKind::LogCleared));
        Ok(())
    }

    fn compact(&mut self) {
        let mut text = String::new();
        for line in self
            .entries
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
        {
            text.push_str(&line);
            text.push('\n');
        }
        match write_atomic(&self.path, text.as_bytes()) {
            Ok(()) => self.lines_on_disk = self.entries.len(),
            Err(e) => warn!(error = %e, "could not compact the audit log"),
        }
    }
}

fn clip(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_FIELD_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn entry(t: u64, kind: AuditKind) -> AuditEntry {
        AuditEntry::new(t, kind).peer("Phone", "SP-1234-5678-9ABC".into())
    }

    #[test]
    fn records_persist_and_reload_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = AuditLog::open(dir.path(), NOW);
        log.record(entry(NOW, AuditKind::PairingSucceeded));
        log.record(entry(NOW + 1, AuditKind::RouteStarted).route(RouteKind::SendMicToVirtualMic));
        log.record(entry(NOW + 2, AuditKind::PermissionChanged).detail(format!(
            "{}=deny",
            permission_name(PermissionKind::UseMyMicrophone)
        )));

        let reloaded = AuditLog::open(dir.path(), NOW + 3).entries();
        assert_eq!(reloaded.len(), 3);
        assert_eq!(reloaded[0].kind, AuditKind::PermissionChanged);
        assert_eq!(reloaded[0].detail, "useMyMicrophone=deny");
        assert_eq!(reloaded[1].route, Some(RouteKind::SendMicToVirtualMic));
        let json = fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(json.contains(r#""kind":"routeStarted""#), "{json}");
    }

    #[test]
    fn bounded_by_count_and_age_and_skips_damaged_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let old =
            serde_json::to_string(&entry(NOW - RETENTION_SECS - 10, AuditKind::DeviceBlocked))
                .unwrap();
        let fresh = serde_json::to_string(&entry(NOW - 10, AuditKind::DeviceUnblocked)).unwrap();
        fs::write(
            &path,
            format!("{old}\nnot json\n{{\"timeUnix\":1,\"kind\":\"fromTheFuture\"}}\n{fresh}\n"),
        )
        .unwrap();
        let mut log = AuditLog::open_with_limit(dir.path(), NOW, 10);
        assert_eq!(log.entries().len(), 1, "old and damaged lines dropped");
        assert_eq!(
            fs::read_to_string(&path).unwrap().lines().count(),
            1,
            "compacted on load"
        );

        for i in 0..35 {
            log.record(entry(NOW + i, AuditKind::RouteStopped).detail("x".repeat(500)));
        }
        let entries = log.entries();
        assert_eq!(entries.len(), 10);
        assert_eq!(entries[0].time_unix, NOW + 34);
        assert_eq!(entries[0].detail.chars().count(), MAX_FIELD_CHARS);
        assert!(fs::read_to_string(&path).unwrap().lines().count() < 20);
        assert_eq!(
            AuditLog::open_with_limit(dir.path(), NOW + 40, 10).entries(),
            entries
        );
    }

    #[test]
    fn clearing_leaves_a_trace() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = AuditLog::open(dir.path(), NOW);
        log.record(entry(NOW, AuditKind::PairingAttempt).detail("192.168.1.9"));
        log.clear(NOW + 5).unwrap();
        let entries = AuditLog::open(dir.path(), NOW + 6).entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, AuditKind::LogCleared);
        // Clearing an empty log works too.
        let other = tempfile::tempdir().unwrap();
        assert!(AuditLog::open(other.path(), NOW).clear(NOW).is_ok());
    }
}
