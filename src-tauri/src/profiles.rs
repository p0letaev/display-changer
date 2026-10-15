//! Profile persistence: a single JSON file in the app config directory.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use display_core::Profile;
use serde::{Deserialize, Serialize};

const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct ProfileFile {
    version: u32,
    profiles: Vec<Profile>,
}

pub struct ProfileStore {
    path: PathBuf,
    profiles: Vec<Profile>,
}

impl ProfileStore {
    /// Reads profiles from `path`. A missing file starts from `seed`; an
    /// unreadable one is moved aside to `*.bak` so it is not overwritten.
    pub fn load(path: PathBuf, seed: impl FnOnce() -> Vec<Profile>) -> io::Result<Self> {
        let profiles = match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<ProfileFile>(&text) {
                Ok(file) => file.profiles,
                Err(e) => {
                    let backup = path.with_extension("json.bak");
                    log::error!(
                        "cannot parse {}: {e}; moved to {}",
                        path.display(),
                        backup.display()
                    );
                    fs::rename(&path, &backup)?;
                    Vec::new()
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let store = Self {
                    path,
                    profiles: seed(),
                };
                store.save()?;
                return Ok(store);
            }
            Err(e) => return Err(e),
        };
        Ok(Self { path, profiles })
    }

    pub fn all(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// Replaces the profile with the same id, or appends a new one.
    pub fn upsert(&mut self, profile: Profile) -> io::Result<()> {
        match self.profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(existing) => *existing = profile,
            None => self.profiles.push(profile),
        }
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> io::Result<bool> {
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        if self.profiles.len() == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = ProfileFile {
            version: FORMAT_VERSION,
            profiles: self.profiles.clone(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(io::Error::other)?;
        // Write-then-rename so a crash never leaves a half-written file.
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, &self.path)
    }
}

pub fn new_profile_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!(
        "p{millis:x}{:02x}",
        COUNTER.fetch_add(1, Ordering::Relaxed) % 256
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use display_core::Layout;

    fn profile(id: &str) -> Profile {
        Profile {
            id: id.into(),
            name: id.into(),
            hotkey: None,
            layout: Layout::default(),
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dc-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("profiles.json")
    }

    #[test]
    fn seeds_missing_file_and_round_trips() {
        let path = temp_path("seed");
        let mut store = ProfileStore::load(path.clone(), || vec![profile("a")]).unwrap();
        store.upsert(profile("b")).unwrap();
        assert!(store.remove("a").unwrap());

        let reloaded = ProfileStore::load(path, Vec::new).unwrap();
        assert_eq!(reloaded.all(), &[profile("b")]);
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let path = temp_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        let store = ProfileStore::load(path.clone(), || vec![profile("seed")]).unwrap();
        assert!(store.all().is_empty());
        assert!(path.with_extension("json.bak").exists());
    }
}
