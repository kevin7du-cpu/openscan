//! Explicit optional download. The application never fetches models at startup.
use openscan_core::{Result, error, storage};
use sha2::{Digest, Sha256};
use std::{io::Read, path::PathBuf};

fn main() -> Result<()> {
    let directory = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or_else(|| error("Usage: download_models <model-directory>"))?,
    );
    std::fs::create_dir_all(&directory)?;
    for (name, hash) in [
        (
            "text-detection.onnx",
            "a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe",
        ),
        (
            "text-recognition.onnx",
            "86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5",
        ),
    ] {
        let path = directory.join(name);
        if path.is_file()
            && std::fs::metadata(&path)?.len() <= 256 * 1024 * 1024
            && format!("{:x}", Sha256::digest(std::fs::read(&path)?)) == hash
        {
            println!("Verified {name}");
            continue;
        }
        let url = format!("https://ocrs-models.s3-accelerate.amazonaws.com/{name}");
        let mut response = ureq::get(&url).call().map_err(|e| error(e.to_string()))?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(256 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 256 * 1024 * 1024 || format!("{:x}", Sha256::digest(&bytes)) != hash {
            return Err(error(format!("Model checksum mismatch: {name}")));
        }
        storage::atomic_write(&path, &bytes, true)?;
        println!("Downloaded and verified {name}");
    }
    Ok(())
}
