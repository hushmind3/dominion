//! Public embeddable Rust SDK over the same AppState, workers and SQLite as the GUI.
use crate::{config::Config,db::Store,engine,migrations,state::AppState,vault::Vault};
use serde_json::{json,Value};
use std::{path::PathBuf,sync::{atomic::{AtomicBool,Ordering},Arc},time::Duration};
use tokio::sync::broadcast;

#[derive(Clone,Debug)]
pub struct EngineOptions {
    pub workspace:PathBuf,pub data_dir:PathBuf,pub web_dir:PathBuf,pub port:u16,
}
impl EngineOptions {
    pub fn new(workspace:impl Into<PathBuf>,data_dir:impl Into<PathBuf>)->Self{
        let workspace=workspace.into();
        Self{web_dir:workspace.join("web/dist"),workspace,data_dir:data_dir.into(),port:4317}
    }
}
impl From<Config> for EngineOptions {
    fn from(c:Config)->Self{Self{workspace:c.workspace,data_dir:c.data_dir,web_dir:c.web_dir,port:c.port}}
}
#[derive(Clone,Debug)]
pub struct Prompt {
    pub instruction:String,pub agent_id:Option<String>,
    pub provider_id:Option<String>,pub model:Option<String>,
    pub reasoning_effort:Option<String>,pub verify_command:Option<String>,pub tools_enabled:bool,
    pub autonomous:bool,pub max_autonomous_attempts:u8,
    pub execution_strategy:ExecutionStrategy,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ExecutionStrategy { Focused, Conservative }
impl ExecutionStrategy {
    fn as_str(self)->&'static str{match self{
        Self::Focused=>"focused",Self::Conservative=>"conservative"
    }}
}
impl Prompt {
    pub fn new(instruction:impl Into<String>)->Self{
        Self{instruction:instruction.into(),agent_id:None,provider_id:None,model:None,
            reasoning_effort:None,verify_command:None,tools_enabled:true,autonomous:true,
            max_autonomous_attempts:3,execution_strategy:ExecutionStrategy::Focused}
    }
}

/// Rust is the engine. HTTP and TypeScript clients are presentation adapters.
#[derive(Clone)]
pub struct Dominion {
    state:AppState,
    started:Arc<AtomicBool>,
}
impl Dominion {
    pub fn open(options:EngineOptions)->Result<Self,String>{
        if !options.workspace.is_dir() {
            return Err(format!("workspace is not a directory: {}",options.workspace.display()));
        }
        std::fs::create_dir_all(&options.data_dir)
            .map_err(|error|format!("cannot create data directory: {error}"))?;
        let config=Config{port:options.port,data_dir:options.data_dir,
            web_dir:options.web_dir,workspace:options.workspace};
        let store=Store::open(&config.db_path()).map_err(|error|error.to_string())?;
        let vault=Vault::open(&config.key_path());
        let state=AppState::new(config,store,vault);
        migrations::run(&state)?;
        // CLI OAuth device waiters are process-local. The native SDK must
        // preserve the desktop's interrupted-login recovery behavior.
        if let Some(mut login)=state.store.get("codex_login","codex"){
            if matches!(login["status"].as_str(),Some("pending"|"device_pending")){
                login["status"]=json!("error");
                login["message"]=json!("서버가 재시작되어 로그인을 다시 시작해야 합니다.");
                state.store.put("codex_login",&login);
            }
        }
        Ok(Self{state,started:Arc::new(AtomicBool::new(false))})
    }
    pub(crate) fn from_config(config:Config)->Result<Self,String>{Self::open(config.into())}
    pub(crate) fn shared_state(&self)->AppState{self.state.clone()}

