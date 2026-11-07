use crate::*;
use std::path::Path;
pub mod escl;
pub mod sane;
#[cfg(windows)]
pub mod windows;

pub trait Backend: Send + Sync {
    fn devices(&self, cancel: &Cancellation) -> Result<Vec<Device>>;
    fn capabilities(&self, id: &str) -> Result<Capabilities>;
    fn acquire(
        &self,
        profile: &Profile,
        directory: &Path,
        cancel: &Cancellation,
        on_page: &mut dyn FnMut(RawPage) -> Result<()>,
    ) -> Result<()>;
}

pub fn for_device(id: &str) -> Result<Box<dyn Backend>> {
    if id.starts_with("escl:") {
        return Ok(Box::new(escl::Escl));
    }
    #[cfg(windows)]
    if id.starts_with("wia:") {
        return Ok(Box::new(windows::Windows));
    }
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    if id.starts_with("sane:") {
        return Ok(Box::new(sane::Sane::system()?));
    }
    Err(error(
        "This scanner belongs to a different platform or driver. Refresh the scanner list and select it again.",
    ))
}

/// Discover installed OS drivers without opening a multicast listener.
/// Network discovery is explicit because it can prompt for firewall permission.
pub fn devices(cancel: &Cancellation) -> (Vec<Device>, Vec<String>) {
    let mut backends: Vec<Box<dyn Backend>> = Vec::new();
    let mut warnings = Vec::new();
    #[cfg(windows)]
    backends.push(Box::new(windows::Windows));
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    match sane::Sane::system() {
        Ok(backend) => backends.push(Box::new(backend)),
        Err(e) => warnings.push(e.to_string()),
    }
    let mut found = Vec::new();
    for backend in backends {
        match backend.devices(cancel) {
            Ok(mut devices) => found.append(&mut devices),
            Err(e) => warnings.push(e.to_string()),
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    (found, warnings)
}

/// Optional mDNS discovery. Hosts should invoke this only from a network-search action.
pub fn network_devices(cancel: &Cancellation) -> Result<Vec<Device>> {
    escl::Escl.devices(cancel)
}
