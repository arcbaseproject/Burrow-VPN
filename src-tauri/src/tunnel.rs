//! Connection state machine: drives the privileged helper, watches the tunnel
//! interface's real byte counters, and reconnects when it disappears.

use crate::store::{Session, Store};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use burrow_helper::parse;

#[cfg(windows)]
pub const IFACE: &str = "burrow";
#[cfg(not(windows))]
pub const IFACE: &str = "burrow0";

/// No packets received for this long while still sending = server unreachable.
const STALL_AFTER: Duration = Duration::from_secs(120);
const MAX_RETRIES: u32 = 5;
// Serialize helper calls and their resulting state updates.
static OPERATION: Mutex<()> = Mutex::new(());

#[derive(Serialize, Clone, Copy, PartialEq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Disconnecting,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IpInfo {
    pub ip: String,
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub org: Option<String>,
    /// "lat,lon" of the address, for the map.
    pub loc: Option<String>,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub state: State,
    pub profile_id: Option<String>,
    pub connected_at: Option<u64>,
    pub rx: u64,
    pub tx: u64,
    pub rx_rate: f64,
    pub tx_rate: f64,
    pub stalled: bool,
    pub error: Option<String>,
    pub ip: Option<IpInfo>,
    pub ip_before: Option<IpInfo>,
    pub ip_pending: bool,
}

pub struct Inner {
    pub status: Status,
    pub store: Store,
    /// User intent; false after a manual disconnect so we don't reconnect.
    want_up: bool,
    retries: u32,
    generation: u64,
    ip_request: u64,
    lookup_protected: bool,
    rx_seen: (Instant, u64, u64),
    last_sample: Option<(Instant, u64, u64)>,
}

impl Inner {
    fn begin_connect(&mut self) -> Option<u64> {
        if self.status.state != State::Disconnected { return None; }
        self.status.ip_before = self.status.ip.clone();
        self.status.ip = None;
        self.status.ip_pending = false;
        self.want_up = true;
        self.status.state = State::Connecting;
        self.retries = 0;
        self.generation += 1;
        Some(self.generation)
    }

    fn current_attempt(&self, generation: u64) -> bool {
        self.want_up && self.generation == generation
    }

    fn allows_lookup(&self, settings: &crate::store::Settings) -> bool {
        settings.ip_lookup && match self.status.state {
            State::Connected => self.lookup_protected || settings.ip_check_exposed,
            State::Disconnected => settings.ip_check_exposed,
            _ => false,
        }
    }

    fn current_lookup(&self, generation: u64, request: u64, state: State) -> bool {
        self.generation == generation && self.ip_request == request && self.status.state == state
    }

    fn disconnect_failed(&mut self, wanted_up: bool, error: String) {
        self.want_up = wanted_up;
        self.status.state = State::Connected;
        self.status.error = Some(error);
    }
}

pub type Shared = Arc<Mutex<Inner>>;

