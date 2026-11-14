use bar_identity::EnvironmentSelection;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub environment_selection: EnvironmentSelection,
    pub imports_desktop_environments: bool,
    pub notifications_muted: bool,
    pub shows_previews: bool,
    pub automatically_checks_for_updates: bool,
    pub installs_without_asking: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            environment_selection: EnvironmentSelection::All,
            imports_desktop_environments: false,
            notifications_muted: false,
            shows_previews: true,
            automatically_checks_for_updates: true,
            installs_without_asking: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingKey {
    ImportsDesktopEnvironments,
    NotificationsMuted,
    ShowsPreviews,
    AutomaticallyChecksForUpdates,
    InstallsWithoutAsking,
}

pub struct SettingsStore {
    path: PathBuf,
    value: Settings,
    recovered: bool,
}
impl SettingsStore {
    /// Creates an in-memory store for a path whose existing contents could not
    /// be decoded. Callers can keep the original file in place and surface the
    /// load error without making startup fatal. Saves are disabled until the
    /// file is repaired and loaded again, so recovery cannot overwrite it.
    pub fn defaults(path: PathBuf) -> Self {
        Self {
            path,
            value: Settings::default(),
            recovered: true,
        }
    }

    pub fn load(path: PathBuf) -> io::Result<Self> {
        let value = match fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(e) => return Err(e),
        };
        let store = Self {
            path,
            value,
            recovered: false,
        };
        if !store.path.exists() {
            store.persist(&store.value)?;
        }
        Ok(store)
    }
    pub fn value(&self) -> &Settings {
        &self.value
    }
    pub fn set(&mut self, key: SettingKey, enabled: bool) -> io::Result<()> {
        let mut next = self.value.clone();
        match key {
            SettingKey::ImportsDesktopEnvironments => next.imports_desktop_environments = enabled,
            SettingKey::NotificationsMuted => next.notifications_muted = enabled,
            SettingKey::ShowsPreviews => next.shows_previews = enabled,
            SettingKey::AutomaticallyChecksForUpdates => {
                next.automatically_checks_for_updates = enabled
            }
            SettingKey::InstallsWithoutAsking => next.installs_without_asking = enabled,
        }
        self.persist(&next)?;
        self.value = next;
        Ok(())
    }
    pub fn set_environment_selection(&mut self, selection: EnvironmentSelection) -> io::Result<()> {
        let mut next = self.value.clone();
        next.environment_selection = selection;
        self.persist(&next)?;
        self.value = next;
        Ok(())
    }
    fn persist(&self, value: &Settings) -> io::Result<()> {
        if self.recovered {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Settings could not be loaded. Repair the settings file and restart before saving; the original file has been preserved.",
            ));
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use io::Write;
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        fs::rename(temporary, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_launch_persists_tauri_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(
            SettingsStore::load(path.clone()).unwrap().value(),
            &Settings::default()
        );
        let saved: Settings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved, Settings::default());
    }
    #[test]
    fn wrong_type_is_rejected_without_changing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let contents = br#"{"showsPreviews":"false"}"#;
        fs::write(&path, contents).unwrap();
        assert!(SettingsStore::load(path.clone()).is_err());
        assert_eq!(fs::read(path).unwrap(), contents);
    }
    #[test]
    fn preferences_survive_restart_and_new_fields_keep_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, br#"{"notificationsMuted":true}"#).unwrap();
        let mut store = SettingsStore::load(path.clone()).unwrap();
        assert!(store.value().notifications_muted);
        assert!(store.value().shows_previews);
        assert!(!store.value().imports_desktop_environments);
        store.set(SettingKey::ShowsPreviews, false).unwrap();
        let read = SettingsStore::load(path).unwrap();
        assert!(!read.value().shows_previews);
        assert!(read.value().notifications_muted);
    }
    #[test]
    fn desktop_environment_import_is_opt_in_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut store = SettingsStore::load(path.clone()).unwrap();
        assert!(!store.value().imports_desktop_environments);
        store
            .set(SettingKey::ImportsDesktopEnvironments, true)
            .unwrap();
        let mut restored = SettingsStore::load(path.clone()).unwrap();
        assert!(restored.value().imports_desktop_environments);
        restored
            .set(SettingKey::ImportsDesktopEnvironments, false)
            .unwrap();
        assert!(
            !SettingsStore::load(path)
                .unwrap()
                .value()
                .imports_desktop_environments
        );
    }
    #[test]
    fn selection_defaults_and_survives_restart_with_other_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, br#"{"notificationsMuted":true}"#).unwrap();
        let mut store = SettingsStore::load(path.clone()).unwrap();
        assert_eq!(
            store.value().environment_selection,
            EnvironmentSelection::All
        );
        let selection =
            EnvironmentSelection::One(bar_identity::EnvironmentId::new("remote-env").unwrap());
        store.set_environment_selection(selection.clone()).unwrap();
        let restored = SettingsStore::load(path).unwrap();
        assert_eq!(restored.value().environment_selection, selection);
        assert!(restored.value().notifications_muted);
    }

    #[test]
    fn malformed_selection_preserves_original_and_recovery_cannot_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let contents = br#"{"environmentSelection":{"one":"  "}}"#;
        fs::write(&path, contents).unwrap();
        assert_eq!(
            SettingsStore::load(path.clone()).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        let mut store = SettingsStore::defaults(path.clone());
        let selection = EnvironmentSelection::One(bar_identity::EnvironmentId::new("env").unwrap());
        assert_eq!(
            store
                .set_environment_selection(selection)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            store.value().environment_selection,
            EnvironmentSelection::All
        );
        assert_eq!(fs::read(path).unwrap(), contents);
    }

    #[test]
    fn failed_selection_save_does_not_commit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut store = SettingsStore::load(path.clone()).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let selection = EnvironmentSelection::One(bar_identity::EnvironmentId::new("env").unwrap());
        assert!(store.set_environment_selection(selection).is_err());
        assert_eq!(
            store.value().environment_selection,
            EnvironmentSelection::All
        );
    }

    #[test]
    fn malformed_preferences_are_not_silently_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"invalid").unwrap();
        assert!(SettingsStore::load(path.clone()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"invalid");
    }

    #[test]
    fn defaults_can_be_used_without_overwriting_malformed_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"invalid").unwrap();
        let mut store = SettingsStore::defaults(path.clone());
        assert_eq!(store.value(), &Settings::default());
        let error = store.set(SettingKey::ShowsPreviews, false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error
            .to_string()
            .contains("original file has been preserved"));
        assert_eq!(store.value(), &Settings::default());
        assert_eq!(fs::read(path).unwrap(), b"invalid");
    }
}
