//! Profiles, settings and secrets. Config text (it holds the private key) goes
//! to the OS credential store; if none is available, to a 0600 file.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use burrow_helper::{parse, unmask, HIDDEN};

const SERVICE: &str = "app.burrow.vpn";

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub endpoints: Vec<String>,
    pub full_tunnel: bool,
    #[serde(default)]
    pub key_file: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub auto_connect: bool,
    pub auto_reconnect: bool,
    pub kill_switch: bool,
    /// Kill switch exception for private/link-local/multicast ranges (Linux).
    pub allow_lan: bool,
    pub launch_at_login: bool,
    /// Look up the public IP while protected (the request goes through the tunnel).
    pub ip_lookup: bool,
    /// Also look it up while exposed, which sends the real IP to ipinfo.io.
    pub ip_check_exposed: bool,
    /// Mask IP addresses on screen until revealed.
    pub hide_ips: bool,
    pub dns: Vec<String>,
    pub theme: String,
    pub last_profile: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auto_connect: false,
            auto_reconnect: true,
            kill_switch: true,
            allow_lan: false,
            launch_at_login: false,
            ip_lookup: true,
            ip_check_exposed: false,
            hide_ips: true,
            dns: vec![],
            theme: "dark".into(),
            last_profile: None,
        }
    }
}

/// Survives app restarts so a running tunnel can be re-adopted.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Session {
    pub profile_id: Option<String>,
    pub connected_at: Option<u64>,
}

pub struct Store {
    pub dir: PathBuf,
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid profile ID".into());
    }
    Ok(())
}

impl Store {
    fn read<T: for<'de> Deserialize<'de> + Default>(&self, file: &str) -> T {
        std::fs::read(self.dir.join(file)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write<T: Serialize>(&self, file: &str, v: &T) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(format!("{file}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(v).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, self.dir.join(file)).map_err(|e| e.to_string())
    }

    pub fn profiles(&self) -> Vec<Profile> {
        self.read("profiles.json")
    }
    pub fn settings(&self) -> Settings {
        self.read("settings.json")
    }
    pub fn save_settings(&self, s: &Settings) -> Result<(), String> {
        self.write("settings.json", s)
    }
    pub fn session(&self) -> Session {
        self.read("session.json")
    }
    pub fn save_session(&self, s: &Session) -> Result<(), String> {
        self.write("session.json", s)
    }

    pub fn save_profile(&self, id: Option<String>, name: &str, text: &str) -> Result<Profile, String> {
        if let Some(id) = &id { validate_id(id)?; }
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err("name must be 1-64 characters".into());
        }
        let text = match &id {
            Some(id) if text.contains(HIDDEN) => unmask(text, &self.config_text(id)?)?,
            _ => text.to_string(),
        };
        let cfg = parse(&text)?;
        let id = id.unwrap_or_else(|| {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
            format!("{:x}", t.as_nanos())
        });
        let mut list = self.profiles();
        if !list.iter().any(|p| p.id == id) && list.len() >= 500 {
            return Err("too many profiles".into());
        }
        let key_file = self.put_secret(&id, &cfg.render(None))?;
        let p = Profile { id: id.clone(), name: name.into(), endpoints: cfg.endpoints(), full_tunnel: cfg.full_tunnel(), key_file };
        match list.iter_mut().find(|x| x.id == id) {
            Some(x) => *x = p.clone(),
            None => list.push(p.clone()),
        }
        self.write("profiles.json", &list)?;
        Ok(p)
    }

    pub fn config_text(&self, id: &str) -> Result<String, String> {
        validate_id(id)?;
        let p = self.profiles().into_iter().find(|p| p.id == id).ok_or("profile not found")?;
        if p.key_file {
            std::fs::read_to_string(self.key_path(id)).map_err(|e| e.to_string())
        } else {
            keyring::Entry::new(SERVICE, id).and_then(|e| e.get_password()).map_err(|e| format!("credential store: {e}"))
        }
    }

    pub fn delete_profile(&self, id: &str) -> Result<(), String> {
        validate_id(id)?;
        let mut list = self.profiles();
        list.retain(|p| p.id != id);
        self.write("profiles.json", &list)?;
        let _ = keyring::Entry::new(SERVICE, id).and_then(|e| e.delete_credential());
        let _ = std::fs::remove_file(self.key_path(id));
        Ok(())
    }

    fn key_path(&self, id: &str) -> PathBuf {
        self.dir.join("keys").join(format!("{id}.conf"))
    }

    /// Returns true if the fallback file was used.
    fn put_secret(&self, id: &str, text: &str) -> Result<bool, String> {
        if keyring::Entry::new(SERVICE, id).and_then(|e| e.set_password(text)).is_ok() {
            let _ = std::fs::remove_file(self.key_path(id));
            return Ok(false);
        }
        let path = self.key_path(id);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        use std::io::Write;
        opts.open(&path).and_then(|mut f| f.write_all(text.as_bytes())).map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_profile_path_traversal_before_io() {
        let store = Store { dir: PathBuf::from("/nonexistent/burrow-test") };
        for id in ["", "../outside", "/tmp/outside", "..\\outside", "abc/def"] {
            assert!(store.config_text(id).is_err());
            assert!(store.delete_profile(id).is_err());
            assert!(store.save_profile(Some(id.into()), "test", "").is_err());
        }
        assert!(validate_id("123abcdef").is_ok());
    }
}
