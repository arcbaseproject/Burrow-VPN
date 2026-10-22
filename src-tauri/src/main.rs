#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod store;
mod tunnel;

use std::time::Duration;
use store::{Profile, Settings, Store};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use tunnel::{Shared, Status};

#[tauri::command]
fn status(s: State<Shared>) -> Status {
    s.lock().unwrap().status.clone()
}

#[tauri::command]
fn profiles(s: State<Shared>) -> Vec<Profile> {
    s.lock().unwrap().store.profiles()
}

#[tauri::command]
fn profile_config(s: State<Shared>, id: String) -> Result<String, String> {
    s.lock().unwrap().store.config_text(&id).map(|t| burrow_helper::mask(&t))
}

#[tauri::command]
fn save_profile(s: State<Shared>, id: Option<String>, name: String, text: String) -> Result<Profile, String> {
    s.lock().unwrap().store.save_profile(id, &name, &text)
}

#[tauri::command]
fn delete_profile(s: State<Shared>, id: String) -> Result<(), String> {
    let st = s.lock().unwrap();
    if st.status.profile_id.as_deref() == Some(&id) && st.status.state != tunnel::State::Disconnected {
        return Err("Disconnect before deleting this profile.".into());
    }
    st.store.delete_profile(&id)
}

#[tauri::command]
fn settings(s: State<Shared>) -> Settings {
    s.lock().unwrap().store.settings()
}

#[tauri::command]
fn save_settings(app: AppHandle, s: State<Shared>, settings: Settings) -> Result<(), String> {
    let mut clean = settings.clone();
    clean.dns = settings.dns.iter().map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect();
    // Reuse the config parser's DNS validation.
    let mut probe = burrow_helper::Config { interface: Default::default(), peers: vec![] };
    probe.override_dns(&clean.dns)?;
    let auto = app.autolaunch();
    if clean.launch_at_login != auto.is_enabled().unwrap_or(false) {
        let r = if clean.launch_at_login { auto.enable() } else { auto.disable() };
        r.map_err(|e| format!("launch at login: {e}"))?;
    }
    let st = s.lock().unwrap();
    // last_profile is owned by the backend.
    clean.last_profile = st.store.settings().last_profile;
    st.store.save_settings(&clean)
}

#[tauri::command]
fn connect(app: AppHandle, id: String) {
    tunnel::connect(&app, id);
}

#[tauri::command]
fn disconnect(app: AppHandle) {
    tunnel::disconnect(&app);
}

#[tauri::command]
fn refresh_ip(app: AppHandle) {
    tunnel::refresh_ip(&app, Duration::ZERO);
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn connect_last(app: &AppHandle) {
    let st = app.state::<Shared>();
    let (s, list) = {
        let st = st.lock().unwrap();
        (st.store.settings(), st.store.profiles())
    };
    let id = s.last_profile.filter(|id| list.iter().any(|p| &p.id == id)).or(list.first().map(|p| p.id.clone()));
    if let Some(id) = id {
        tunnel::connect(app, id);
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(|app| {
            let store = Store { dir: app.path().app_config_dir()? };
            let settings = store.settings();
            let tunnel_up = sysinfo::Networks::new_with_refreshed_list().iter().any(|(n, _)| n.as_str() == tunnel::IFACE);
            app.manage(tunnel::new(store));

            let handle = app.handle();
            let item = |id: &str, label: &str| MenuItem::with_id(handle, id, label, true, None::<&str>);
            let menu = Menu::with_items(
                handle,
                &[&item("show", "Show Burrow")?, &item("connect", "Connect")?, &item("disconnect", "Disconnect")?, &item("quit", "Quit")?],
            )?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Burrow VPN")
                .menu(&menu)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "show" => show(app),
                    "connect" => connect_last(app),
                    "disconnect" => tunnel::disconnect(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            if !std::env::args().any(|a| a == "--minimized") {
                show(handle);
            }
            tunnel::monitor(handle.clone());
            tunnel::refresh_ip(handle, Duration::ZERO);
            // A tunnel still up from last session is adopted by the monitor instead.
            if settings.auto_connect && !tunnel_up {
                connect_last(handle);
            }
            Ok(())
        })
        // Closing the window quits: many Linux desktops show no tray, and a hidden
        // instance would make every relaunch exit through the single-instance guard.
        // The tunnel is a system interface, keeps running, and is adopted on next start.
        .on_window_event(|w, e| {
            if let WindowEvent::CloseRequested { .. } = e {
                w.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            status,
            profiles,
            profile_config,
            save_profile,
            delete_profile,
            settings,
            save_settings,
            connect,
            disconnect,
            refresh_ip
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Burrow");
}
