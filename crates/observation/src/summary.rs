use std::fmt;

use crate::schema::v1::DiagnosticLevelV1;
use crate::{RunId, StoredRun, UnixTimestampV1};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub schema_version: u32,
    pub run_id: RunId,
    pub vehicle_identity: String,
    pub created_at: UnixTimestampV1,
    pub session_started_at: UnixTimestampV1,
    pub diagnostic_count: usize,
    pub ledger_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
    pub first_recorded_at: Option<UnixTimestampV1>,
    pub last_recorded_at: Option<UnixTimestampV1>,
    pub initial_state: Option<&'static str>,
    pub final_state: Option<&'static str>,
    pub final_ledger_sequence: Option<u64>,
}

pub fn summarize(run: &StoredRun) -> RunSummary {
    let recorded_at = run
        .diagnostics
        .iter()
        .map(|record| &record.recorded_at)
        .chain(run.ledger.iter().map(|record| &record.recorded_at));

    let first_recorded_at = recorded_at.clone().min().cloned();
    let last_recorded_at = recorded_at.max().cloned();

    RunSummary {
        schema_version: run.manifest.schema_version,
        run_id: run.manifest.run_id.clone(),
        vehicle_identity: run.manifest.vehicle.identity.clone(),
        created_at: run.manifest.created_at,
        session_started_at: run.manifest.session_started_at,
        diagnostic_count: run.diagnostics.len(),
        ledger_count: run.ledger.len(),
        warning_count: run
            .diagnostics
            .iter()
            .filter(|record| record.payload.level == DiagnosticLevelV1::Warning)
            .count(),
        error_count: run
            .diagnostics
            .iter()
            .filter(|record| record.payload.level == DiagnosticLevelV1::Error)
            .count(),
        first_recorded_at,
        last_recorded_at,
        initial_state: run
            .ledger
            .first()
            .map(|record| record.payload.old_state.label()),
        final_state: run
            .ledger
            .last()
            .map(|record| record.payload.next_state.label()),
        final_ledger_sequence: run.ledger.last().map(|record| record.payload.record_seq),
    }
}

impl fmt::Display for RunSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "schema_version: {}", self.schema_version)?;
        writeln!(f, "run_id: {}", self.run_id)?;
        writeln!(f, "vehicle_identity: {}", self.vehicle_identity)?;
        writeln!(f, "created_at: {}", self.created_at)?;
        writeln!(f, "session_started_at: {}", self.session_started_at)?;
        writeln!(f, "diagnostic_count: {}", self.diagnostic_count)?;
        writeln!(f, "ledger_count: {}", self.ledger_count)?;
        writeln!(f, "warning_count: {}", self.warning_count)?;
        writeln!(f, "error_count: {}", self.error_count)?;
        writeln!(
            f,
            "first_recorded_at: {}",
            display_option(self.first_recorded_at)
        )?;
        writeln!(
            f,
            "last_recorded_at: {}",
            display_option(self.last_recorded_at)
        )?;
        writeln!(f, "initial_state: {}", display_option(self.initial_state))?;
        writeln!(f, "final_state: {}", display_option(self.final_state))?;
        writeln!(
            f,
            "final_ledger_sequence: {}",
            display_option(self.final_ledger_sequence)
        )
    }
}

fn display_option(value: Option<impl fmt::Display>) -> impl fmt::Display {
    value.map_or_else(|| "-".to_string(), |value| value.to_string())
}
