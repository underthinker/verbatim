//! Resumable, hash-verified model downloads.
//!
//! This crate is deliberately the only production component allowed to own an
//! HTTP client. Bytes first land in a `.part` file, are verified against the
//! catalog's pinned SHA-256, then atomically renamed into the model store.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::RANGE;
use sha2::{Digest, Sha256};
use verbatim_engines::ModelHandle;
use verbatim_engines::model::{DownloadError, ModelDownloader, ModelSpec, ProgressSink};

pub struct HttpModelDownloader {
    models_dir: PathBuf,
    client: Client,
    /// Both onboarding and Settings share this downloader. Only one transfer
    /// may mutate the model store at a time, or two commands can corrupt the
    /// same `.part` file (and race the final rename).
    download_lock: Mutex<()>,
}

impl HttpModelDownloader {
    pub fn new(models_dir: PathBuf) -> Result<Self, DownloadError> {
        let client = Client::builder()
            .user_agent(concat!("Verbatim/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(transport)?;
        Ok(Self {
            models_dir,
            client,
            download_lock: Mutex::new(()),
        })
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
        let _download_guard = self.download_lock.try_lock().map_err(|_| {
            DownloadError::Transport("another model download is already in progress".to_owned())
        })?;
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
        let mut last_progress = Instant::now();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = response.read(&mut buffer).map_err(transport)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count]).map_err(transport)?;
            received = received.saturating_add(count as u64);
            // Do not emit thousands of events into the webview during a fast
            // transfer. Ten updates per second remains visually smooth while
            // leaving the app event loop responsive.
            if received >= spec.size_bytes || last_progress.elapsed() >= Duration::from_millis(100)
            {
                progress(received.min(spec.size_bytes), spec.size_bytes);
                last_progress = Instant::now();
            }
        }
        file.sync_all().map_err(transport)?;

        activate_download(&partial_path, &final_path, spec, received)
    }
}

fn activate_download(
    partial_path: &Path,
    final_path: &Path,
    spec: &ModelSpec,
    received: u64,
) -> Result<ModelHandle, DownloadError> {
    if received != spec.size_bytes {
        // A clean-but-early EOF is still resumable. Keep the partial file just
        // as we do for a transport error instead of throwing away a nearly
        // complete model.
        return Err(DownloadError::Transport(format!(
            "download ended after {received} of {} bytes",
            spec.size_bytes
        )));
    }
    if !verify(partial_path, spec.sha256)? {
        let _ = std::fs::remove_file(partial_path);
        return Err(DownloadError::HashMismatch(spec.id.to_owned()));
    }
    if final_path.exists() {
        std::fs::remove_file(final_path).map_err(transport)?;
    }
    std::fs::rename(partial_path, final_path).map_err(transport)?;
    Ok(ModelHandle {
        path: final_path.to_owned(),
    })
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

    fn fixture_spec(size_bytes: u64, sha256: &'static str) -> ModelSpec {
        ModelSpec {
            id: "fixture",
            name: "Fixture",
            kind: verbatim_engines::model::ModelKind::Transcription,
            size_bytes,
            url: "https://example.invalid/model.bin",
            sha256,
            min_ram_gib: 1,
            license: "MIT",
            attribution: "test fixture",
        }
    }

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

    #[test]
    fn incomplete_download_remains_resumable() {
        let dir = std::env::temp_dir().join(format!(
            "verbatim-downloader-incomplete-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        let partial = dir.join("fixture.bin.part");
        let final_path = dir.join("fixture.bin");
        std::fs::write(&partial, b"partial").expect("write partial");

        let error = activate_download(
            &partial,
            &final_path,
            &fixture_spec(
                8,
                "b91136286b50c5be49bd3fdbd00648a98aded623894ebd9debdaa91ad844ca5c",
            ),
            7,
        )
        .expect_err("short transfer must fail");

        assert!(error.to_string().contains("7 of 8 bytes"));
        assert!(
            partial.exists(),
            "partial data must remain for Range resume"
        );
        assert!(!final_path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_download_is_rejected_before_network_or_disk_mutation() {
        let downloader = HttpModelDownloader::new(std::env::temp_dir())
            .expect("construct downloader for lock test");
        let _guard = downloader.download_lock.lock().expect("lock downloader");
        let progress = |_received: u64, _total: u64| {};

        let error = downloader
            .download(
                &fixture_spec(
                    8,
                    "b91136286b50c5be49bd3fdbd00648a98aded623894ebd9debdaa91ad844ca5c",
                ),
                &progress,
            )
            .expect_err("second download must be rejected");

        assert!(error.to_string().contains("already in progress"));
    }
}
