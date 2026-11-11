//! Model presence checks and verified download from MathCraft-Models releases.
//!
//! Models are not bundled in the DMG (≈210 MB); they are fetched once per
//! model on first use, SHA-256 verified, and shared with the legacy app.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

const MANIFEST: &str = include_str!("../resources/models.json");

/// Every model the app knows, in display order, with its archive size
/// (Content-Length of the v1.0.0 release asset) for the download button.
pub const CATALOG: [(&str, u64); 4] = [
    ("mathcraft-formula-det", 67_531_517),
    ("mathcraft-formula-rec", 108_795_631),
    ("mathcraft-text-det", 4_533_780),
    ("mathcraft-text-rec", 14_102_746),
];

#[derive(Deserialize)]
struct Manifest {
    models: BTreeMap<String, ModelSpec>,
}

#[derive(Deserialize, Clone)]
pub struct ModelSpec {
    pub files: Vec<FileSpec>,
    pub sources: Vec<String>,
}

#[derive(Deserialize, Clone)]
pub struct FileSpec {
    pub path: String,
    pub sha256: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Progress {
    pub model: String,
    pub downloaded: u64,
    pub total: u64,
}

pub fn spec(model: &str) -> Result<ModelSpec> {
    let manifest: Manifest = serde_json::from_str(MANIFEST).expect("embedded manifest is valid");
    manifest
        .models
        .get(model)
        .cloned()
        .ok_or_else(|| anyhow!("unknown model {model}"))
}

/// Cheap presence check (sizes only); hashes are verified on download.
pub fn is_installed(root: &Path, model: &str) -> bool {
    spec(model).is_ok_and(|s| {
        s.files.iter().all(|f| {
            root.join(model)
                .join(&f.path)
                .metadata()
                .is_ok_and(|m| m.len() > 0)
        })
    })
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Extract `archive` and install verified files into `root/model`.
pub fn install_archive(root: &Path, model: &str, archive: &Path) -> Result<()> {
    let spec = spec(model)?;
    let staging = root.join(format!(".{model}.staging"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)
        .context("downloaded archive is not a zip")?;
    for f in &spec.files {
        // Accept both flat archives and archives with a `<model>/` folder.
        let mut entry = None;
        for i in 0..zip.len() {
            let name = zip.by_index(i)?.name().to_string();
            if name == f.path || name.ends_with(&format!("/{}", f.path)) {
                entry = Some(i);
                break;
            }
        }
        let index = entry.ok_or_else(|| anyhow!("{} missing from archive", f.path))?;
        let target = staging.join(&f.path);
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut zip.by_index(index)?, &mut out)?;
        out.flush()?;
        let actual = sha256_file(&target)?;
        if actual != f.sha256 {
            let _ = std::fs::remove_dir_all(&staging);
            bail!("checksum mismatch for {model}/{}", f.path);
        }
    }
    let final_dir = root.join(model);
    let _ = std::fs::remove_dir_all(&final_dir);
    std::fs::rename(&staging, &final_dir)?;
    Ok(())
}

pub fn download(root: &Path, model: &str, on_progress: &mut dyn FnMut(Progress)) -> Result<()> {
    let spec = spec(model)?;
    std::fs::create_dir_all(root)?;
    let part = root.join(format!(".{model}.zip.part"));
    let mut last_err = anyhow!("no download source");
    for url in &spec.sources {
        match fetch(url, &part, model, on_progress) {
            Ok(()) => {
                let result = install_archive(root, model, &part);
                let _ = std::fs::remove_file(&part);
                return result;
            }
            Err(e) => last_err = e,
        }
    }
    let _ = std::fs::remove_file(&part);
    Err(last_err.context(format!("failed to download {model}")))
}

fn fetch(url: &str, dest: &Path, model: &str, on_progress: &mut dyn FnMut(Progress)) -> Result<()> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(600))
        .call()?;
    let total = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut reader = resp.into_reader();
    let mut out = std::fs::File::create(dest)?;
    let mut buf = vec![0u8; 1 << 16];
    let mut downloaded = 0u64;
    let mut last_emit = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        downloaded += n as u64;
        if downloaded - last_emit >= 1 << 20 {
            last_emit = downloaded;
            on_progress(Progress {
                model: model.into(),
                downloaded,
                total,
            });
        }
    }
    on_progress(Progress {
        model: model.into(),
        downloaded,
        total,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn manifest_lists_all_models() {
        for m in [
            "mathcraft-formula-det",
            "mathcraft-formula-rec",
            "mathcraft-text-det",
            "mathcraft-text-rec",
        ] {
            let s = spec(m).unwrap();
            assert!(!s.files.is_empty() && s.sources[0].starts_with("https://"));
        }
        assert!(spec("nope").is_err());
    }

    #[test]
    fn catalog_matches_manifest() {
        for (id, bytes) in CATALOG {
            assert!(spec(id).is_ok(), "{id} missing from manifest");
            assert!(bytes > 1_000_000);
        }
    }

    #[test]
    fn rejects_tampered_archive() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("a.zip");
        make_zip(
            &zip_path,
            &[(
                "mathcraft-text-det/ppocrv5_mobile_det.onnx",
                b"not the model",
            )],
        );
        let err = install_archive(dir.path(), "mathcraft-text-det", &zip_path).unwrap_err();
        assert!(err.to_string().contains("checksum"), "{err}");
        assert!(!is_installed(dir.path(), "mathcraft-text-det"));
        assert!(!dir.path().join(".mathcraft-text-det.staging").exists());
    }

    #[test]
    fn reports_missing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("a.zip");
        make_zip(&zip_path, &[("other.txt", b"x")]);
        assert!(install_archive(dir.path(), "mathcraft-text-rec", &zip_path)
            .unwrap_err()
            .to_string()
            .contains("missing"));
    }

    #[test]
    fn downloads_from_local_server() {
        // Serve a deliberately wrong archive: proves the HTTP + verify path end to end.
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("served.zip");
        make_zip(&zip_path, &[("ppocrv5_mobile_det.onnx", b"fake")]);
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let bytes = std::fs::read(&zip_path).unwrap();
        let handle = std::thread::spawn(move || {
            let req = server.recv().unwrap();
            req.respond(tiny_http::Response::from_data(bytes)).unwrap();
        });
        let mut events = Vec::new();
        let dest = dir.path().join("dl.part");
        fetch(
            &format!("http://{addr}/m.zip"),
            &dest,
            "mathcraft-text-det",
            &mut |p| events.push(p),
        )
        .unwrap();
        handle.join().unwrap();
        assert_eq!(
            events.last().unwrap().downloaded,
            std::fs::metadata(&dest).unwrap().len()
        );
        assert!(install_archive(dir.path(), "mathcraft-text-det", &dest).is_err());
    }
}
