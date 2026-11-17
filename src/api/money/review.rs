//! Owner review, preserved as atomic versioned Money decisions.
//!
//! User decisions are data; no inference, side effects or approval grants.
//! The existing SQLite Store transaction binds a candidate revision to its
//! immutable decision record. No shadow database, scheduler or auth bypass.
use super::{decorate,fail,now,valid_won};
use crate::state::AppState;
use axum::{extract::{Path,Query,State},http::StatusCode,
    response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Map,Value};
use sha2::{Digest,Sha256};

#[derive(Deserialize,Default)]
pub struct ArchiveQuery {
    pub status:Option<String>,
    pub q:Option<String>,
    pub limit:Option<usize>,
}

/// Search includes rejected and postponed hypotheses. Never delete a
/// previously seen opportunity because the owner changed their mind.
pub async fn archive(
    State(state):State<AppState>,Query(query):Query<ArchiveQuery>,
)->Response{
    let status=query.status.as_deref().unwrap_or("archived");
    if !matches!(status,
        "archived"|"all"|"on_hold"|"rejected"|"hypothesis"|"researching"|
        "testing"|"validated"|"launched"|"implementation_requested"){
        return fail("지원하지 않는 후보 검색 상태입니다.");
    }
    let search=query.q.as_deref().unwrap_or("").trim();
    if search.chars().count()>160 {
        return fail("후보 검색어는 최대 160자입니다.");
    }
    match state.store.money_candidate_archive(status,search,query.limit.unwrap_or(50)) {
        Ok(found)=>Json(found).into_response(),
        Err(err)=>super::error(StatusCode::INTERNAL_SERVER_ERROR,&err)
    }
}

#[derive(Deserialize,Default)]
pub struct HistoryQuery {pub limit:Option<usize>}

/// Immutable decision history for one candidate, newest revision first.
pub async fn history(
    State(state):State<AppState>,Path(id):Path<String>,Query(query):Query<HistoryQuery>,
)->Response{
    if state.store.get("money_candidate",&id).is_none(){
        return super::error(StatusCode::NOT_FOUND,"사업 후보를 찾지 못했습니다.");
    }
    match state.store.money_candidate_decisions(&id,query.limit.unwrap_or(50)){
        Ok(rows)=>Json(rows).into_response(),
        Err(err)=>super::error(StatusCode::INTERNAL_SERVER_ERROR,&err)
    }
}

fn requested_updates(body:&Value)->Result<(Map<String,Value>,Map<String,Value>,Option<u64>,String),String>{
    let obj=body.as_object().ok_or("사업 후보 수정 요청은 JSON 객체여야 합니다.")?;
    let mut top=Map::new();
    if let Some(value)=obj.get("status"){
        let status=value.as_str().ok_or("사업 후보 상태는 문자열이어야 합니다.")?;
        if !matches!(status,
            "hypothesis"|"researching"|"testing"|"validated"|"rejected"|
            "launched"|"on_hold"){
            return Err("지원하지 않는 사업 상태입니다.".into());
        }
        top.insert("status".into(),json!(status));
    }
    for key in ["firstTest","killSignal","distribution","defensibility","notes"]{
        if let Some(value)=obj.get(key){
            let str_value=value.as_str().ok_or("후보 메모와 텍스트 항목은 문자열이어야 합니다.")?;
            top.insert(key.into(),json!(str_value.chars().take(3000).collect::<String>()));
        }
    }
    let mut economy=Map::new();
    if let Some(value)=obj.get("economics"){
        let changes=value.as_object().ok_or("경제성 변경은 JSON 객체여야 합니다.")?;
        for key in ["priceWon","variableCostWon","acquisitionCostWon",
            "monthlyFixedWon","monthlyUnits","setupCostWon","refundRatePct"]{
            if let Some(value)=changes.get(key){
                if value.is_null(){economy.insert(key.into(),Value::Null);}
                else if let Some(n)=valid_won(value,if key=="refundRatePct"{100.0}else{1e14}){
                    economy.insert(key.into(),json!(n));
                }else{
                    return Err("경제성 값은 0 이상의 유효한 숫자 또는 비어 있는 값이어야 합니다.".into());
                }
            }
        }
    }
    if top.is_empty() && economy.is_empty(){
        return Err("변경할 후보 항목이 없습니다.".into());
    }
    let expected_revision=match obj.get("expectedRevision"){
        None=>None,
        Some(v)=>Some(v.as_u64().ok_or("expectedRevision은 0 이상의 정수여야 합니다.")?),
    };
    let reason=match obj.get("decisionReason").or_else(||obj.get("reason")){
        None=>String::new(),
        Some(value)=>value.as_str().ok_or("판단 사유는 문자열이어야 합니다.")?
            .chars().take(2000).collect::<String>(),
    };
    Ok((top,economy,expected_revision,reason))
}

