use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn data_directory() -> Result<PathBuf> {
    directories::ProjectDirs::from("org", "OpenScan", "OpenScan")
        .map(|p| p.data_local_dir().to_path_buf())
        .ok_or_else(|| error("Cannot locate the application data directory."))
}
pub fn atomic_write(path: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| error("Choose an absolute output filename."))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if overwrite {
        temp.persist(path).map_err(|e| Error::Io(e.error))?;
    } else {
        temp.persist_noclobber(path)
            .map_err(|e| Error::Io(e.error))?;
    }
    Ok(())
}
pub fn bounded_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    if fs::metadata(path)?.len() > 4 * 1024 * 1024 {
        return Err(error("Settings file exceeds 4 MB."));
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub struct PresetStore {
    pub directory: PathBuf,
}
impl PresetStore {
    pub fn new(directory: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            directory: match directory {
                Some(p) => p,
                None => data_directory()?.join("Presets"),
            },
        })
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(format!(
            "{:x}.json",
            Sha256::digest(name.trim().to_uppercase().as_bytes())
        ))
    }
    pub fn list(&self) -> Result<Vec<Profile>> {
        if !self.directory.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.directory)?.take(1000) {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                let p: Profile = bounded_json(&path)?;
                p.validate()?;
                out.push(p);
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
    pub fn save(&self, p: &Profile, overwrite: bool) -> Result<()> {
        p.validate()?;
        atomic_write(
            &self.path(&p.name),
            &serde_json::to_vec_pretty(p)?,
            overwrite,
        )
    }
    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.path(name);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub directory: PathBuf,
    pub pages: Vec<Page>,
}
impl Session {
    pub fn new(directory: Option<PathBuf>) -> Result<Self> {
        let directory = if let Some(p) = directory {
            if p.exists() {
                return Err(error("Session directory already exists."));
            }
            fs::create_dir_all(&p)?;
            p.canonicalize()?
        } else {
            let base = data_directory()?.join("Sessions");
            fs::create_dir_all(&base)?;
            tempfile::Builder::new()
                .prefix("scan-")
                .tempdir_in(base)?
                .keep()
        };
        fs::create_dir_all(directory.join("originals"))?;
        fs::create_dir_all(directory.join("processed"))?;
        let session = Self {
            directory,
            pages: Vec::new(),
        };
        session.save()?;
        Ok(session)
    }
    pub fn save(&self) -> Result<()> {
        atomic_write(
            &self.directory.join("session.json"),
            &serde_json::to_vec_pretty(self)?,
            true,
        )
    }
    pub fn acquisition_directory(&self) -> Result<PathBuf> {
        Ok(tempfile::Builder::new()
            .prefix("acquire-")
            .tempdir_in(&self.directory)?
            .keep())
    }
    pub fn load(directory: &Path) -> Result<Self> {
        let root = directory.canonicalize()?;
        let mut session: Self = bounded_json(&root.join("session.json"))?;
        if session.pages.len() > 250 {
            return Err(error("Session exceeds 250 pages."));
        }
        for page in &session.pages {
            for path in [&page.original_path, &page.image_path] {
                if !path.canonicalize()?.starts_with(&root) {
                    return Err(error("Session image resolves outside its directory."));
                }
            }
            if !(75..=600).contains(&page.dpi) || ![0, 90, 180, 270].contains(&page.rotation) {
                return Err(error("Invalid session page settings."));
            }
        }
        session.directory = root;
        Ok(session)
    }
    pub fn add(&mut self, raw: RawPage, p: &Profile, cancel: &Cancellation) -> Result<()> {
        if self.pages.len() >= 250 {
            return Err(error("A session can hold at most 250 pages."));
        }
        let id = format!("page-{:04}", self.pages.len() + 1);
        let extension = raw
            .path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("png");
        let original = self
            .directory
            .join("originals")
            .join(format!("{id}.{extension}"));
        if raw.path != original {
            if fs::metadata(&raw.path)?.len() > correction::MAX_BYTES {
                return Err(error("Scan exceeds 256 MB."));
            }
            let bytes = fs::read(&raw.path)?;
            atomic_write(&original, &bytes, false)?;
        }
        // Register a received original first; cancellation or correction failure cannot lose it.
        self.pages.push(Page {
            id,
            original_path: original.clone(),
            image_path: original,
            dpi: raw.dpi,
            deskew_angle: 0.,
            ink_percent: 0.,
            excluded: false,
            rotation: 0,
        });
        self.save()?;
        let i = self.pages.len() - 1;
        let rotation = if p.flip_duplex_backs && p.source == Source::Duplex && i % 2 == 1 {
            180
        } else {
            0
        };
        self.reprocess(i, p, rotation, cancel)
    }
    pub fn reprocess(
        &mut self,
        index: usize,
        p: &Profile,
        rotation: u16,
        cancel: &Cancellation,
    ) -> Result<()> {
        let page = self
            .pages
            .get(index)
            .ok_or_else(|| error("Page no longer exists."))?;
        let result =
            correction::process(correction::read(&page.original_path)?, p, rotation, cancel)?;
        let mut file = tempfile::Builder::new()
            .prefix("corrected-")
            .suffix(".png")
            .tempfile_in(self.directory.join("processed"))?;
        result
            .image
            .write_to(file.as_file_mut(), image::ImageFormat::Png)?;
        file.as_file().sync_all()?;
        let (_, path) = file.keep().map_err(|e| Error::Io(e.error))?;
        let page = self
            .pages
            .get_mut(index)
            .ok_or_else(|| error("Page no longer exists."))?;
        page.image_path = path;
        page.deskew_angle = result.angle;
        page.ink_percent = result.ink_percent;
        page.excluded =
            p.cleanup.skip_blank_pages && result.ink_percent < p.cleanup.blank_ink_percent;
        page.rotation = rotation;
        self.save()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_cannot_escape_or_overwrite_without_consent() {
        let tmp = tempfile::tempdir().expect("directory");
        let store = PresetStore::new(Some(tmp.path().join("presets"))).expect("store");
        let p = Profile {
            name: "../../outside".into(),
            ..Default::default()
        };
        store.save(&p, false).expect("save");
        assert!(store.save(&p, false).is_err());
        assert_eq!(store.list().expect("list").len(), 1);
        assert!(!tmp.path().join("outside.json").exists());
        store.delete(&p.name).expect("delete");
        assert!(store.list().expect("list").is_empty());
    }
}