pub fn new(store: Store) -> Shared {
    Arc::new(Mutex::new(Inner {
        status: Status::default(),
        store,
        want_up: false,
        retries: 0,
        generation: 0,
        ip_request: 0,
        lookup_protected: false,
        rx_seen: (Instant::now(), 0, 0),
        last_sample: None,
    }))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn emit(app: &AppHandle, st: &Inner) {
    let _ = app.emit("status", &st.status);
}

fn shared(app: &AppHandle) -> Shared {
    app.state::<Shared>().inner().clone()
}

pub fn connect(app: &AppHandle, profile_id: String) {
    let sh = shared(app);
    let generation = {
        let mut st = sh.lock().unwrap();
        let Some(generation) = st.begin_connect() else { return; };
        emit(app, &st);
        generation
    };
    let app = app.clone();
    std::thread::spawn(move || bring_up(&app, profile_id, State::Connecting, generation));
}

fn bring_up(app: &AppHandle, profile_id: String, phase: State, generation: u64) {
    let operation = OPERATION.lock().unwrap();
    let sh = shared(app);
    let (text, settings) = {
        let mut st = sh.lock().unwrap();
        if !st.current_attempt(generation) { return; }
        st.status.state = phase;
        st.status.error = None;
        st.status.stalled = false;
        st.status.profile_id = Some(profile_id.clone());
        emit(app, &st);
        (st.store.config_text(&profile_id), st.store.settings())
    };

    let mut lookup_protected = false;
    let res = text.and_then(|t| {
        let mut cfg = parse(&t)?;
        // IPv4 defaults also receive IPv6 leak hardening in the helper.
        lookup_protected = cfg.peers.iter().any(|p| p.allowed_ips.iter().any(|c| c == "0.0.0.0/0"));
        cfg.override_dns(&settings.dns)?;
        let mut args = vec!["up"];
        if settings.kill_switch {
            args.push("--kill-switch");
            if settings.allow_lan && cfg!(target_os = "linux") {
                args.push("--allow-lan");
            }
        }
        helper(&args, Some(&cfg.render(None)))
    });

    let mut st = sh.lock().unwrap();
    if !st.current_attempt(generation) { return; }
    match res {
        Ok(()) => {
            st.lookup_protected = lookup_protected;
            st.status.state = State::Connected;
            st.status.connected_at = Some(now());
            st.retries = 0;
            st.last_sample = None;
            st.rx_seen = (Instant::now(), 0, 0);
            let session = Session { profile_id: Some(profile_id.clone()), connected_at: st.status.connected_at };
            let _ = st.store.save_session(&session);
            let mut s = st.store.settings();
            s.last_profile = Some(profile_id);
            let _ = st.store.save_settings(&s);
            drop(st);
            refresh_ip(app, Duration::from_millis(1500));
        }
        Err(e) if phase == State::Reconnecting && st.want_up && st.retries < MAX_RETRIES => {
            st.retries += 1;
            st.status.error = Some(format!("Reconnect attempt {} failed: {e}", st.retries));
            let delay = Duration::from_secs(2u64.pow(st.retries));
            emit(app, &st);
            drop(st);
            drop(operation);
            std::thread::sleep(delay);
            if shared(app).lock().unwrap().want_up {
                bring_up(app, profile_id, State::Reconnecting, generation);
            }
        }
        Err(e) => {
            st.status.state = State::Disconnected;
            st.status.error = Some(e);
            st.want_up = false;
            emit(app, &st);
        }
    }
}

pub fn disconnect(app: &AppHandle) {
    let sh = shared(app);
    let wanted_up = {
        let mut st = sh.lock().unwrap();
        if st.status.state == State::Disconnecting { return; }
        let wanted_up = st.want_up;
        st.want_up = false;
        st.generation += 1;
        st.status.ip_pending = false;
        st.status.state = State::Disconnecting;
        emit(app, &st);
        wanted_up
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let _operation = OPERATION.lock().unwrap();
        let res = helper(&["down"], None);
        let mut st = sh.lock().unwrap();
        match res {
            Ok(()) => {
                st.status = Status { ip: None, ..Status::default() };
                let _ = st.store.save_session(&Session::default());
                drop(st);
                refresh_ip(&app, Duration::from_millis(500));
            }
            Err(e) => {
                // Still up (e.g. auth cancelled): report the real state.
                st.disconnect_failed(wanted_up, e);
                emit(&app, &st);
                drop(st);
                refresh_ip(&app, Duration::ZERO);
            }
        }
    });
}

/// Public IP via ipinfo.io over verified TLS. While exposed this would hand
/// the real IP to a third party, so it only runs then if explicitly allowed.
pub fn refresh_ip(app: &AppHandle, delay: Duration) {
    let (generation, request) = {
        let sh = shared(app);
        let mut st = sh.lock().unwrap();
        st.ip_request += 1;
        (st.generation, st.ip_request)
    };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(8))).build().into();
        // A fresh tunnel can drop the first packets while its handshake completes, so retry.
        for wait in [0, 2, 5] {
            std::thread::sleep(Duration::from_secs(wait));
            let started = {
                let sh = shared(&app);
                let mut st = sh.lock().unwrap();
                let s = st.store.settings();
                let state = st.status.state;
                if st.generation != generation || st.ip_request != request { return; }
                if !st.allows_lookup(&s) {
                    st.status.ip = None;
                    st.status.ip_pending = false;
                    emit(&app, &st);
                    return;
                }
                st.status.ip_pending = true;
                emit(&app, &st);
                state
            };
            let info = agent.get("https://ipinfo.io/json").call().ok().and_then(|mut r| r.body_mut().read_json::<IpInfo>().ok());
            let sh = shared(&app);
            let mut st = sh.lock().unwrap();
            // An answer from before a connect/disconnect describes the wrong address.
            if !st.current_lookup(generation, request, started) {
                return;
            }
            if !st.allows_lookup(&st.store.settings()) {
                st.status.ip = None;
                st.status.ip_pending = false;
                emit(&app, &st);
                return;
            }
            if info.is_some() || wait == 5 {
                st.status.ip = info;
                st.status.ip_pending = false;
                emit(&app, &st);
                return;
            }
        }
    });
}

