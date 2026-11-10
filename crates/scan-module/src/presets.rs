//! Atomic JSON presets; names are hashed, never used as filesystem paths.
use crate::{CleanupOptions, Result, ScanError, ScanProfile};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn json_error(e: serde_json::Error) -> ScanError {
    ScanError::InvalidRequest(e.to_string())
}
pub(crate) fn atomic_json(path: &Path, value: &impl serde::Serialize, replace: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| ScanError::InvalidRequest("Missing parent directory".into()))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec_pretty(value).map_err(json_error)?)?;
    temp.as_file().sync_all()?;
    if replace {
        temp.persist(path).map_err(|e| ScanError::from(e.error))?;
    } else {
        temp.persist_noclobber(path)
            .map_err(|e| ScanError::from(e.error))?;
    }
    Ok(())
}

/// Per-library defaults shared by any host using the reusable scanner module.
/// Named presets remain independent and can deliberately override these choices.
pub struct CorrectionStore {
    path: PathBuf,
}
impl CorrectionStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            path: root.as_ref().join("image-correction.json"),
        }
    }
    pub fn load(&self) -> Result<CleanupOptions> {
        match fs::metadata(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CleanupOptions::default());
            }
            Err(e) => return Err(e.into()),
            Ok(meta) if meta.len() > 1024 * 1024 => {
                return Err(ScanError::InvalidRequest(
                    "Image correction settings exceed 1 MB".into(),
                ));
            }
            Ok(_) => (),
        }
        let options = serde_json::from_slice(&fs::read(&self.path)?).map_err(json_error)?;
        validate_corrections(&options)?;
        Ok(options)
    }
    pub fn save(&self, options: &CleanupOptions) -> Result<()> {
        validate_corrections(options)?;
        atomic_json(&self.path, options, true)
    }
}
fn validate_corrections(options: &CleanupOptions) -> Result<()> {
    ScanProfile {
        cleanup: options.clone(),
        ..Default::default()
    }
    .validate()
}

pub struct PresetStore {
    root: PathBuf,
}
impl PresetStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    fn path(&self, name: &str) -> PathBuf {
        let hash = Sha256::digest(name.trim().to_uppercase().as_bytes());
        let name = hash.iter().map(|b| format!("{b:02x}")).collect::<String>();
        self.root.join(format!("{name}.json"))
    }
    pub fn list(&self) -> Result<Vec<ScanProfile>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut profiles = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                if fs::metadata(&path)?.len() > 1024 * 1024 {
                    return Err(ScanError::InvalidRequest("Preset exceeds 1 MB".into()));
                }
                let profile: ScanProfile =
                    serde_json::from_slice(&fs::read(&path)?).map_err(json_error)?;
                profile.validate()?;
                profiles.push(profile);
            }
        }
        profiles.sort_by_key(|p| p.name.to_lowercase());
        Ok(profiles)
    }
    pub fn save(&self, profile: &ScanProfile, replace: bool) -> Result<()> {
        let mut profile = profile.clone();
        profile.name = profile.name.trim().into();
        profile.validate()?;
        atomic_json(&self.path(&profile.name), &profile, replace)
    }
    /// Archive rather than permanently remove a preset; archived files are ignored by list.
    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.path(name);
        let archive = self.root.join("deleted");
        fs::create_dir_all(&archive)?;
        fs::rename(path, archive.join(format!("{}.json", uuid::Uuid::new_v4())))?;
        Ok(())
    }
    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<()> {
        let old_path = self.path(old_name);
        let mut profile: ScanProfile =
            serde_json::from_slice(&fs::read(&old_path)?).map_err(json_error)?;
        profile.name = new_name.trim().into();
        profile.validate()?;
        if self.path(new_name) == old_path {
            return self.save(&profile, true);
        }
        self.save(&profile, false)?;
        self.delete(old_name)
    }
}

#[cfg(test)]
mod correction_tests {
    use super::*;

    #[test]
    fn corrections_survive_reopen_and_invalid_save_preserves_previous_settings() {
        let temp = tempfile::tempdir().unwrap();
        let store = CorrectionStore::new(temp.path());
        assert_eq!(store.load().unwrap(), CleanupOptions::default());
        let options = CleanupOptions {
            auto_crop: true,
            denoise: true,
            brightness: 14,
            contrast: -8,
            rotate_degrees: 90,
            ..Default::default()
        };
        store.save(&options).unwrap();
        assert_eq!(CorrectionStore::new(temp.path()).load().unwrap(), options);
        let before = fs::read(&store.path).unwrap();
        let invalid = CleanupOptions {
            brightness: 101,
            ..options.clone()
        };
        assert!(store.save(&invalid).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), before);
        assert_eq!(store.load().unwrap(), options);
    }

    #[test]
    fn broken_settings_are_reported_without_overwriting_them() {
        let temp = tempfile::tempdir().unwrap();
        let store = CorrectionStore::new(temp.path());
        fs::write(&store.path, "not JSON").unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read_to_string(&store.path).unwrap(), "not JSON");
        fs::remove_file(&store.path).unwrap();
        fs::create_dir(&store.path).unwrap();
        assert!(store.save(&CleanupOptions::default()).is_err());
    }
}
