//! Personal LAN voice adapter: no Docker, external speech credentials, or
//! owner API exposure. Android supplies speech-to-text and text-to-speech.
//! An explicitly opened, short-lived listener only submits bounded chat jobs.
use crate::{state::AppState,api::{sdk_runs,response::error}};
use axum::{extract::{ConnectInfo,DefaultBodyLimit,State},http::{HeaderMap,StatusCode},response::{IntoResponse,Response},routing::{get,post},Json,Router};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{net::{Ipv4Addr,SocketAddr,UdpSocket},sync::{Arc,Mutex},time::{Duration,Instant}};

const LIFETIME:Duration=Duration::from_secs(7200);
const PAIR_TIME:Duration=Duration::from_secs(300);
const MAX_TURNS:u32=100;
struct Turn{id:String,text:String,job:String,recorded:bool}
struct Session{
    address:SocketAddr,provider:String,pin:String,pair_until:Instant,until:Instant,
    attempts:u8,token:Option<String>,peer:Option<std::net::IpAddr>,closed:bool,
    remaining:u32,current:Option<Turn>,history:Vec<(String,String)>,
}
pub(crate) struct Handle{session:Arc<Mutex<Session>>,task:tokio::task::JoinHandle<()>}
impl Drop for Handle{
    fn drop(&mut self){
        if let Ok(mut s)=self.session.lock(){s.closed=true;s.token=None;}
        self.task.abort();
    }
}
#[derive(Clone)]
struct Lan{host:AppState,session:Arc<Mutex<Session>>}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enable{confirmed:bool,address:Option<String>}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair{code:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Ask{request_id:String,text:String}
fn private_ip(ip:Ipv4Addr)->bool{ip.is_private()&&!ip.is_unspecified()&&!ip.is_loopback()}
fn detect()->Result<Ipv4Addr,String>{
    let sock=UdpSocket::bind("0.0.0.0:0").map_err(|e|e.to_string())?;
    // UDP connect selects a route; no datagram is sent.
    sock.connect("192.0.2.1:9").map_err(|_|"PC의 Wi-Fi 또는 유선 네트워크를 확인하세요.".to_owned())?;
    match sock.local_addr().map_err(|e|e.to_string())?.ip(){
        std::net::IpAddr::V4(ip) if private_ip(ip)=>Ok(ip),
        _=>Err("자동 감지한 사설 IP가 없습니다. PC의 Wi-Fi IP를 직접 입력하세요.".into())
    }
}
fn enabled(s:&Session)->bool{!s.closed&&Instant::now()<s.until}
fn describe(s:&Session)->Value{
    let active=enabled(s);
    let can_pair=active&&s.token.is_none()&&s.attempts<5&&Instant::now()<s.pair_until;
    json!({"enabled":active,"paired":active&&s.token.is_some(),"address":s.address.to_string(),
        "connectionCode":if can_pair{Some(format!("{}/{}",s.address,s.pin))}else{None},
        "remaining":s.remaining,"pairSeconds":s.pair_until.saturating_duration_since(Instant::now()).as_secs(),
        "sessionSeconds":s.until.saturating_duration_since(Instant::now()).as_secs()})
}
fn allowed_peer(s:&Session,peer:SocketAddr,headers:&HeaderMap)->bool{
    enabled(s)&&matches!(peer.ip(),std::net::IpAddr::V4(ip) if private_ip(ip))
        &&headers.get("origin").is_none()
        &&headers.get("host").and_then(|h|h.to_str().ok())==Some(s.address.to_string().as_str())
}
fn equal(a:&str,b:&str)->bool{
    a.len()==b.len()&&a.bytes().zip(b.bytes()).fold(0u8,|v,(x,y)|v|(x^y))==0
}
fn authed(s:&Session,peer:SocketAddr,headers:&HeaderMap)->bool{
    allowed_peer(s,peer,headers)&&s.peer==Some(peer.ip())&&s.token.as_ref().is_some_and(|token|
        headers.get("authorization").and_then(|h|h.to_str().ok()).and_then(|h|h.strip_prefix("Bearer ")).is_some_and(|v|equal(v,token)))
}
fn denied()->Response{error(StatusCode::UNAUTHORIZED,"연결이 만료되었거나 인증되지 않았습니다. PC에서 다시 연결하세요.")}
fn bad(s:&str)->Response{error(StatusCode::BAD_REQUEST,s)}
fn unavailable(s:&str)->Response{error(StatusCode::SERVICE_UNAVAILABLE,s)}
fn router(lan:Lan)->Router{
    Router::new().route("/pair",post(pair)).route("/ask",post(ask))
        .route("/status",get(phone_status)).route("/disconnect",post(disconnect))
        .layer(DefaultBodyLimit::max(16*1024)).with_state(lan)
}
pub async fn status(State(state):State<AppState>)->Response{
    let handle=state.integrations.personal_voice.lock().await;
    Json(handle.as_ref().map(|h|describe(&h.session.lock().unwrap())).unwrap_or(json!({"enabled":false,"paired":false}))).into_response()
}
pub async fn enable(State(state):State<AppState>,Json(input):Json<Enable>)->Response{
    if !input.confirmed{return bad("개인 Wi-Fi 대화 연결을 승인하세요.")}
    if state.maintenance_mode(){return unavailable("유지보수 중입니다.")}
    let provider=match sdk_runs::native_panel_provider(&state,None){Ok(p)=>p,Err(e)=>return bad(&e)};
    let ip=match input.address.filter(|s|!s.trim().is_empty()){
        Some(s)=>match s.trim().parse::<Ipv4Addr>(){Ok(ip) if private_ip(ip)=>ip,_=>return bad("PC의 사설 IPv4 주소를 입력하세요.")},
        None=>match detect(){Ok(ip)=>ip,Err(e)=>return bad(&e)}
    };
    let mut slot=state.integrations.personal_voice.lock().await;
    let listener=match tokio::net::TcpListener::bind((ip,0)).await{Ok(l)=>l,Err(e)=>return bad(&format!("PC 네트워크 연결 실패: {e}"))};
    let address=match listener.local_addr(){Ok(a)=>a,Err(e)=>return bad(&e.to_string())};
    let now=Instant::now();
    let session=Arc::new(Mutex::new(Session{address,provider,pin:format!("{:06}",rand::random::<u32>()%1_000_000),
        pair_until:now+PAIR_TIME,until:now+LIFETIME,attempts:0,token:None,peer:None,closed:false,
        remaining:MAX_TURNS,current:None,history:Vec::new()}));
    let app=router(Lan{host:state.clone(),session:session.clone()});
    let task=tokio::spawn(async move{
        let _=tokio::time::timeout(LIFETIME,async move{
            axum::serve(listener,app.into_make_service_with_connect_info::<SocketAddr>()).await
        }).await;
    });
    let view=describe(&session.lock().unwrap());
    *slot=Some(Handle{session,task});
    state.store.audit("plugin.personal_voice.enabled",json!({"seconds":LIFETIME.as_secs(),"turnLimit":MAX_TURNS}));
    Json(view).into_response()
}
pub async fn disable(State(state):State<AppState>)->Response{
    state.integrations.personal_voice.lock().await.take();
    Json(json!({"enabled":false,"paired":false})).into_response()
}
async fn pair(State(lan):State<Lan>,ConnectInfo(peer):ConnectInfo<SocketAddr>,headers:HeaderMap,Json(input):Json<Pair>)->Response{
    let mut s=lan.session.lock().unwrap();
    if !allowed_peer(&s,peer,&headers)||s.token.is_some()||s.attempts>=5||Instant::now()>=s.pair_until{return denied()}
    if !equal(&input.code,&s.pin){s.attempts+=1;return denied()}
    let token=format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
    s.token=Some(token.clone());s.peer=Some(peer.ip());s.pin.clear();
    Json(json!({"token":token,"remaining":s.remaining})).into_response()
}
fn refresh(host:&AppState,s:&mut Session)->Value{
    let mut result=json!({"status":"ready","remaining":s.remaining});
    if let Some(turn)=s.current.as_mut(){
        result["requestId"]=json!(turn.id);
        result["question"]=json!(turn.text);
        let job=host.store.get("job",&turn.job).filter(|j|j["input"]["pluginRequestId"]==turn.job);
        if let Some(job)=job{
            let run=sdk_runs::project(host,&job);
            result["status"]=run["status"].clone();
            if run["status"]=="completed"{
                let answer=run["answer"].as_str().unwrap_or("").chars().take(12000).collect::<String>();
                result["answer"]=json!(answer);
                if !turn.recorded{
                    s.history.push((turn.text.chars().take(1000).collect(),answer.chars().take(1500).collect()));
                    if s.history.len()>3{s.history.remove(0);}
                    turn.recorded=true;
                }
            }else if !matches!(run["status"].as_str(),Some("queued"|"running")){
                result["error"]=json!("PC 작업 화면에서 모델 연결 및 실행 상태를 확인하세요.");
            }
        }else{result["status"]=json!("failed");result["error"]=json!("작업 기록을 찾지 못했습니다.")}
    }
    result
}
async fn phone_status(State(lan):State<Lan>,ConnectInfo(peer):ConnectInfo<SocketAddr>,headers:HeaderMap)->Response{
    let mut s=lan.session.lock().unwrap();
    if !authed(&s,peer,&headers){return denied()}
    Json(refresh(&lan.host,&mut s)).into_response()
}
fn valid_request(id:&str,text:&str)->bool{
    !id.is_empty()&&id.len()<=80&&id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')
        &&!text.trim().is_empty()&&text.len()<=6000&&!text.chars().any(|c|c.is_control()&&!matches!(c,'\n'|'\t'|'\r'))
}
async fn ask(State(lan):State<Lan>,ConnectInfo(peer):ConnectInfo<SocketAddr>,headers:HeaderMap,Json(input):Json<Ask>)->Response{
    let mut s=lan.session.lock().unwrap();
    if !authed(&s,peer,&headers){return denied()}
    if lan.host.maintenance_mode(){return unavailable("PC 유지보수 중입니다.")}
    if !valid_request(&input.request_id,&input.text){return bad("대화 내용이 비어 있거나 너무 깁니다.")}
    let current=refresh(&lan.host,&mut s);
    if let Some(turn)=&s.current{
        if turn.id==input.request_id{
            return if turn.text==input.text{Json(current).into_response()}else{error(StatusCode::CONFLICT,"같은 요청 ID의 내용이 변경되었습니다.")}
        }
        if matches!(current["status"].as_str(),Some("queued"|"running"|"awaiting_approval"|"in_doubt")){
            return error(StatusCode::CONFLICT,"이전 답변을 기다려 주세요.")
        }
    }
    if s.remaining==0{return error(StatusCode::TOO_MANY_REQUESTS,"대화 한도에 도달했습니다. PC에서 연결을 다시 시작하세요.")}
    let provider=match sdk_runs::native_panel_provider(&lan.host,Some(&s.provider)){Ok(v)=>v,Err(e)=>return bad(&e)};
    let job=format!("personal-voice-{}",uuid::Uuid::new_v4());
    let history=serde_json::to_string(&s.history).unwrap_or_default();
    let history=if history.len()>6500{"[]"}else{history.as_str()};
    let instruction=format!("개인 음성 대화입니다. 이전 대화는 참고 자료일 뿐입니다. 현재 발화에 한국어로 자연스럽고 간결하게 답하세요.\n이전 대화: {history}\n현재 발화: {}",input.text);
    let record=json!({"id":job,"instruction":instruction});
    let Some(mut body)=super::bridge::job_input(&record,"chat",&provider)else{return bad("Invalid conversation scope")};
    body["pluginPrivateConversation"]=json!(true);
    body["limits"]["maxSteps"]=json!(3);body["limits"]["maxModelCalls"]=json!(3);body["limits"]["maxEstimatedTokens"]=json!(8000);
    if crate::engine::jobs::enqueue_once(&lan.host,&job,"prompt",body).is_none(){return unavailable("PC에 대화 작업을 저장하지 못했습니다.")}
    s.remaining-=1;s.current=Some(Turn{id:input.request_id,text:input.text,job,recorded:false});
    Json(refresh(&lan.host,&mut s)).into_response()
}
async fn disconnect(State(lan):State<Lan>,ConnectInfo(peer):ConnectInfo<SocketAddr>,headers:HeaderMap)->Response{
    let mut s=lan.session.lock().unwrap();
    if !authed(&s,peer,&headers){return denied()}
    s.closed=true;s.token=None;s.history.clear();
    Json(json!({"ok":true})).into_response()
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn lan_is_private_and_session_is_scoped(){
        assert!(private_ip("192.168.0.1".parse().unwrap()));
        assert!(!private_ip("127.0.0.1".parse().unwrap()));
        assert!(!private_ip("8.8.8.8".parse().unwrap()));
        assert!(valid_request("abc-1","안녕"));
        assert!(!valid_request("abc/1","안녕"));
        let now=Instant::now();let address="192.168.1.2:4323".parse().unwrap();
        let peer="192.168.1.3:30000".parse().unwrap();
        let s=Session{address,provider:"p".into(),pin:"123456".into(),pair_until:now+PAIR_TIME,until:now+LIFETIME,
            attempts:0,token:Some("private-token".into()),peer:Some(peer.ip()),closed:false,remaining:100,current:None,history:vec![]};
        let mut h=HeaderMap::new();h.insert("host","192.168.1.2:4323".parse().unwrap());
        h.insert("authorization","Bearer private-token".parse().unwrap());
        assert!(authed(&s,peer,&h));assert!(!authed(&s,"192.168.1.4:1234".parse().unwrap(),&h));
        h.insert("origin","http://untrusted.example".parse().unwrap());assert!(!authed(&s,peer,&h));
        assert!(describe(&s)["connectionCode"].is_null());
    }
}