/// 1 Hz sampler of the tunnel interface's kernel counters.
pub fn monitor(app: AppHandle) {
    std::thread::spawn(move || {
        let mut nets = sysinfo::Networks::new_with_refreshed_list();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            nets.refresh(true);
            let counters = nets.iter().find(|(n, _)| n.as_str() == IFACE).map(|(_, d)| (d.total_received(), d.total_transmitted()));
            let sh = shared(&app);
            let mut st = sh.lock().unwrap();
            match (st.status.state, counters) {
                (State::Connected, Some((rx, tx))) => sample(&mut st, rx, tx),
                (State::Connected, None) => {
                    st.status.rx_rate = 0.0;
                    st.status.tx_rate = 0.0;
                    if st.want_up && st.store.settings().auto_reconnect {
                        let id = st.status.profile_id.clone().unwrap_or_default();
                        st.status.state = State::Reconnecting;
                        st.generation += 1;
                        st.status.ip = None;
                        st.status.ip_pending = false;
                        st.retries = 0;
                        let app = app.clone();
                        let generation = st.generation;
                        std::thread::spawn(move || bring_up(&app, id, State::Reconnecting, generation));
                    } else {
                        st.status.state = State::Disconnected;
                        st.status.error = Some("The tunnel went down unexpectedly.".into());
                        st.want_up = false;
                    }
                }
                (State::Disconnected, Some((rx, tx))) => {
                    // Tunnel left running by a previous session: adopt it.
                    let session = st.store.session();
                    st.status.state = State::Connected;
                    st.status.profile_id = session.profile_id;
                    st.status.connected_at = session.connected_at;
                    st.lookup_protected = st.status.profile_id.as_ref()
                        .and_then(|id| st.store.config_text(id).ok())
                        .and_then(|text| parse(&text).ok())
                        .is_some_and(|cfg| cfg.peers.iter().any(|p| p.allowed_ips.iter().any(|c| c == "0.0.0.0/0")));
                    st.want_up = true;
                    sample(&mut st, rx, tx);
                    drop(st);
                    refresh_ip(&app, Duration::ZERO);
                    continue;
                }
                _ => {}
            }
            emit(&app, &st);
        }
    });
}

fn sample(st: &mut Inner, rx: u64, tx: u64) {
    let t = Instant::now();
    if let Some((t0, rx0, tx0)) = st.last_sample {
        let dt = t.duration_since(t0).as_secs_f64().max(0.001);
        st.status.rx_rate = rx.saturating_sub(rx0) as f64 / dt;
        st.status.tx_rate = tx.saturating_sub(tx0) as f64 / dt;
    }
    st.last_sample = Some((t, rx, tx));
    st.status.rx = rx;
    st.status.tx = tx;
    let (seen, last_rx, tx_then) = st.rx_seen;
    if rx != last_rx {
        st.rx_seen = (t, rx, tx);
    }
    st.status.stalled = rx == last_rx && tx > tx_then && t.duration_since(seen) > STALL_AFTER;
}

fn helper_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(if cfg!(windows) { "burrow-helper.exe" } else { "burrow-helper" })
}

#[cfg(not(windows))]
fn helper(args: &[&str], stdin: Option<&str>) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // pkexec reports a missing binary with the same code as a denied prompt.
    if !helper_path().exists() {
        return Err(format!("{} is missing; reinstall Burrow.", helper_path().display()));
    }
    let mut child = Command::new("pkexec")
        .arg(helper_path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run pkexec (install polkit): {e}"))?;
    let mut pipe = child.stdin.take().unwrap();
    pipe.write_all(stdin.unwrap_or("").as_bytes()).map_err(|e| e.to_string())?;
    drop(pipe);
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    match out.status.code() {
        Some(0) => Ok(()),
        Some(126) | Some(127) => Err("Authorization was cancelled or denied.".into()),
        _ => {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if msg.is_empty() { "The helper failed.".into() } else { msg })
        }
    }
}