    /// Run the authoritative native engine background lifecycle once per process.
    pub fn start(&self)->Result<(),String>{
        tokio::runtime::Handle::try_current()
            .map_err(|_|"Dominion::start requires a Tokio runtime".to_string())?;
        if self.started.swap(true,Ordering::AcqRel){return Ok(());}
        engine::jobs::recover(&self.state);
        engine::wake::recover(&self.state);
        // Restore persisted completion evidence, not retired self-improvement
        // campaigns or autonomous policy/experiment generation.
        engine::improve::recover_finalizations(&self.state);
        engine::algorithms::seed(&self.state);
        engine::jobs::start(self.state.clone());
        engine::maintenance::start(self.state.clone());
        engine::scheduler::start(self.state.clone());
        engine::discord::start(self.state.clone());
        engine::telegram::start(self.state.clone());
        let refresh_state=self.state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(45)).await;
            loop {
                let _=crate::api::providers::refresh_free_catalog(&refresh_state).await;
                tokio::time::sleep(Duration::from_secs(6*60*60)).await;
            }
        });
        Ok(())
    }
    /// Write a native engine job without starting an external agent or API wrapper.
    pub fn submit(&self,prompt:Prompt)->Result<Value,String>{
        self.submit_with_options(prompt,None,None)
    }
    /// Run one task in a chosen existing repository without changing the
    /// global default for other jobs. A separate method retains source
    /// compatibility with callers constructing the existing Prompt struct.
    pub fn submit_in_workspace(&self,prompt:Prompt,workspace:impl Into<PathBuf>)
        ->Result<Value,String>{
        self.submit_with_options(prompt,Some(workspace.into()),None)
    }
    /// Per-job native execution options without introducing a second agent
    /// runtime or changing the public Prompt struct's fields.
    pub fn submit_with_options(
        &self,prompt:Prompt,workspace_dir:Option<PathBuf>,token_budget:Option<u64>
    )->Result<Value,String>{
        let instruction=prompt.instruction.trim();
        if instruction.is_empty()||instruction.len()>64_000 {
            return Err("instruction must contain 1..64000 bytes".into());
        }
        if token_budget.is_some_and(|amount|!(1024..=2_000_000).contains(&amount)){
            return Err("token_budget must be 1024..2000000".into());
        }
        let workspace=workspace_dir.as_ref()
            .map(|path|crate::engine::workspace_scope::resolve(&self.state,
                path.to_str().ok_or("workspace_dir must be Unicode")?))
            .transpose()?;
        if let Some(agent)=prompt.agent_id.as_deref(){
            if self.state.store.get("agent",agent).is_none(){
                return Err("unknown agent".into());
            }
        }
        if let Some(effort)=prompt.reasoning_effort.as_deref(){
            if !engine::reasoning::valid_effort(effort){return Err("invalid reasoning effort".into());}
        }
        if prompt.verify_command.as_deref().is_some_and(|s|s.len()>2048){
            return Err("verify_command must be at most 2048 bytes".into());
        }
        if !(1..=6).contains(&prompt.max_autonomous_attempts){
            return Err("max_autonomous_attempts must be 1..6".into());
        }
        let mut input=json!({
            "prompt":instruction,"toolsEnabled":prompt.tools_enabled,
            "autonomous":prompt.autonomous,
            "maxAutonomousAttempts":prompt.max_autonomous_attempts,
            "executionStrategy":prompt.execution_strategy.as_str(),
        });
        if let Some(path)=workspace{
            input["workspaceDir"]=json!(path.to_string_lossy().to_string());
        }
        if let Some(budget)=token_budget{
            input["tokenBudget"]=json!(budget);
        }
        for (name,value) in [
            ("agentId",prompt.agent_id),("providerId",prompt.provider_id),
            ("model",prompt.model),("reasoningEffort",prompt.reasoning_effort),
            ("verifyCommand",prompt.verify_command),
        ]{
            if let Some(value)=value.filter(|s|!s.trim().is_empty()){input[name]=json!(value);}
        }
        engine::jobs::enqueue(&self.state,"prompt",input)
            .ok_or_else(|| "cannot persist job".to_string())
    }
    /// Legacy archive remains readable without loading the retired media engine.
    pub fn archived_media_job(&self,id:&str)->Option<Value>{
        self.state.store.get("job",id)
            .filter(|job|matches!(job["kind"].as_str(),Some("video"|"youtube_upload")))
            .map(|job|crate::api::video::public_job(&job))
    }

    /// Claim a queued job through the same worker path, never a duplicate runner.
    pub fn schedule_now(&self,id:&str)->Result<(),String>{
        if self.state.store.get("job",id).is_none(){return Err("unknown job".into());}
        tokio::runtime::Handle::try_current()
            .map_err(|_|"schedule_now requires Tokio".to_string())?;
        engine::jobs::start_now(self.state.clone(),id.to_string());
        Ok(())
    }
    /// Persist a high-priority owner instruction for the next model turn.
    pub fn steer(&self,id:&str,instruction:&str)->Result<Value,String>{
        engine::jobs::steer(&self.state,id,instruction).map_err(|error|error.to_string())
    }
    pub fn job(&self,id:&str)->Option<Value>{self.state.store.get("job",id)}
    pub fn jobs(&self)->Vec<Value>{self.state.store.all("job")}
    pub fn session(&self,id:&str)->Option<Value>{self.state.store.get("session",id)}
    pub fn approvals(&self)->Vec<Value>{
        self.state.store.all("approval").into_iter()
            .filter(|r|matches!(r["status"].as_str(),Some("pending"|"interrupted")))
            .collect()
    }
    pub fn events_after(&self,seq:u64,limit:usize)->Vec<Value>{
        self.state.bus.replay(seq,limit.clamp(1,1000))
    }
    pub fn subscribe(&self)->broadcast::Receiver<Value>{self.state.bus.subscribe()}
    pub fn cancel_session(&self,id:&str){self.state.cancel_session(id)}
    pub fn preview_decomposition(&self,instruction:&str)->Value{
        engine::decompose::decompose(instruction,&json!({"implementation":"numbered-first"}))
    }
    pub fn acceptance_checks(&self,instruction:&str)->Value{
        engine::verification::prepare_acceptance_checks(&self.state,instruction)
    }
    pub fn verify(&self,instruction:&str,checks:&Value,evidence:&[Value])->Vec<String>{
        engine::verification::check_acceptance(&self.state,instruction,checks,evidence)
    }
    /// Read durable state after process restart; never resend a side-effecting turn.
    pub async fn wait(&self,id:&str,timeout:Duration)->Result<Value,String>{
        let began=tokio::time::Instant::now();
        loop {
            let job=self.job(id).ok_or("unknown job")?;
            if matches!(job["status"].as_str(),
                Some("completed"|"failed"|"cancelled"|"needs_review")){return Ok(job);}
            if timeout!=Duration::ZERO&&began.elapsed()>=timeout{
                return Err("job wait timed out; job remains persisted".into());
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sdk()->(Dominion,PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-rust-sdk-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let sdk=Dominion::open(EngineOptions::new(&dir,dir.join("data"))).unwrap();
        (sdk,dir)
    }
    #[test]
    fn native_client_persists_real_engine_jobs_and_events(){
        let (sdk,dir)=sdk();
        let job=sdk.submit(Prompt::new("Inspect the repository and prepare a patch")).unwrap();
        let id=job["id"].as_str().unwrap();
        assert_eq!(job["status"],"queued");
        assert_eq!(sdk.job(id).unwrap()["input"]["prompt"],"Inspect the repository and prepare a patch");
        let steering=sdk.steer(id,"Prioritize the real tests before reporting").unwrap();
        assert_eq!(steering["steeringRevision"],1);
        assert_eq!(sdk.job(id).unwrap()["steering"][0]["revision"],1);
        assert!(sdk.events_after(0,100).iter().any(|event|
            event["kind"]=="job"&&event["record"]["id"]==id));
        assert_eq!(sdk.clone().job(id).unwrap()["id"],id);
        drop(sdk);
        let reopened=Dominion::open(EngineOptions::new(&dir,dir.join("data"))).unwrap();
        assert_eq!(reopened.job(id).unwrap()["status"],"queued");
        drop(reopened);let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn native_sdk_exposes_same_frozen_verification_command_as_gui(){
        let (sdk,dir)=sdk();
        let mut prompt=Prompt::new("Fix the failing module");
        prompt.verify_command=Some("cargo test".into());
        let job=sdk.submit(prompt).unwrap();
        assert_eq!(job["input"]["verifyCommand"],"cargo test");
        drop(sdk);let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn invalid_owner_contract_cannot_enqueue_work(){
        let (sdk,dir)=sdk();
        assert!(sdk.submit(Prompt::new(" ")).is_err());
        let mut prompt=Prompt::new("Make a change");
        prompt.agent_id=Some("unknown-agent".into());
        assert!(sdk.submit(prompt).is_err());
        assert!(sdk.jobs().is_empty());
        drop(sdk);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn native_archived_video_receipt_never_starts_generation(){
        let (sdk,dir)=sdk();
        let job=sdk.state.store.put("job",&json!({"kind":"video",
            "status":"completed","input":{"title":"Old footage"}})).unwrap();
        let id=job["id"].as_str().unwrap();
        assert_eq!(sdk.archived_media_job(id).unwrap()["archived"],true);
        assert_eq!(sdk.archived_media_job(id).unwrap()["generationAvailable"],false);
        drop(sdk);let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn direct_native_preview_and_event_subscription(){
        let (sdk,dir)=sdk();
        let mut receiver=sdk.subscribe();
        let preview=sdk.preview_decomposition("1. Inspect source\n2. Edit Rust\n3. Run tests");
        assert_eq!(preview["subtasks"].as_array().unwrap().len(),3);
        let job=sdk.submit(Prompt::new("Review the code")).unwrap();
        let event=receiver.recv().await.unwrap();
        assert_eq!(event["record"]["id"],job["id"]);
        let waited=sdk.wait(job["id"].as_str().unwrap(),Duration::from_millis(1)).await;
        assert!(waited.is_err());
        drop(sdk);let _=std::fs::remove_dir_all(dir);
    }
}
