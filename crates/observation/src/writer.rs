//! Durable, non-overwriting run capture: manifest plus two flushed JSONL streams.
//!
//! `RunWriter` is the only way this crate creates a run directory. It never appends to or
//! overwrites an existing run — see `docs/superpowers/specs/2026-07-17-phase-3-observation-capture-design.md`.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use common::facade::{DiagnosticRecord, PublishedTransitionRecord};

use crate::ObservationError;
use crate::schema::CURRENT_SCHEMA_VERSION;
use crate::schema::v1::{
    ManifestV1, RunMetadata, StreamsV1, VehicleV1, diagnostic_envelope, ledger_envelope,
};

const MANIFEST_FILE_NAME: &str = "manifest.json";
const DIAGNOSTIC_FILE_NAME: &str = "diagnostic.jsonl";
const LEDGER_FILE_NAME: &str = "ledger.jsonl";

/// Owns one run directory's manifest and two open, flushed stream files.
#[derive(Debug)]
pub struct RunWriter {
    metadata: RunMetadata,
    run_dir: PathBuf,
    diagnostic: BufWriter<File>,
    ledger: BufWriter<File>,
}

impl RunWriter {
    /// Create `<parent>/<run-id>/` with `manifest.json`, `diagnostic.jsonl`, and `ledger.jsonl`.
    ///
    /// Fails with [`ObservationError::RunAlreadyExists`] if the run directory already exists;
    /// this writer never overwrites or appends to a previous run.
    ///
    /// If a later creation step fails after the run directory exists, the partial directory is
    /// intentionally retained. Removing it here could race with another process that has written
    /// files into that directory, so callers can inspect and clean up only after coordinating
    /// ownership.
    pub fn create(
        parent: impl AsRef<Path>,
        metadata: RunMetadata,
    ) -> Result<Self, ObservationError> {
        let parent = parent.as_ref();
        fs::create_dir_all(parent).map_err(|source| ObservationError::Io {
            path: parent.to_path_buf(),
            source,
        })?;

        let run_dir = parent.join(metadata.run_id.to_string());
        fs::create_dir(&run_dir).map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                ObservationError::RunAlreadyExists(run_dir.clone())
            } else {
                ObservationError::Io {
                    path: run_dir.clone(),
                    source,
                }
            }
        })?;

        let manifest = ManifestV1 {
            schema_version: CURRENT_SCHEMA_VERSION,
            run_id: metadata.run_id.clone(),
            created_at: metadata.created_at,
            session_started_at: metadata.session_started_at,
            vehicle: VehicleV1 {
                identity: metadata.vehicle_identity.clone(),
            },
            scenario: metadata.scenario.clone(),
            streams: StreamsV1 {
                diagnostic: DIAGNOSTIC_FILE_NAME.to_string(),
                ledger: LEDGER_FILE_NAME.to_string(),
            },
        };
        write_manifest(&run_dir.join(MANIFEST_FILE_NAME), &manifest)?;

        let diagnostic = BufWriter::new(open_new_file(&run_dir.join(DIAGNOSTIC_FILE_NAME))?);
        let ledger = BufWriter::new(open_new_file(&run_dir.join(LEDGER_FILE_NAME))?);

        Ok(Self {
            metadata,
            run_dir,
            diagnostic,
            ledger,
        })
    }

    /// The created run directory: `<parent>/<run-id>/`.
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Project, serialize, and durably append one diagnostic row.
    pub fn record_diagnostic(&mut self, record: &DiagnosticRecord) -> Result<(), ObservationError> {
        let envelope = diagnostic_envelope(&self.metadata, record)?;
        let path = self.run_dir.join(DIAGNOSTIC_FILE_NAME);
        write_json_line(&mut self.diagnostic, &path, &envelope)
    }

    /// Project, serialize, and durably append one ledger row.
    pub fn record_ledger(
        &mut self,
        record: &PublishedTransitionRecord,
    ) -> Result<(), ObservationError> {
        let envelope = ledger_envelope(&self.metadata, record)?;
        let path = self.run_dir.join(LEDGER_FILE_NAME);
        write_json_line(&mut self.ledger, &path, &envelope)
    }

    /// Flush both streams. Every row is already flushed by `record_*`, so this guards against
    /// forgetting a flush rather than doing new work.
    pub fn finish(mut self) -> Result<(), ObservationError> {
        let diagnostic_path = self.run_dir.join(DIAGNOSTIC_FILE_NAME);
        self.diagnostic
            .flush()
            .map_err(|source| ObservationError::Io {
                path: diagnostic_path,
                source,
            })?;

        let ledger_path = self.run_dir.join(LEDGER_FILE_NAME);
        self.ledger.flush().map_err(|source| ObservationError::Io {
            path: ledger_path,
            source,
        })?;

        Ok(())
    }
}

fn open_new_file(path: &Path) -> Result<File, ObservationError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| ObservationError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn write_manifest(path: &Path, manifest: &ManifestV1) -> Result<(), ObservationError> {
    let mut writer = BufWriter::new(open_new_file(path)?);
    serde_json::to_writer_pretty(&mut writer, manifest).map_err(|source| {
        ObservationError::Json {
            path: path.to_path_buf(),
            source,
        }
    })?;
    writer
        .write_all(b"\n")
        .map_err(|source| ObservationError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    writer.flush().map_err(|source| ObservationError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn write_json_line<T: Serialize>(
    writer: &mut BufWriter<File>,
    path: &Path,
    value: &T,
) -> Result<(), ObservationError> {
    serde_json::to_writer(&mut *writer, value).map_err(|source| ObservationError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    writer
        .write_all(b"\n")
        .map_err(|source| ObservationError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    writer.flush().map_err(|source| ObservationError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}