#[cfg(windows)]
fn helper(args: &[&str], stdin: Option<&str>) -> Result<(), String> {
    // UAC elevation can't pipe stdin, so the config goes through a temp file
    // in the user's own (private) temp dir and is deleted right after.
    let tmp = match stdin {
        Some(text) => {
            let p = std::env::temp_dir().join(format!("burrow-{}.conf", std::process::id()));
            std::fs::write(&p, text).map_err(|e| e.to_string())?;
            Some(p)
        }
        None => None,
    };
    let mut params = args.join(" ");
    if let Some(p) = &tmp {
        params += &format!(" \"{}\"", p.display());
    }
    let code = elevate(&helper_path(), &params);
    if let Some(p) = tmp {
        let _ = std::fs::remove_file(p);
    }
    match code? {
        0 => Ok(()),
        2 => Err("The configuration was rejected by the helper.".into()),
        3 => Err("WireGuard for Windows is not installed (https://www.wireguard.com/install/).".into()),
        4 => Err("WireGuard failed to start or stop the tunnel.".into()),
        c => Err(format!("The helper failed (exit code {c}).")),
    }
}

#[cfg(windows)]
fn elevate(exe: &std::path::Path, params: &str) -> Result<u32, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();
    let verb = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    let params = wide(params.as_ref());
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return Err("Administrator approval was cancelled.".into());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_requires_protection_or_exposed_consent() {
        let shared = new(Store { dir: PathBuf::from("/nonexistent/burrow-test") });
        let mut st = shared.lock().unwrap();
        let mut settings = crate::store::Settings::default();
        st.status.state = State::Connected;
        assert!(!st.allows_lookup(&settings));
        st.lookup_protected = true;
        assert!(st.allows_lookup(&settings));
        st.lookup_protected = false;
        settings.ip_check_exposed = true;
        assert!(st.allows_lookup(&settings));
        st.status.state = State::Connecting;
        assert!(!st.allows_lookup(&settings));
        st.status.state = State::Disconnected;
        assert!(st.allows_lookup(&settings));
        settings.ip_lookup = false;
        assert!(!st.allows_lookup(&settings));
    }

    #[test]
    fn lookup_rejects_old_connection_and_superseded_request() {
        let shared = new(Store { dir: PathBuf::from("/nonexistent/burrow-test") });
        let mut st = shared.lock().unwrap();
        st.status.state = State::Connected;
        assert!(st.current_lookup(0, 0, State::Connected));
        st.generation += 1;
        assert!(!st.current_lookup(0, 0, State::Connected));
        st.ip_request += 1;
        assert!(!st.current_lookup(1, 0, State::Connected));
        assert!(st.current_lookup(1, 1, State::Connected));
    }

    #[test]
    fn cancelled_disconnect_preserves_reconnect_intent() {
        let shared = new(Store { dir: PathBuf::from("/nonexistent/burrow-test") });
        let mut st = shared.lock().unwrap();
        st.disconnect_failed(true, "cancelled".into());
        assert!(st.want_up);
        assert_eq!(st.status.state, State::Connected);
        st.disconnect_failed(false, "cancelled".into());
        assert!(!st.want_up);
    }

    #[test]
    fn reserves_connect_and_rejects_stale_attempts() {
        let shared = new(Store { dir: PathBuf::from("/nonexistent/burrow-test") });
        let mut st = shared.lock().unwrap();
        let first = st.begin_connect().unwrap();
        assert!(st.current_attempt(first));
        assert!(st.begin_connect().is_none());
        for phase in [State::Connected, State::Reconnecting, State::Disconnecting] {
            st.status.state = phase;
            assert!(st.begin_connect().is_none());
        }
        st.want_up = false;
        st.generation += 1;
        assert!(!st.current_attempt(first));
        st.status.state = State::Disconnected;
        let second = st.begin_connect().unwrap();
        assert!(!st.current_attempt(first));
        assert!(st.current_attempt(second));
    }
}
