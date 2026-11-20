//! Host-owned durable checkpoint adapter for the portable agent kernel.
//! The agent SDK depends on the Journal trait, never on SQLite or AppState.
use crate::host::execution::jobs::JobContext;
use dominion_sdk::harness::{Journal,Snapshot};
use serde_json::json;

pub struct DurableJournal<'a> {pub(crate) context:&'a JobContext}

impl Journal for DurableJournal<'_> {
    fn save(&self, snapshot:&Snapshot)->Result<(),String> {
        let in_flight=snapshot.model_in_flight || !snapshot.in_flight.is_empty();
        let patch=json!({
            "nativeState":snapshot,
            "nativeInFlight":in_flight,
            "stepIndex":snapshot.step,
            "modelCalls":snapshot.model_calls,
            "estimatedTokens":snapshot.charged_tokens
        });
        // Ordinary checkpointing must refuse owner-cancelled jobs. A
        // *terminal cancelled snapshot* is the exception: persist proof that
        // generation was stopped without authorizing any further action.
        // This CAS requires an existing owner cancellation flag and cannot
        // clear it or reopen execution.
        if matches!(snapshot.status,dominion_sdk::harness::Status::Cancelled)
            && self.context.state.store.get("job",&self.context.job_id)
                .is_some_and(|job|job["cancelRequested"]==true)
        {
            let saved=self.context.state.store.patch_if("job",
                &self.context.job_id,"cancelRequested",&json!(true),&patch)
                .ok_or("Cancelled native task checkpoint could not be saved")?;
            self.context.state.bus.record("job",&saved);
            return Ok(());
        }
        self.context.checkpoint(patch)
    }
    fn cancelled(&self, id: &str)->bool {
        self.context.state.store.get("job",id).is_none_or(|job| {
            job["cancelRequested"]==true || job["status"]=="cancelled" ||
            job["sessionId"].as_str().is_some_and(|session|
                self.context.state.is_cancelled(session))
        })
    }
    fn directive(&self, id: &str)->Result<Option<String>,String> {
        let job=self.context.state.store.get("job",id)
            .ok_or("Running job missing while checking owner directive")?;
        Ok(job["nativeDirective"].as_str().map(str::to_string))
    }
}
