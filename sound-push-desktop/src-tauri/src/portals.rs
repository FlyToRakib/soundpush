//! xdg-desktop-portal integration for sandboxed and Wayland desktops (plan §26.2, §13.2, §24).
//!
//! Inside a Flatpak, and on Wayland generally, an app cannot grab a global shortcut, ask to keep
//! running after its window closes, or stop the session going to sleep by itself. The portal does
//! all three on its behalf, with the user's consent, and is the only way the Flatpak build can
//! behave like the deb and rpm ones:
//!
//! * `org.freedesktop.portal.GlobalShortcuts` — the microphone mute and push-to-talk shortcuts.
//! * `org.freedesktop.portal.Background` — running in the tray, and launching at sign-in.
//! * `org.freedesktop.portal.Inhibit` — "Prevent sleep while streaming".
//!
//! Nothing here is required. Outside a sandbox, or when the portal or one of these interfaces is
//! missing, every entry point answers "not available" and the ordinary paths (the global-shortcut
//! plugin, `tauri-plugin-autostart`, `systemd-inhibit`) are used exactly as before.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time::Duration;

use dbus::arg::{RefArg, Variant};
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::message::MatchRule;
use tracing::{debug, info, warn};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REQUEST: &str = "org.freedesktop.portal.Request";
const GLOBAL_SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const BACKGROUND: &str = "org.freedesktop.portal.Background";
const INHIBIT: &str = "org.freedesktop.portal.Inhibit";
/// Portal calls go through a dialog the user answers, so they are given real time.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Inhibit flags: 4 = suspend, 8 = idle. Logout and user switching are not SoundPush's business.
const INHIBIT_SUSPEND_AND_IDLE: u32 = 4 | 8;

/// Options are always `a{sv}` on the wire.
type Options = HashMap<String, Variant<Box<dyn RefArg>>>;

fn option(value: impl RefArg + 'static) -> Variant<Box<dyn RefArg>> {
    Variant(Box::new(value))
}

/// Whether SoundPush runs inside a sandbox that hides the session from it (Flatpak, Snap).
pub fn sandboxed() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
        || std::env::var_os("SNAP").is_some()
        || std::env::var("container").is_ok_and(|c| !c.is_empty())
}

/// Whether this is a Wayland session, where an ordinary X11 key grab does not work.
pub fn wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Whether the portal should be used at all: inside a sandbox it is the only way, and on Wayland
/// it is the only one that works. Elsewhere the existing paths are left alone.
pub fn preferred() -> bool {
    sandboxed() || wayland()
}

/// Version of a portal interface, or `None` when the portal or the interface is absent.
fn version(connection: &Connection, interface: &str) -> Option<u32> {
    let proxy = connection.with_proxy(PORTAL, PORTAL_PATH, Duration::from_secs(2));
    proxy.get::<u32>(interface, "version").ok()
}

