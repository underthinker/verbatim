//! Resumable, hash-verified model downloads.
//!
//! This crate is deliberately the only production component allowed to own an
//! HTTP client. Bytes first land in a `.part` file, are verified against the
//! catalog's pinned SHA-256, then atomically renamed into the model store.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::RANGE;
use sha2::{Digest, Sha256};
use verbatim_engines::ModelHandle;
use verbatim_engines::model::{DownloadError, ModelDownloader, ModelSpec, ProgressSink};

pub struct HttpModelDownloader {
    models_dir: PathBuf,
    client: Client,
}

impl HttpModelDownloader {
    pub fn new(models_dir: PathBuf) -> Result<Self, DownloadError> {
        let client = Client::builder()
            .user_agent(concat!("Verbatim/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(transport)?;
        Ok(Self { models_dir, client })
    }

    fn paths(&self, spec: &ModelSpec) -> (PathBuf, PathBuf) {
        let final_path = self.models_dir.join(format!("{}.bin", spec.id));
        let partial_path = self.models_dir.join(format!("{}.bin.part", spec.id));
        (final_path, partial_path)
    }
}

impl ModelDownloader for HttpModelDownloader {
    fn download(
        &self,
        spec: &ModelSpec,
        progress: &ProgressSink,
    ) -> Result<ModelHandle, DownloadError> {
        if spec.url.is_empty() || spec.sha256.len() != 64 {
            return Err(DownloadError::Transport(format!(
                "{} has no downloadable artifact",
                spec.id
            )));
        }
        std::fs::create_dir_all(&self.models_dir).map_err(transport)?;
        let (final_path, partial_path) = self.paths(spec);

        if final_path.exists() && verify(&final_path, spec.sha256)? {
            progress(spec.size_bytes, spec.size_bytes);
            return Ok(ModelHandle { path: final_path });
        }

        let mut offset = std::fs::metadata(&partial_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if offset > spec.size_bytes {
            std::fs::remove_file(&partial_path).map_err(transport)?;
            offset = 0;
        }

        let mut request = self.client.get(spec.url);
        if offset > 0 {
            request = request.header(RANGE, format!("bytes={offset}-"));
        }
        let mut response = request.send().map_err(transport)?;
        if offset > 0 && response.status() == StatusCode::OK {
            // The origin ignored Range. Restart instead of appending duplicate bytes.
            offset = 0;
        } else if offset > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(DownloadError::Transport(format!(
                "resume failed with HTTP {}",
                response.status()
            )));
        } else if offset == 0 && !response.status().is_success() {
            return Err(DownloadError::Transport(format!(
                "download failed with HTTP {}",
                response.status()
            )));
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(offset == 0)
            .open(&partial_path)
            .map_err(transport)?;
        file.seek(SeekFrom::Start(offset)).map_err(transport)?;
        progress(offset, spec.size_bytes);
        let mut received = offset;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = response.read(&mut buffer).map_err(transport)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count]).map_err(transport)?;
            received = received.saturating_add(count as u64);
            progress(received.min(spec.size_bytes), spec.size_bytes);
        }
        file.sync_all().map_err(transport)?;

        if received != spec.size_bytes || !verify(&partial_path, spec.sha256)? {
            let _ = std::fs::remove_file(&partial_path);
            return Err(DownloadError::HashMismatch(spec.id.to_owned()));
        }
        if final_path.exists() {
            std::fs::remove_file(&final_path).map_err(transport)?;
        }
        std::fs::rename(&partial_path, &final_path).map_err(transport)?;
        Ok(ModelHandle { path: final_path })
    }
}

fn verify(path: &Path, expected: &str) -> Result<bool, DownloadError> {
    let mut file = File::open(path).map_err(transport)?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest).map_err(transport)?;
    Ok(format!("{:x}", digest.finalize()) == expected)
}

fn transport(error: impl std::fmt::Display) -> DownloadError {
    DownloadError::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_a_pinned_digest() {
        let path =
            std::env::temp_dir().join(format!("verbatim-downloader-hash-{}", std::process::id()));
        std::fs::write(&path, b"verbatim").expect("write fixture");
        assert!(
            verify(
                &path,
                "b91136286b50c5be49bd3fdbd00648a98aded623894ebd9debdaa91ad844ca5c"
            )
            .expect("verify")
        );
        assert!(
            !verify(
                &path,
                "0000000000000000000000000000000000000000000000000000000000000000"
            )
            .expect("reject")
        );
        let _ = std::fs::remove_file(path);
    }
}