/// Update only the decision-owned fields against the *latest* candidate.
/// A simultaneous approval from Money→native job can never be silently
/// replaced by a stale owner review payload.
pub async fn update_candidate(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    let Some(initial)=state.store.get("money_candidate",&id)else{
        return super::error(StatusCode::NOT_FOUND,"사업 후보를 찾지 못했습니다.");
    };
    let (requested,economy,expected_revision,reason)=match requested_updates(&body){
        Ok(changes)=>changes,Err(message)=>return fail(&message)
    };
    let decision_id_hash=format!("{:x}",Sha256::digest(id.as_bytes()));
    let committed=state.store.mutate_record("money_candidate",&id,initial,|candidate|{
        let current_revision=candidate["reviewRevision"].as_u64().unwrap_or(0);
        if expected_revision.is_some_and(|rev|rev!=current_revision){
            return Err("revision_conflict".into());
        }
        if requested.get("status").is_some_and(|status|
            matches!(status.as_str(),Some("rejected"|"on_hold")))
            && candidate["implementationJobId"].as_str().is_some(){
            return Err("implementation_claimed".into());
        }
        let mut changes=Map::new();
        for (key,after) in &requested{
            let before=candidate.get(key).cloned().unwrap_or(Value::Null);
            if before!=*after{
                changes.insert(key.clone(),json!({"before":before,"after":after}));
                candidate[key]=after.clone();
            }
        }
        if !economy.is_empty(){
            let before=candidate["economics"].clone();
            let mut values=before.as_object().cloned().unwrap_or_default();
            for (key,value) in &economy{values.insert(key.clone(),value.clone());}
            let after=Value::Object(values);
            if before!=after{
                changes.insert("economics".into(),json!({"before":before,"after":after}));
                candidate["economics"]=after;
            }
        }
        if changes.is_empty(){return Ok(vec![]);}
        let revision=current_revision.checked_add(1).ok_or("review revision overflow")?;
        let decision_id=format!("money:review:{decision_id_hash}:{revision}");
        let at=now();
        let event=json!({
            "id":decision_id,"candidateId":id,"revision":revision,
            "previousRevision":current_revision,"at":at,
            "reason":reason,"changes":changes,
            "statusAfter":candidate["status"],
            "origin":"owner_review","verifiedSale":false
        });
        candidate["reviewRevision"]=json!(revision);
        candidate["lastReviewId"]=event["id"].clone();
        candidate["lastReviewAt"]=json!(at);
        candidate["lastReviewReason"]=json!(reason);
        candidate["updatedAt"]=json!(at);
        Ok(vec![("money_decision".into(),event)])
    });
    match committed{
        Ok(row)=>{
            state.bus.record("money_candidate",&row);
            let experiments=state.store.money_recent_experiments(
                row["campaignId"].as_str().unwrap_or(""),150).unwrap_or_default();
            let capital=state.store.get("money_campaign",
                row["campaignId"].as_str().unwrap_or(""))
                .and_then(|c|c["capitalWon"].as_f64()).unwrap_or(-1.0);
            Json(decorate(row,&experiments,capital)).into_response()
        },
        Err(ref reason) if reason=="revision_conflict"=>
            super::error(StatusCode::CONFLICT,"후보가 다른 화면에서 수정되었습니다. 최신 리비전을 읽고 다시 검토하세요."),
        Err(ref reason) if reason=="implementation_claimed"=>
            super::error(StatusCode::CONFLICT,"이미 구현 작업이 승인된 후보입니다. 먼저 연결된 작업을 확인하세요."),
        Err(err)=>super::error(StatusCode::INTERNAL_SERVER_ERROR,&err)
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->AppState{
        let root=std::env::temp_dir().join(format!("dominion-money-review-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("money_candidate",&json!({
            "id":"review/1","campaignId":"money:auto","status":"hypothesis",
            "title":"Observed industrial opportunity","offer":"Unverified buyer service",
            "evidence":[{"claim":"buyer request","url":"https://buyer.example/request"}],
            "economics":{"priceWon":null},"reviewRevision":0
        })).unwrap();
        state
    }
    #[tokio::test]
    async fn hold_and_restore_are_atomic_and_preserve_full_reason_history(){
        let state=state();
        let hold=update_candidate(State(state.clone()),Path("review/1".into()),
            Json(json!({"status":"on_hold","decisionReason":"Buyer price unknown",
                "expectedRevision":0,"firstTest":"Contact buyer after approval"}))).await;
        assert_eq!(hold.status(),StatusCode::OK);
        let retained=state.store.get("money_candidate","review/1").unwrap();
        assert_eq!(retained["reviewRevision"],1);
        assert_eq!(retained["lastReviewReason"],"Buyer price unknown");
        assert_eq!(retained["evidence"][0]["url"],"https://buyer.example/request");
        let archive=state.store.money_candidate_archive("archived","industrial",50).unwrap();
        assert_eq!(archive.len(),1);
        assert_eq!(archive[0]["status"],"on_hold");
        let held=state.store.money_candidate_decisions("review/1",10).unwrap();
        assert_eq!(held.len(),1);
        assert_eq!(held[0]["changes"]["status"]["before"],"hypothesis");
        let resumed=update_candidate(State(state.clone()),Path("review/1".into()),
            Json(json!({"status":"researching","expectedRevision":1,
                "decisionReason":"New quote received"}))).await;
        assert_eq!(resumed.status(),StatusCode::OK);
        let history=state.store.money_candidate_decisions("review/1",10).unwrap();
        assert_eq!(history.len(),2);
        assert_eq!(history[0]["revision"],2);
        assert_eq!(history[1]["reason"],"Buyer price unknown");
        assert!(state.store.money_candidate_archive("archived","",50).unwrap().is_empty());
    }
    #[tokio::test]
    async fn simultaneous_owner_reviews_do_not_erase_each_others_decisions(){
        let state=state();
        let (a,b)=tokio::join!(
            update_candidate(State(state.clone()),Path("review/1".into()),
                Json(json!({"status":"on_hold","expectedRevision":0}))),
            update_candidate(State(state.clone()),Path("review/1".into()),
                Json(json!({"status":"rejected","expectedRevision":0})))
        );
        assert_eq!(usize::from(a.status()==StatusCode::OK)+
            usize::from(b.status()==StatusCode::OK),1);
        assert_eq!(usize::from(a.status()==StatusCode::CONFLICT)+
            usize::from(b.status()==StatusCode::CONFLICT),1);
        assert_eq!(state.store.get("money_candidate","review/1").unwrap()["reviewRevision"],1);
        assert_eq!(state.store.money_candidate_decisions("review/1",10).unwrap().len(),1);
    }
    #[tokio::test]
    async fn invalid_economics_cannot_create_a_partial_decision(){
        let state=state();
        let failed=update_candidate(State(state.clone()),Path("review/1".into()),
            Json(json!({"status":"on_hold","economics":{"priceWon":-5}}))).await;
        assert_eq!(failed.status(),StatusCode::BAD_REQUEST);
        assert_eq!(state.store.get("money_candidate","review/1").unwrap()["status"],"hypothesis");
        assert!(state.store.money_candidate_decisions("review/1",10).unwrap().is_empty());
    }
    #[tokio::test]
    async fn an_approved_implementation_cannot_be_silently_archived(){
        let state=state();
        let mut row=state.store.get("money_candidate","review/1").unwrap();
        row["implementationJobId"]=json!("money:implement:existing");
        state.store.put("money_candidate",&row).unwrap();
        let response=update_candidate(State(state.clone()),Path("review/1".into()),
            Json(json!({"status":"rejected","decisionReason":"pause"}))).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert!(state.store.money_candidate_decisions("review/1",10).unwrap().is_empty());
    }
}
