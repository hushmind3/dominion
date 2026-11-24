//! Typed HTTP integration with the running product; no local execution.
use crate::{RunEventPage, RunEventRef};
use dominion_extensions::{Manifest,Registration,SDK_SOURCE_KIND};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{json, Value};
use std::{fmt, time::Duration};
use tokio::time::{Instant, timeout_at, sleep};

#[derive(Debug)]
pub enum ClientError {
    Invalid(String),
    Transport(reqwest::Error),
    Api { status: u16, message: String },
    Timeout,
    Unverified { status: RunStatus, outcome: Option<String> },
}
impl fmt::Display for ClientError {
    fn fmt(&self, f:&mut fmt::Formatter<'_>)->fmt::Result {
        match self {
            Self::Invalid(s)=>f.write_str(s),
            Self::Transport(e)=>write!(f,"{e}"),
            Self::Api{status,message}=>write!(f,"HTTP {status}: {message}"),
            Self::Timeout=>f.write_str("Wait expired; the durable run was not cancelled"),
            Self::Unverified{status,outcome}=>write!(f,"Unverified task: {status:?} ({outcome:?})"),
        }
    }
}
impl std::error::Error for ClientError {}
impl From<reqwest::Error> for ClientError { fn from(e:reqwest::Error)->Self { Self::Transport(e) } }

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all="snake_case")]
pub enum RunStatus {
    Queued, Running, AwaitingApproval, InDoubt, Interrupted, NeedsReview,
    Completed, Cancelled, Exhausted, Denied, Failed,
    #[serde(other)] Unknown,
}
impl RunStatus {
    /// Unknown future states are returned to the caller for review, not polled forever.
    pub fn settled(&self)->bool { !matches!(self,Self::Queued|Self::Running) }
}
#[derive(Clone,Debug,Default,Deserialize,Serialize)]
pub struct RunActions {
    #[serde(default)] pub approve:bool,
    #[serde(default)] pub deny:bool,
    #[serde(default)] pub cancel:bool,
    #[serde(default)] pub resume:bool,
    #[serde(default)] pub reconcile:bool,
    #[serde(default)] pub steer:bool,
    #[serde(default)] pub refresh:bool,
}
#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ExtensionSnapshot { pub id:String,pub version:String,pub schema_version:u32 }
#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(rename_all="camelCase")]
pub struct AgentRun {
    pub id:String,
    pub status:RunStatus,
    pub instruction:String,
    #[serde(default)] pub extension:Option<ExtensionSnapshot>,
    #[serde(default)] pub provider_id:Option<String>,
    #[serde(default)] pub model:Option<String>,
    #[serde(default)] pub session_id:Option<String>,
    #[serde(default)] pub answer:Option<String>,
    #[serde(default)] pub error:Option<String>,
    #[serde(default)] pub task_outcome:Option<String>,
    #[serde(default)] pub verified:Option<bool>,
    #[serde(default)] pub acceptance_passed:Option<bool>,
    #[serde(default)] pub failure_class:Option<String>,
    #[serde(default)] pub actions:Option<RunActions>,
    #[serde(default)] pub checkpoint:Option<Value>,
    #[serde(default)] pub run_ref:Option<RunEventRef>,
    #[serde(default)] pub evidence_execution_id:Option<String>,
    #[serde(default)] pub uncertain_effect_count:Option<u64>,
    #[serde(default)] pub read_only:bool,
}
impl AgentRun {
    pub fn verified_success(&self)->bool {
        self.status==RunStatus::Completed && self.verified==Some(true)
            && self.task_outcome.as_deref()==Some("succeeded")
    }
}
#[derive(Clone,Debug,Deserialize,Serialize)]
pub struct SourceRef { pub kind:String, pub id:String }

