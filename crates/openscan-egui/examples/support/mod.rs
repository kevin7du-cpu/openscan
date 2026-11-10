use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, PartialEq)]
pub struct Paths {
    pub storage: PathBuf,
    pub models: PathBuf,
}

impl Paths {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Option<Self>, String> {
        let mut arguments = arguments.into_iter();
        let mut storage = None;
        let mut models = None;
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--help" | "-h") => return Ok(None),
                Some("--storage") => storage = Some(value(&mut arguments, "--storage")?),
                Some("--models") => models = Some(value(&mut arguments, "--models")?),
                _ => return Err(format!("Unknown argument: {}", argument.to_string_lossy())),
            }
        }
        let storage = storage
            .or_else(default_storage)
            .ok_or_else(|| "No user data directory found; supply --storage DIRECTORY".to_owned())?;
        let models = models
            .or_else(|| env_path("OPENSCAN_MODELS"))
            .unwrap_or_else(|| storage.join("models"));
        Ok(Some(Self { storage, models }))
    }
}

fn value(arguments: &mut impl Iterator<Item = OsString>, name: &str) -> Result<PathBuf, String> {
    arguments
        .next()
        .filter(|value| !value.is_empty() && !value.to_string_lossy().starts_with("--"))
        .map(PathBuf::from)
        .ok_or_else(|| format!("{name} requires a directory"))
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn default_storage() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env_path("LOCALAPPDATA").map(|base| base.join("OpenScan/Rust"))
    }
    #[cfg(target_os = "macos")]
    {
        env_path("HOME").map(|base| base.join("Library/Application Support/OpenScan"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        env_path("XDG_DATA_HOME")
            .or_else(|| env_path("HOME").map(|base| base.join(".local/share")))
            .map(|base| base.join("openscan"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_paths_allow_spaces_without_a_source_checkout() {
        let paths =
            Paths::parse(["--storage", "My scans", "--models", "OCR models"].map(Into::into))
                .unwrap()
                .unwrap();
        assert_eq!(paths.storage, PathBuf::from("My scans"));
        assert_eq!(paths.models, PathBuf::from("OCR models"));
    }

    #[test]
    fn rejects_unknown_and_missing_arguments_and_accepts_help() {
        for arguments in [
            vec!["--unknown"],
            vec!["--storage"],
            vec!["--models", "--storage"],
        ] {
            assert!(Paths::parse(arguments.into_iter().map(Into::into)).is_err());
        }
        assert!(Paths::parse(["--help".into()]).unwrap().is_none());
    }
}
