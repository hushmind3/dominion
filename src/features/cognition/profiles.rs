//! Host-owned algorithm profiles, never a runtime or database dependency.
//! Each job pins a named version and immutable implementation revision.
use dominion_sdk::harness::{AgentAlgorithm,FocusedAlgorithm,StandardAlgorithm};
use serde_json::{json,Value};

pub const DEFAULT:&str="standard-v1";
pub const FOCUSED:&str="focused-v1";
const STANDARD_REVISION:&str="standard-v1.2026-11-20-r1";
const FOCUSED_REVISION:&str="focused-v1.2026-11-20-r1";
static DEFAULT_IMPL:StandardAlgorithm=StandardAlgorithm;
static FOCUSED_IMPL:FocusedAlgorithm=FocusedAlgorithm;

#[derive(Clone,Copy)]
pub struct Selected {
    pub id:&'static str,
    pub revision:&'static str,
    pub algorithm:&'static dyn AgentAlgorithm,
}
impl Selected {
    pub fn binding(self)->Value {
        json!({"format":1,"id":self.id,"revision":self.revision})
    }
}
pub fn supported(id:&str)->bool {matches!(id,DEFAULT|FOCUSED)}
pub fn select(id:&str)->Result<Selected,String> {
    let value=match id {
        DEFAULT=>Selected{id:DEFAULT,revision:STANDARD_REVISION,algorithm:&DEFAULT_IMPL},
        FOCUSED=>Selected{id:FOCUSED,revision:FOCUSED_REVISION,algorithm:&FOCUSED_IMPL},
        _=>return Err(format!("Unknown agent algorithm profile: {id}")),
    };
    if value.algorithm.id()!=value.id {
        return Err("Algorithm registry identity mismatch".into());
    }
    Ok(value)
}
/// Missing markers are treated as legacy standard, never silently upgraded to
/// focused. An existing marker must match ID, revision and schema exactly.
pub fn for_job(job:&Value)->Result<Selected,String> {
    let requested=match job.pointer("/input/algorithmId") {
        None|Some(Value::Null)=>DEFAULT,
        Some(Value::String(value))=>value.as_str(),
        _=>return Err("Algorithm ID must be a string".into()),
    };
    let chosen=select(requested)?;
    if !job["nativeAlgorithm"].is_null() && job["nativeAlgorithm"]!=chosen.binding(){
        return Err("Pinned agent algorithm ID or implementation revision changed; review required".into());
    }
    if job["nativeAlgorithm"].is_null() && job["nativeState"].is_object()
       && chosen.id!=DEFAULT {
        return Err("Legacy unpinned checkpoint cannot adopt a new algorithm".into());
    }
    Ok(chosen)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn profiles_are_explicit_and_not_arbitrary_user_code() {
        assert!(supported(DEFAULT)&&supported(FOCUSED));
        assert!(!supported("third-party"));
        assert_eq!(select(DEFAULT).unwrap().algorithm.id(),DEFAULT);
        assert_eq!(select(FOCUSED).unwrap().algorithm.id(),FOCUSED);
        assert!(select("not-installed").is_err());
    }
    #[test] fn revision_and_identity_are_pinned_on_recovery(){
        let raw=json!({"input":{"algorithmId":FOCUSED}});
        let bound=for_job(&raw).unwrap().binding();
        let job=json!({"input":{"algorithmId":FOCUSED},
            "nativeAlgorithm":bound,"nativeState":{"status":"running"}});
        assert_eq!(for_job(&job).unwrap().id,FOCUSED);
        let mut altered=job.clone();
        altered["nativeAlgorithm"]["revision"]=json!("new-implementation");
        assert!(for_job(&altered).is_err());
        altered=job;
        altered["input"]["algorithmId"]=json!(DEFAULT);
        assert!(for_job(&altered).is_err());
    }
    #[test] fn legacy_snapshots_only_allow_default(){
        let old=json!({"input":{"prompt":"old"},"nativeState":{"status":"running"}});
        assert_eq!(for_job(&old).unwrap().id,DEFAULT);
        let mut old=old;
        old["input"]["algorithmId"]=json!(FOCUSED);
        assert!(for_job(&old).is_err());
    }
}