#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(rename_all="camelCase")]
pub struct RunSummary {
    pub id:String,
    pub status:RunStatus,
    #[serde(default)] pub session_id:Option<String>,
    #[serde(default)] pub source:Option<Value>,
    #[serde(default)] pub task_outcome:Option<String>,
    #[serde(default)] pub verified:Option<bool>,
    #[serde(default)] pub created_at:Option<String>,
}
#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(rename_all="camelCase")]
pub struct InferenceRoute { pub provider_id:String, pub model:String }
#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(rename_all="camelCase")]
pub struct InferenceSupport {
    pub provider_id:String,
    pub model:String,
    pub thinking_toggle:bool,
    pub reasoning_efforts:Vec<String>,
}
#[derive(Clone,Debug,Default,Serialize)]
#[serde(rename_all="camelCase")]
pub struct StartRun {
    pub instruction:String,
    #[serde(skip_serializing_if="Option::is_none")] pub provider_id:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] pub model:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] pub agent_id:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] pub source:Option<SourceRef>,
    #[serde(skip_serializing_if="Option::is_none")] pub idempotency_key:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] pub verify_command:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] pub max_rounds:Option<usize>,
    #[serde(skip_serializing_if="Option::is_none")] pub token_budget:Option<u64>,
    #[serde(skip_serializing_if="Option::is_none")] pub budget_usd:Option<f64>,
    #[serde(skip_serializing_if="Option::is_none")] pub workspace_dir:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] pub shell_sandbox:Option<String>,
}
#[derive(Clone)]
pub struct Client {
    base:Url,
    http:reqwest::Client,
    token:Option<String>,
}
impl Client {
    pub fn connect(base:&str)->Result<Self,ClientError>{
        let mut base=Url::parse(base).map_err(|e|ClientError::Invalid(e.to_string()))?;
        if !matches!(base.scheme(),"http"|"https")||base.host_str().is_none()
            ||!base.username().is_empty()||base.password().is_some() {
            return Err(ClientError::Invalid("Expected an HTTP(S) base URL without credentials".into()));
        }
        base.set_query(None);base.set_fragment(None);
        let http=reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30)).build()?;
        Ok(Self{base,http,token:None})
    }
    pub fn with_admin_token(mut self,token:impl Into<String>)->Self{self.token=Some(token.into());self}
    fn url(&self,segments:&[&str])->Result<Url,ClientError>{
        if segments.first()!=Some(&"api")||segments.iter().any(|s|s.is_empty()||*s=="."||*s==".."){
            return Err(ClientError::Invalid("Invalid API path segment".into()));
        }
        let mut url=self.base.clone();
        url.path_segments_mut().map_err(|_|ClientError::Invalid("Invalid base URL".into()))?
            .pop_if_empty().extend(segments.iter().copied());
        Ok(url)
    }
    async fn request<T:DeserializeOwned>(&self,method:Method,url:Url,body:Option<Value>)->Result<T,ClientError>{
        let mut req=self.http.request(method,url).header("Accept","application/json");
        if let Some(token)=&self.token{req=req.header("X-Dominion-Token",token);}
        if let Some(body)=body{req=req.json(&body);}
        let response=req.send().await?;
        if !response.status().is_success(){
            let status=response.status().as_u16();
            let message=response.json::<Value>().await.ok()
                .and_then(|v|v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(||format!("HTTP {status}"));
            return Err(ClientError::Api{status,message});
        }
        Ok(response.json().await?)
    }
    async fn get<T:DeserializeOwned>(&self,path:&[&str])->Result<T,ClientError>{
        self.request(Method::GET,self.url(path)?,None).await
    }
    async fn post<T:DeserializeOwned>(&self,path:&[&str],body:Value)->Result<T,ClientError>{
        self.request(Method::POST,self.url(path)?,Some(body)).await
    }
    pub async fn list_runs(&self)->Result<Vec<AgentRun>,ClientError>{self.get(&["api","sdk","runs"]).await}
    pub async fn runs_by_source(&self,kind:&str)->Result<Vec<RunSummary>,ClientError>{
        self.get(&["api","sdk","runs","source",kind]).await
    }
    pub async fn inference_support(&self,route:&InferenceRoute)->Result<InferenceSupport,ClientError>{
        let mut url=self.url(&["api","chat","inference-support"])?;
        url.query_pairs_mut().append_pair("providerId",&route.provider_id).append_pair("model",&route.model);
        self.request(Method::GET,url,None).await
    }
    pub async fn inference_support_batch(&self,routes:&[InferenceRoute])->Result<Vec<InferenceSupport>,ClientError>{
        let value=serde_json::to_value(routes).map_err(|e|ClientError::Invalid(e.to_string()))?;
        self.post(&["api","chat","inference-support"],value).await
    }
    pub async fn get_run(&self,id:&str)->Result<AgentRun,ClientError>{self.get(&["api","sdk","runs",id]).await}
    pub async fn list_extensions(&self)->Result<Vec<Registration>,ClientError>{self.get(&["api","extensions","apps"]).await}
    pub async fn register_extension(&self,manifest:&Manifest)->Result<Registration,ClientError>{
        let body=serde_json::to_value(manifest).map_err(|e|ClientError::Invalid(e.to_string()))?;
        self.post(&["api","extensions","apps"],body).await
    }
    pub async fn set_extension_enabled(&self,id:&str,enabled:bool)->Result<Registration,ClientError>{
        self.post(&["api","extensions","apps",id,"state"],json!({"enabled":enabled})).await
    }
    pub async fn unregister_extension(&self,id:&str)->Result<(),ClientError>{
        let _:Value=self.request(Method::DELETE,self.url(&["api","extensions","apps",id])?,None).await?;Ok(())
    }
    pub async fn start_extension_run(&self,id:&str,input:&StartRun)->Result<AgentRun,ClientError>{
        let mut request=input.clone();
        request.source=Some(SourceRef{kind:SDK_SOURCE_KIND.into(),id:id.into()});
        self.start_run(&request).await
    }
    pub async fn start_run(&self,input:&StartRun)->Result<AgentRun,ClientError>{
        let body=serde_json::to_value(input).map_err(|e|ClientError::Invalid(e.to_string()))?;
        self.post(&["api","sdk","runs"],body).await
    }
    pub async fn approve_call(&self,id:&str,call_id:&str)->Result<AgentRun,ClientError>{
        self.post(&["api","sdk","runs",id,"approve"],json!({"callId":call_id})).await
    }
    /// Denial uses the same owner approval endpoint as the product UI.
    pub async fn deny_call(&self,id:&str,call_id:&str)->Result<AgentRun,ClientError>{
        let current=self.get_run(id).await?;
        if !current.actions.as_ref().is_some_and(|a|a.deny)
            ||current.checkpoint.as_ref().and_then(|c|c["pendingCall"]["id"].as_str())!=Some(call_id){
            return Err(ClientError::Invalid("Not the run's pending approval".into()));
        }
        let result:Value=self.post(&["api","approvals"],json!({"id":call_id,"allow":false})).await?;
        if result["status"]!="denied"{return Err(ClientError::Api{status:409,message:"Approval was not denied".into()});}
        self.get_run(id).await
    }
    pub async fn cancel_run(&self,id:&str)->Result<AgentRun,ClientError>{self.post(&["api","sdk","runs",id,"cancel"],json!({})).await}
    pub async fn resume_run(&self,id:&str)->Result<AgentRun,ClientError>{self.post(&["api","sdk","runs",id,"resume"],json!({})).await}
    pub async fn reconcile_run(&self,id:&str,applied:bool,details:&str)->Result<AgentRun,ClientError>{
        self.post(&["api","sdk","runs",id,"reconcile"],json!({"applied":applied,"details":details})).await
    }
    pub async fn steer_run(&self,id:&str,instruction:&str,mode:&str)->Result<Value,ClientError>{
        self.post(&["api","sdk","runs",id,"steer"],json!({"instruction":instruction,"mode":mode})).await
    }
    pub async fn execution_events(&self,id:&str,after:u64,limit:u32)->Result<RunEventPage,ClientError>{
        let mut url=self.url(&["api","executions",id,"events"])?;
        url.query_pairs_mut().append_pair("after",&after.to_string()).append_pair("limit",&limit.to_string());
        self.request(Method::GET,url,None).await
    }
    pub async fn run_events(&self,id:&str,after:u64,limit:u32)->Result<RunEventPage,ClientError>{
        let run=self.get_run(id).await?;
        let execution_id=run.run_ref.as_ref().map(|r|r.execution_id.as_str()).or(run.evidence_execution_id.as_deref())
            .ok_or_else(||ClientError::Api{status:409,message:"Native execution has not been admitted".into()})?;
        self.execution_events(execution_id,after,limit).await
    }
    /// Deadline includes network time. Dropping the future never cancels server work.
    pub async fn wait_for_run(&self,id:&str,max_wait:Duration,interval:Duration,mut on_update:impl FnMut(&AgentRun))->Result<AgentRun,ClientError>{
        let deadline=Instant::now().checked_add(max_wait).ok_or_else(||ClientError::Invalid("Invalid deadline".into()))?;
        timeout_at(deadline,async{
            loop{
                let run=self.get_run(id).await?;
                on_update(&run);
                if run.status.settled(){return Ok(run);}
                sleep(interval.max(Duration::from_millis(50))).await;
            }
        }).await.map_err(|_|ClientError::Timeout)?
    }
    pub async fn wait_for_verified_run(&self,id:&str,max_wait:Duration,interval:Duration,on_update:impl FnMut(&AgentRun))->Result<AgentRun,ClientError>{
        let run=self.wait_for_run(id,max_wait,interval,on_update).await?;
        if run.verified_success(){Ok(run)}else{Err(ClientError::Unverified{status:run.status,outcome:run.task_outcome})}
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn owner_review_and_future_status_stop_waiting(){
        for state in ["needs_review","awaiting_approval","in_doubt","interrupted","future_state"]{
            let run:AgentRun=serde_json::from_value(json!({"id":"r","instruction":"goal","status":state})).unwrap();
            assert!(run.status.settled());
            assert!(!run.verified_success());
        }
    }
    #[test]
    fn only_native_verified_success_counts(){
        let mut run:AgentRun=serde_json::from_value(json!({"id":"r","instruction":"goal","status":"completed","verified":true,"taskOutcome":"answered_unverified"})).unwrap();
        assert!(!run.verified_success());
        run.task_outcome=Some("succeeded".into());
        assert!(run.verified_success());
    }
    #[test]
    fn path_segments_and_retry_identity_are_preserved(){
        let client=Client::connect("http://127.0.0.1:4317/prefix/").unwrap();
        assert_eq!(client.url(&["api","sdk","runs","a/b"]).unwrap().path(),"/prefix/api/sdk/runs/a%2Fb");
        assert!(client.url(&["api",".."]).is_err());
        let input=StartRun{instruction:"goal".into(),idempotency_key:Some("retry-1".into()),..Default::default()};
        let value=serde_json::to_value(input).unwrap();
        assert_eq!(value["idempotencyKey"],"retry-1");
        assert!(value.get("providerId").is_none());
    }
}