/// A token the portal turns into an object path. It only has to be unique within this app and
/// contain nothing but letters, digits and underscores.
fn token(prefix: &str) -> String {
    format!(
        "soundpush_{prefix}_{}_{}",
        std::process::id(),
        // A run that asks twice must not reuse a path the portal still holds.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// The object path the portal will answer a request with `handle_token` on, so the reply can be
/// watched for before the call is made and never missed.
fn request_path(connection: &Connection, handle_token: &str) -> String {
    // The portal derives it from the caller's unique bus name with dots and the leading colon
    // replaced by underscores.
    let sender = connection.unique_name().replace('.', "_");
    let sender = sender.strip_prefix(':').unwrap_or(&sender);
    format!("/org/freedesktop/portal/desktop/request/{sender}/{handle_token}")
}

/// Wait for the `Response` a portal request answers with. Returns the response code (0 = the
/// user agreed) together with the results, or `None` when nothing arrives in time.
fn wait_for_response(
    connection: &Connection,
    path: &str,
    deadline: Duration,
) -> Option<(u32, Options)> {
    let rule = MatchRule::new_signal(REQUEST, "Response").with_path(path.to_string());
    let (tx, rx) = std::sync::mpsc::channel();
    let added = connection.add_match(rule, move |(code, results): (u32, Options), _, _| {
        let _ = tx.send((code, results));
        // Once is enough; the portal destroys the request object afterwards.
        false
    });
    if let Err(e) = added {
        warn!(error = %e, "could not listen for the portal's answer");
        return None;
    }
    let until = std::time::Instant::now() + deadline;
    loop {
        if let Ok(answer) = rx.try_recv() {
            return Some(answer);
        }
        let left = until.checked_duration_since(std::time::Instant::now())?;
        if connection
            .process(left.min(Duration::from_millis(250)))
            .is_err()
        {
            return None;
        }
    }
}

// ---------------------------------------------------------------- Background and autostart

/// Ask to keep running when no window is open, and to be started at sign-in when `autostart` is
/// set (plan §24). Returns whether the portal granted autostart; the caller mirrors that into the
/// setting, exactly as the registry and `StartupApproved` state are mirrored on Windows.
///
/// `command` is what the portal writes into the autostart file. Inside a Flatpak it must be the
/// command as seen from outside the sandbox, which is what the portal expects.
pub fn request_background(reason: &str, autostart: bool, command: &[String]) -> Option<bool> {
    let connection = Connection::new_session().ok()?;
    version(&connection, BACKGROUND)?;
    let handle_token = token("background");
    let path = request_path(&connection, &handle_token);

    let mut options: Options = HashMap::new();
    options.insert("handle_token".into(), option(handle_token));
    options.insert("reason".into(), option(reason.to_string()));
    options.insert("autostart".into(), option(autostart));
    options.insert("commandline".into(), option(command.to_vec()));
    options.insert("dbus-activatable".into(), option(false));

    let proxy = connection.with_proxy(PORTAL, PORTAL_PATH, TIMEOUT);
    // An empty parent window is right for an app with no window open.
    let call: Result<(dbus::Path,), _> =
        proxy.method_call(BACKGROUND, "RequestBackground", ("", options));
    if let Err(e) = call {
        warn!(error = %e, "the portal refused the background request");
        return None;
    }
    let (code, results) = wait_for_response(&connection, &path, TIMEOUT)?;
    if code != 0 {
        debug!(code, "the user did not allow running in the background");
        return Some(false);
    }
    let granted = results
        .get("autostart")
        .and_then(|v| v.0.as_u64())
        .is_some_and(|v| v != 0);
    info!(granted, "the portal answered the background request");
    Some(granted)
}

// ------------------------------------------------------------------------------- Inhibit

/// Keeps the session awake until it is dropped (plan §24 "Prevent sleep while streaming").
pub struct PortalInhibit {
    connection: Connection,
    request: String,
}

impl PortalInhibit {
    /// Ask the portal to stop the session suspending or going idle. `None` when the portal has no
    /// Inhibit interface, so the caller falls back to `systemd-inhibit`.
    pub fn acquire(reason: &str) -> Option<Self> {
        let connection = Connection::new_session().ok()?;
        version(&connection, INHIBIT)?;
        let handle_token = token("inhibit");
        let request = request_path(&connection, &handle_token);
        let mut options: Options = HashMap::new();
        options.insert("handle_token".into(), option(handle_token));
        options.insert("reason".into(), option(reason.to_string()));
        let proxy = connection.with_proxy(PORTAL, PORTAL_PATH, TIMEOUT);
        let call: Result<(dbus::Path,), _> =
            proxy.method_call(INHIBIT, "Inhibit", ("", INHIBIT_SUSPEND_AND_IDLE, options));
        match call {
            Ok(_) => {
                info!("the portal is keeping the session awake");
                Some(Self {
                    connection,
                    request,
                })
            }
            Err(e) => {
                warn!(error = %e, "the portal would not keep the session awake");
                None
            }
        }
    }
}

impl Drop for PortalInhibit {
    fn drop(&mut self) {
        // Closing the request is what ends the inhibit; the portal also drops it when this
        // process leaves the bus, so a crash never leaves the session awake for ever.
        let proxy =
            self.connection
                .with_proxy(PORTAL, self.request.clone(), Duration::from_secs(2));
        if let Err(e) = proxy.method_call::<(), _, _, _>(REQUEST, "Close", ()) {
            debug!(error = %e, "the portal had already ended the inhibit");
        }
    }
}

// ----------------------------------------------------------------------- Global shortcuts

/// Which SoundPush shortcut the portal reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutEvent {
    MutePressed,
    PushToTalkPressed,
    PushToTalkReleased,
}

/// Portal shortcut ids. They are stored by the portal, so they must not change between releases.
const MUTE_ID: &str = "toggle-microphone-mute";
const PUSH_TO_TALK_ID: &str = "push-to-talk";

/// Bind the two shortcuts through the portal and report activations on `events` until the process
/// ends. `descriptions` are what the portal shows the user, so they come from the caller (which
/// has the translations). `triggers` are the preferred key combinations in the portal's own
/// syntax ("CTRL+SHIFT+m"); the user can change them in the system's shortcut settings, which is
/// exactly why a portal shortcut has no "already in use" error to report.
///
/// Returns false when the portal has no GlobalShortcuts interface, so the caller keeps using the
/// global-shortcut plugin.
pub fn bind_shortcuts(
    shortcuts: Vec<(&'static str, String, Option<String>)>,
    events: Sender<ShortcutEvent>,
) -> bool {
    let Ok(connection) = Connection::new_session() else {
        return false;
    };
    if version(&connection, GLOBAL_SHORTCUTS).is_none() {
        debug!("the portal has no global shortcuts; using the ordinary path");
        return false;
    }
    let started = std::thread::Builder::new()
        .name("sp-portal-shortcuts".into())
        .spawn(move || run_shortcuts(shortcuts, &events));
    match started {
        Ok(_) => true,
        Err(e) => {
            warn!(error = %e, "could not start the portal shortcut listener");
            false
        }
    }
}

fn run_shortcuts(
    shortcuts: Vec<(&'static str, String, Option<String>)>,
    events: &Sender<ShortcutEvent>,
) {
    let Ok(connection) = Connection::new_session() else {
        return;
    };
    let Some(session) = create_session(&connection) else {
        return;
    };
    if !bind(&connection, &session, shortcuts) {
        return;
    }
    // Activated and Deactivated carry the session they belong to, the shortcut id and a timestamp.
    let listen = |member: &'static str, pressed: bool| {
        let rule = MatchRule::new_signal(GLOBAL_SHORTCUTS, member);
        let session = session.clone();
        let events = events.clone();
        connection.add_match(
            rule,
            move |(from, id, _stamp, _options): (dbus::Path, String, u64, Options), _, _| {
                // A session the portal still holds from an earlier run is not this one's.
                if from.to_string() != session {
                    return true;
                }
                let event = match (id.as_str(), pressed) {
                    (MUTE_ID, true) => Some(ShortcutEvent::MutePressed),
                    (PUSH_TO_TALK_ID, true) => Some(ShortcutEvent::PushToTalkPressed),
                    (PUSH_TO_TALK_ID, false) => Some(ShortcutEvent::PushToTalkReleased),
                    _ => None,
                };
                if let Some(event) = event {
                    let _ = events.send(event);
                }
                true
            },
        )
    };
    if listen("Activated", true).is_err() || listen("Deactivated", false).is_err() {
        warn!("could not listen for portal shortcuts");
        return;
    }
    info!("global shortcuts are handled by the desktop portal");
    loop {
        if let Err(e) = connection.process(Duration::from_secs(300)) {
            warn!(error = %e, "stopped listening for portal shortcuts");
            return;
        }
    }
}

/// `CreateSession` answers with the session's object path in its response.
fn create_session(connection: &Connection) -> Option<String> {
    let handle_token = token("shortcuts");
    let path = request_path(connection, &handle_token);
    let mut options: Options = HashMap::new();
    options.insert("handle_token".into(), option(handle_token));
    options.insert("session_handle_token".into(), option(token("session")));
    let proxy = connection.with_proxy(PORTAL, PORTAL_PATH, TIMEOUT);
    let call: Result<(dbus::Path,), _> =
        proxy.method_call(GLOBAL_SHORTCUTS, "CreateSession", (options,));
    if let Err(e) = call {
        warn!(error = %e, "the portal would not open a shortcuts session");
        return None;
    }
    let (code, results) = wait_for_response(connection, &path, TIMEOUT)?;
    if code != 0 {
        debug!(code, "no shortcuts session");
        return None;
    }
    results
        .get("session_handle")
        .and_then(|v| v.0.as_str())
        .map(str::to_string)
}

/// `BindShortcuts` shows the user what SoundPush wants and remembers their answer, so this is
/// silent from the second run on.
fn bind(
    connection: &Connection,
    session: &str,
    shortcuts: Vec<(&'static str, String, Option<String>)>,
) -> bool {
    let handle_token = token("bind");
    let path = request_path(connection, &handle_token);
    let mut options: Options = HashMap::new();
    options.insert("handle_token".into(), option(handle_token));

    let wanted: Vec<(String, Options)> = shortcuts
        .into_iter()
        .map(|(id, description, trigger)| {
            let mut entry: Options = HashMap::new();
            entry.insert("description".into(), option(description));
            if let Some(trigger) = trigger {
                entry.insert("preferred_trigger".into(), option(trigger));
            }
            (id.to_string(), entry)
        })
        .collect();

    let session = dbus::Path::new(session.to_string()).ok();
    let Some(session) = session else {
        return false;
    };
    let proxy = connection.with_proxy(PORTAL, PORTAL_PATH, TIMEOUT);
    let call: Result<(dbus::Path,), _> = proxy.method_call(
        GLOBAL_SHORTCUTS,
        "BindShortcuts",
        (session, wanted, "", options),
    );
    if let Err(e) = call {
        warn!(error = %e, "the portal would not bind the shortcuts");
        return false;
    }
    match wait_for_response(connection, &path, TIMEOUT) {
        Some((0, _)) => true,
        other => {
            debug!(?other, "the user did not allow the shortcuts");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_path_matches_what_the_portal_derives() {
        // The portal builds the path from the caller's unique name, so watching for the answer
        // before making the call is safe. Only the shape can be checked without a session bus.
        let token = token("inhibit");
        assert!(
            token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{token} must be usable in an object path"
        );
        assert!(token.starts_with("soundpush_inhibit_"));
        assert_ne!(token, super::token("inhibit"), "two asks, two paths");
    }

    #[test]
    fn the_portal_is_only_preferred_where_it_is_needed() {
        // A plain X11 session outside a sandbox keeps the paths it already had.
        assert_eq!(preferred(), sandboxed() || wayland());
    }
}
