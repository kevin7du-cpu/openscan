#![cfg_attr(windows, windows_subsystem = "windows")]
mod window;
use openscan_core::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf};

fn main() {
    let result=std::panic::catch_unwind(run).unwrap_or_else(|_|Err(error("The scanner operation stopped unexpectedly. Original scans remain in the recovery session.")));
    let (value, code) = match result {
        Ok(value) => {
            let code = if value.get("ok").and_then(Value::as_bool) == Some(false) {
                1
            } else {
                0
            };
            (value, code)
        }
        Err(e) => (
            json!({"ok":false,"protocolVersion":1,"error":e.to_string()}),
            1,
        ),
    };
    println!("{value}");
    std::process::exit(code);
}
fn input<T: serde::de::DeserializeOwned>() -> Result<T> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(error("Request exceeds 4 MB."));
    }
    Ok(serde_json::from_slice(&bytes)?)
}
fn run() -> Result<Value> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verb = args.first().map(String::as_str).unwrap_or("dialog");
    let cancel = Cancellation::default();
    match verb {
        "dialog" | "presets-dialog" => window::run(
            verb == "presets-dialog",
            args.iter().any(|a| a == "--return-json"),
            args.windows(2)
                .find(|w| w[0] == "--session")
                .map(|w| PathBuf::from(&w[1])),
        ),
        "version" => Ok(
            json!({"ok":true,"protocolVersion":1,"version":env!("CARGO_PKG_VERSION"),"implementation":"Rust","drivers":if cfg!(windows){vec!["wia","escl"]}else{vec!["sane","escl"]},"ocr":ocr::available()}),
        ),
        "devices" => {
            let (mut devices, mut warnings) = backend::devices(&cancel);
            if args.iter().any(|a| a == "--network") {
                match backend::network_devices(&cancel) {
                    Ok(mut found) => devices.append(&mut found),
                    Err(e) => warnings.push(e.to_string()),
                }
            }
            Ok(json!({"ok":true,"protocolVersion":1,"devices":devices,"warnings":warnings}))
        }
        "capabilities" => {
            let r: DeviceRequest = input()?;
            let caps = backend::for_device(&r.device_id)?.capabilities(&r.device_id)?;
            Ok(json!({"ok":true,"protocolVersion":1,"capabilities":caps}))
        }
        "scan" => {
            let r: ScanRequest = input()?;
            r.profile.validate()?;
            if r.output.exists() {
                return Err(error("The output exists; choose a new filename."));
            }
            let session = Session::new(r.session_directory)?;
            let (session, warning) = acquire(session, &r.profile, &cancel);
            if !session.pages.iter().any(|p| !p.excluded) {
                return Err(error(
                    warning.unwrap_or_else(|| "No pages received.".into()),
                ));
            }
            pdf::export(&session.pages, &r.output, &r.profile, false, &cancel)?;
            Ok(
                json!({"ok":warning.is_none(),"protocolVersion":1,"path":r.output.canonicalize()?,"session":session.directory,"result":{"received":session.pages.len(),"warning":warning}}),
            )
        }
        "process" => {
            let r: ProcessRequest = input()?;
            r.profile.validate()?;
            if r.images.is_empty() || r.images.len() > 250 {
                return Err(error("Provide 1–250 image files."));
            }
            if r.output.exists() {
                return Err(error("The output exists; choose a new filename."));
            }
            let mut session = Session::new(None)?;
            for path in r.images {
                session.import(&path, &r.profile, &cancel)?;
            }
            pdf::export(&session.pages, &r.output, &r.profile, false, &cancel)?;
            Ok(
                json!({"ok":true,"protocolVersion":1,"path":r.output.canonicalize()?,"session":session.directory,"pages":session.pages}),
            )
        }
        "preset-list" => {
            let r: DirectoryRequest = input()?;
            Ok(
                json!({"ok":true,"protocolVersion":1,"presets":PresetStore::new(r.directory)?.list()?}),
            )
        }
        "presets" => Ok(
            json!({"ok":true,"protocolVersion":1,"presets":PresetStore::new(args.get(1).map(PathBuf::from))?.list()?}),
        ),
        "preset-save" => {
            let r: PresetRequest = input()?;
            PresetStore::new(r.directory)?.save(&r.profile, r.overwrite)?;
            Ok(json!({"ok":true,"protocolVersion":1}))
        }
        "preset-delete" => {
            let r: NameRequest = input()?;
            PresetStore::new(r.directory)?.delete(&r.name)?;
            Ok(json!({"ok":true,"protocolVersion":1}))
        }
        _ => Err(error(
            "Commands: dialog, presets-dialog, devices, capabilities, scan, process, presets, preset-list, preset-save, preset-delete, version. Requests use JSON on stdin.",
        )),
    }
}
pub(crate) fn acquire(
    mut session: Session,
    profile: &Profile,
    cancel: &Cancellation,
) -> (Session, Option<String>) {
    let result = (|| {
        profile.validate()?;
        let id = profile
            .device_id
            .as_deref()
            .ok_or_else(|| error("Select a scanner."))?;
        let backend = backend::for_device(id)?;
        let raw = session.acquisition_directory()?;
        backend.acquire(profile, &raw, cancel, &mut |page| {
            session.add(page, profile, cancel)
        })
    })();
    let warning = result.err().map(|e| e.to_string());
    (session, warning)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeviceRequest {
    device_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScanRequest {
    profile: Profile,
    output: PathBuf,
    session_directory: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProcessRequest {
    images: Vec<PathBuf>,
    profile: Profile,
    output: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryRequest {
    directory: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetRequest {
    profile: Profile,
    #[serde(default)]
    overwrite: bool,
    directory: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameRequest {
    name: String,
    directory: Option<PathBuf>,
}
