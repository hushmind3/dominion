//! One-click, source-backed money opportunity drafts. No model calls.
use crate::{engine, state::AppState};
use axum::{extract::{Path,State}, http::StatusCode, response::{IntoResponse,Response}, Json};
use serde_json::{json,Value};
use std::collections::HashSet;

const CAMPAIGN_ID:&str="money:auto";
const REPORT_ID:&str="money:find:latest";
// The database campaign claim prevents duplicate model jobs; this lock also
// preserves the report -> selected candidates -> job reference as one sequence.
static FIND_LOCK:tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn source_host(url:&str)->&str{
    url.strip_prefix("https://").unwrap_or("")
        .split(|ch:char|ch=='/'||ch=='?'||ch=='#').next().unwrap_or("").trim_start_matches("www.")
}
fn publication_age(evidence:&Value,now:&chrono::DateTime<chrono::Utc>)->Option<i64>{
    let rows=evidence.as_array()?;
    if rows.is_empty(){return None;}
    let mut oldest=0i64;
    for row in rows{
        // Capture time is not publication time, especially for old tenders.
        let raw=row["publishedAt"].as_str()?;
        let parsed=chrono::DateTime::parse_from_rfc3339(raw)
            .or_else(|_|chrono::DateTime::parse_from_rfc2822(raw)).ok()?;
        let days=now.signed_duration_since(parsed.with_timezone(&chrono::Utc)).num_days();
        if days< -1{return None;}
        oldest=oldest.max(days.max(0));
    }
    Some(oldest)
}
fn discovery_priority(draft:&Value,now:&chrono::DateTime<chrono::Utc>)->i64{
    let base=match draft["origin"].as_str(){
        Some("source_pair_draft")=>34,
        Some("single_source_research_draft")=>20,
        _=>12
    };
    let research=(draft["researchPriority"].as_u64().unwrap_or(0).min(100)/5) as i64;
    let buyer=if draft["buyerSignal"]=="buyer_request"{7}else{0};
    let fetched=draft["evidence"].as_array().into_iter().flatten()
        .filter(|row|row["contentStatus"]=="page_fetched").count().min(2) as i64*3;
    let age=match publication_age(&draft["evidence"],now){
        Some(0..=7)=>16,Some(8..=30)=>9,Some(31..=90)=>2,
        Some(91..=180)=>-8,Some(_)=>-25,None=>0
    };
    base+research+buyer+fetched+age
}
/// Deterministic research relevance. Free: no model tokens are spent deciding
/// which collected signal deserves a draft.
fn signal_score(row:&Value)->i64{
    let mut score=0i64;
    let tags=row["changeTags"].as_array().cloned().unwrap_or_default();
    if tags.iter().any(|t|t=="buyer_request"){score+=40;}
    if tags.iter().any(|t|t=="price"||t=="supply"||t=="demand"){score+=15;}
    let description=row["description"].as_str().unwrap_or("").chars().count();
    score+=(description.min(400)/10) as i64;
    if row["articleExcerpt"].as_str().is_some_and(|t|t.chars().count()>100){score+=20;}
    if row["url"].as_str().is_some_and(|u|u.starts_with("https://")){score+=5;}
    score
}

fn next_drafts(pool:&[Value],previous:&Value,limit:usize)->(Vec<Value>,Vec<String>,usize){
    if pool.is_empty(){return (Vec::new(),Vec::new(),0);}
    // Rank by evidence strength instead of rotating by cursor: the draft set
    // should reflect what the sources actually contain, and the shown-history
    // still prevents repeats on the next run.
    let mut ordered=pool.iter().collect::<Vec<_>>();
    ordered.sort_by(|a,b|signal_score(b).cmp(&signal_score(a)));
    let mut history=previous["shownCandidateIds"].as_array().into_iter()
        .flatten().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>();
    let seen=history.iter().cloned().collect::<HashSet<_>>();
    let mut selected=Vec::new();
    let mut picked=HashSet::new();
    for unseen_only in [true,false]{
        for offset in 0..ordered.len(){
            let draft=ordered[offset];
            let Some(id)=draft["id"].as_str()else{continue};
            if draft["status"]=="rejected"||picked.contains(id)
                ||(unseen_only&&seen.contains(id)){continue;}
            picked.insert(id.to_owned());
            selected.push(draft.clone());
            if selected.len()>=limit{break;}
        }
        if selected.len()>=limit{break;}
    }
    for draft in &selected{
        if let Some(id)=draft["id"].as_str(){
            if !seen.contains(id){history.push(id.to_owned());}
        }
    }
    if history.len()>128{history.drain(..history.len()-128);}
    (selected,history,0)
}

fn short(row:&Value,field:&str,max:usize)->String{
    row[field].as_str().unwrap_or("").trim().chars().take(max).collect()
}
fn choose_model(kind:&str)->(&'static str,&'static str,&'static str){
    match kind{
        "buyer_supply_spread"|"new_tool_to_buyer"=>(
            "구매 요청에 맞춘 공급·구축 서비스",
            "구매 요청의 요구사항과 공급 원가 차이를 실제 계약으로 연결하면 구축비 또는 납품 대금을 받을 수 있습니다.",
            "구매 공고의 규격·마감일을 읽고 공급 기술의 라이선스·가격을 대조한 뒤, 납품 가능한 최소 구성과 견적 초안을 작성합니다."),
        "automation_transfer"=>(
            "비용 상승 업종의 반복업무 자동화",
            "반복업무에 이미 지출하는 인건비·외주비의 일부를 자동화 서비스 이용료로 전환하는 수익 가설입니다.",
            "비용이 상승한 업무 한 가지를 선정하고 도구로 처리 가능한 비율을 확인해 시연·비용 절감표·유료 시험안을 작성합니다."),
        "input_substitution"|"demand_cost_spread"|"bottleneck_bypass"=>(
            "기존 공급을 대체하는 비용 절감 서비스",
            "비용이 높거나 공급이 부족한 기존 방식보다 저렴한 대체 공급으로 납품차익이나 구축비를 얻는 가설입니다.",
            "고객의 기존 품목·단가·규격을 확인하고 대체 공급의 견적·납기·호환성을 비교한 뒤 시험 공급을 제안합니다."),
        "supply_substitution"=>(
            "공급 부족을 대체하는 제품·서비스",
            "기존 공급을 기다리는 고객에게 대체 옵션을 제공하고 납품 또는 도입 지원 대금을 받는 수익 가설입니다.",
            "부족한 품목·기능을 확인하고 대체 기술의 필수 요건을 비교해 소규모 유료 시험 공급안을 만듭니다."),
        "compliance_product"=>(
            "규정 변화에 대응하는 지원 도구",
            "규정 대응을 위한 반복 비용을 자동 점검·문서화 서비스로 줄여 이용료를 받을 수 있다는 가설입니다.",
            "규정 원문·적용 대상을 확인하고 반복 점검 항목을 추출해 체크리스트·보고서 생성 도구 초안을 작성합니다."),
        _=>(
            "시장 변화 대응 서비스",
            "관측된 시장 변화에서 발생한 비용·수요 문제를 해결하는 유료 서비스 가설입니다.",
            "원문을 확인하고 실제 지출·구매 담당자·기술 적합성을 조사해 최소 유료 시험안을 작성합니다.")
    }
}
fn candidate_from_pair(state:&AppState,pair:&Value)->Option<Value>{
    let source=state.store.get("money_signal",pair["supplySignalId"].as_str()?)?;
    let buyer=state.store.get("money_signal",pair["buyerSignalId"].as_str()?)?;
    let a=short(&source,"url",1500);
    let b=short(&buyer,"url",1500);
    if !a.starts_with("https://")||!b.starts_with("https://")||a==b
        ||source_host(&a).is_empty()||source_host(&a).eq_ignore_ascii_case(source_host(&b))
        ||pair["supplyUrl"].as_str().is_some_and(|old|old!=a)
        ||pair["buyerUrl"].as_str().is_some_and(|old|old!=b){return None}
    let source_title=short(&source,"title",150);
    let buyer_title=short(&buyer,"title",150);
    if source_title.len()<8||buyer_title.len()<8{return None}
    let (product,why,plan)=choose_model(pair["mechanism"].as_str().unwrap_or(""));
    let causal=format!("관측된 공급 변화({})가 구매자 문제({}) 해결에 적용된다면 기존 비용·시간·위험을 줄이고 그 절감 일부를 대금으로 받을 가능성이 있다",source_title,buyer_title);
    let id=format!("money:draft:{}:{}",CAMPAIGN_ID,pair["id"].as_str()?);
    let heading=format!("{} · {}",product,buyer_title.chars().take(48).collect::<String>());
    let first=short(pair,"nextProof",650);
    Some(json!({
        "id":id,"campaignId":CAMPAIGN_ID,"jobId":null,
        "status":"hypothesis","origin":"source_pair_draft",
        "evidenceStatus":"two_source_research_question_only","buyerSignal":pair["buyerChange"],
        "title":heading,
        "customer":if pair["buyerChange"]=="buyer_request"{"공개 구매 요청의 발주 담당자 (미확인)"}
            else{"비용·공급 문제를 겪는 업종의 구매 담당자 (미확인)"},
        "problem":buyer_title,
        "offer":format!("'{}' 문제에 '{}' 공급 변화를 적용한 서비스·제품 초안",buyer_title,source_title),
        "mechanism":causal,"whyPay":why,"implementationPlan":plan,
        "inferenceSteps":[format!("관측: 공급 시장에서 {}",source_title),
            format!("관측: 구매측에서 {}",buyer_title),
            format!("추측: 새로운 공급 수단이 구매자의 문제를 대체하거나 완화한다면 {} 형태로 판매 가능",product),
            "추측: 고객의 기존 지출이나 위험을 낮추는 대가로 거래가 성립할 수 있음"],
        "counterfactual":"양쪽 산업의 규격·법규가 맞지 않거나 기존 방법의 실지출이 낮다면 이 연결은 돈벌이가 되지 않는다",
        "firstBuyer":if pair["buyerChange"]=="buyer_request"{"구매 공고 담당자 · 실제 결제권자 확인 필요"}
            else{"구매 권한이 있는 고객 · 수요 확인 필요"},
        "firstPaymentStep":if first.is_empty(){"원문에서 실제 비용·구매권자 확인 후 유료 시범 공급을 제안"}
            else{first.as_str()},
        "firstTest":first,
        "killSignal":"규격·단가·결제권자 조사에서 유료 수요가 없으면 제외",
        "distribution":"","defensibility":"","buyerEvidence":[],
        "evidence":[{"claim":buyer_title,"url":b,"kind":"buyer_side_observation",
                       "publishedAt":buyer["publishedAt"],"capturedAt":buyer["capturedAt"],
                       "contentStatus":buyer["contentStatus"]},
                    {"claim":source_title,"url":a,"kind":"supply_side_observation",
                       "publishedAt":source["publishedAt"],"capturedAt":source["capturedAt"],
                       "contentStatus":source["contentStatus"]}],
        "assumptions":["서로 다른 출처의 연결은 추론이며 실제 발주·구매 증거가 아님"],
        "researchPriority":pair["researchPriority"],
        "economics":{"priceWon":null,"variableCostWon":null,"acquisitionCostWon":null,
            "monthlyFixedWon":null,"monthlyUnits":null,"setupCostWon":null,"refundRatePct":null},
        "createdAt":chrono::Utc::now().to_rfc3339()
    }))
}
fn candidate_from_signal(source:&Value)->Option<Value>{
    let url=short(source,"url",1500);
    if !url.starts_with("https://"){return None}
    let title=short(source,"title",160);
    if title.chars().count()<8{return None}
    let topic=source["topics"].as_array().and_then(|a|a.first()).and_then(Value::as_str)
        .unwrap_or("시장");
    let changes=source["changeTags"].as_array()?;
    let action=changes.iter().filter_map(Value::as_str)
        .find(|t|matches!(*t,"buyer_request"|"cost_rise"|"shortage"|"demand"|"policy"))?;
    let (product,why,plan)=choose_model(match action {
        "buyer_request"=>"new_tool_to_buyer",
        "cost_rise"=>"automation_transfer",
        "shortage"=>"supply_substitution",
        "policy"=>"compliance_product",
        _=>"demand_cost_spread"
    });
    Some(json!({
        "id":format!("money:draft:{}:signal:{}",CAMPAIGN_ID,source["id"].as_str()?),
        "campaignId":CAMPAIGN_ID,"status":"hypothesis","origin":"single_source_research_draft",
        "evidenceStatus":"single_source_only_not_validated","buyerSignal":action,
        "title":format!("{} · {}",product,title.chars().take(48).collect::<String>()),
        "customer":format!("{} 분야의 구매·운영 담당자 (실제 결제권자 확인 필요)",topic),
        "problem":title,
        "offer":format!("'{}'라는 시장 변화에 대응하는 유료 서비스 아이디어",title),
        "mechanism":why,"whyPay":why,
        "inferenceSteps":[format!("관측: {} 분야의 새로운 변화 - {}",topic,title),
            format!("추측: 변화가 고객에게 반복적인 비용 또는 불편을 만든다면 {}",product),
            "추측: 문제 해결 결과물에 대한 유료 검증을 거쳐 거래 여부를 결정"],
        "counterfactual":"고객이 실제 비용이나 불편을 느끼지 않거나 이미 무료 대안이 있다면 유료 수요는 성립하지 않는다",
        "implementationPlan":plan,
        "firstBuyer":"원문에서 구매 요청·예산 및 결제 담당자 확인",
        "firstPaymentStep":"원문 검증 → 지출 항목 조사 → 유료 시범 제안",
        "firstTest":"고객과 기존 지출·구매 가능 금액 확인",
        "killSignal":"실제 구매 문제 또는 지출 증거가 없으면 제외",
        "buyerEvidence":[],"evidence":[{"claim":title,"url":url,"kind":"source_observation",
            "publishedAt":source["publishedAt"],"capturedAt":source["capturedAt"],
            "contentStatus":source["contentStatus"]}],
        "assumptions":["단일 기사에서 만든 초기 조사 초안, 고객 구매 의사 미확인"],
        "economics":{"priceWon":null,"variableCostWon":null,"acquisitionCostWon":null,
            "monthlyFixedWon":null,"monthlyUnits":null,"setupCostWon":null,"refundRatePct":null},
        "createdAt":chrono::Utc::now().to_rfc3339()
    }))
}
fn candidates(state:&AppState)->Result<Vec<Value>,String>{
    let mut result=Vec::new();
    let mut seen_sources=HashSet::new();
    let mut seen_products=HashSet::new();
    for pair in state.store.money_best_tensions(60)? {
        let Some(candidate)=candidate_from_pair(state,&pair)else{continue};
        let buyer=pair["buyerSignalId"].as_str().unwrap_or("");
        if !seen_sources.insert(buyer.to_owned()){continue}
        let product=short(&candidate,"title",150);
        if !seen_products.insert(product){continue}
        result.push(candidate);
        if result.len()>=18{break}
    }
    // Some archives do not yet contain cross-sector joins; classify actual
    // buyer/cost events as explicitly weaker, single-source research drafts.
    if result.len()<9 {
        for signal in state.store.money_recent_signals(120)?{
            let Some(candidate)=candidate_from_signal(&signal)else{continue};
            let source_id=signal["id"].as_str().unwrap_or("");
            if !seen_sources.insert(source_id.to_string()){continue}
            result.push(candidate);
            if result.len()>=18{break}
        }
    }
    // Existing validated/modelled business hypotheses are useful when the
    // new source archive is sparse. Keep their provenance and original records.
    if result.len()<3 {
        let mut known=result.iter().filter_map(|x|x["id"].as_str())
            .map(str::to_string).collect::<HashSet<_>>();
        for item in state.store.money_recent_records("money_candidate",100)?{
            let Some(id)=item["id"].as_str()else{continue};
            if item["status"]=="rejected"||known.contains(id)
                ||short(&item,"title",160).is_empty()
                ||short(&item,"offer",300).is_empty(){continue}
            known.insert(id.to_string());
            result.push(item);
            if result.len()>=3{break}
        }
    }
    let now=chrono::Utc::now();
    result.sort_by(|a,b|discovery_priority(b,&now).cmp(&discovery_priority(a,&now))
        .then_with(||a["id"].as_str().cmp(&b["id"].as_str())));
    Ok(result)
}
fn active_automations(state:&AppState)->Vec<String>{
    state.store.all("money_autopilot").into_iter().filter(|r|r["enabled"]==true)
        .filter_map(|r|r["id"].as_str().map(str::to_owned)).collect()
}
fn saved_list(state:&AppState,ids:&[String])->Vec<Value>{
    ids.iter().filter_map(|id|state.store.get("money_candidate",id)).collect()
}
fn view(state:&AppState)->Value{
    let report=state.store.get("money_find_report",REPORT_ID).unwrap_or(Value::Null);
    let ids=report["candidateIds"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
    let mut drafts=saved_list(state,&ids);
    let job_id=report["reasoningJobId"].as_str();
    let job=job_id.and_then(|id|state.store.get("job",id)).unwrap_or(Value::Null);
    let status=job["status"].as_str().unwrap_or("not_requested");
    let mut model_used=Value::Null;
    if matches!(status,"completed"|"needs_review"){
        let generated=job["result"]["candidateIds"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).filter_map(|id|state.store.get("money_candidate",id))
            .collect::<Vec<_>>();
        if !generated.is_empty(){
            let mut selected=generated.into_iter().take(3).collect::<Vec<_>>();
            let mut seen=selected.iter().filter_map(|c|c["id"].as_str())
                .map(str::to_owned).collect::<HashSet<_>>();
            for guess in drafts{
                if selected.len()>=3{break}
                let Some(id)=guess["id"].as_str()else{continue};
                if seen.insert(id.to_owned()){selected.push(guess);}
            }
            drafts=selected;
        }
    }
    if let Some(session)=job["sessionId"].as_str().or_else(||job["result"]["sessionId"].as_str())
        .and_then(|id|state.store.get("session",id)){
        model_used=json!({"providerId":session["providerId"],"model":session["model"]});
    }
    // Each Money inference job owns a normal Rust session. The persistent
    // model.usage ledger is authoritative; scheduled != billed and missing
    // price/FX telemetry must not become a fabricated zero-cost claim.
    let usage=job["sessionId"].as_str().or_else(||job["result"]["sessionId"].as_str())
        .map(|session_id|state.store.session_usage(session_id));
    let calls=usage.as_ref().and_then(|u|u["calls"].as_u64()).unwrap_or(0);
    let inference_usage=usage.as_ref().map(|u|json!({
        "calls":u["calls"],"promptTokens":u["promptTokens"],
        "completionTokens":u["completionTokens"],"totalTokens":u["totalTokens"],
        "cachedPromptTokens":u["cachedPromptTokens"],
        "cacheObservedCalls":u["cacheObservedCalls"],
        "estimatedCalls":u["estimatedCalls"],
        "pricedCalls":u["pricedCalls"],"unpricedCalls":u["unpricedCalls"],
        "costWon":null,"costStatus":"not_measured_in_krw"
    }));
    let error=job["error"].as_str().or_else(||job["result"]["reason"].as_str());
    json!({"drafts":drafts,"searchedAt":report["searchedAt"],
        "sourceCount":report["sourceCount"],"modelCalls":calls,
        "inferenceUsage":inference_usage,
        "selectedAgentId":report["selectedAgentId"],"selectedAgentName":report["selectedAgentName"],
        "inferenceJobId":job_id,"inferenceStatus":status,
        "inferenceError":error.or_else(||report["inferenceStartError"].as_str()),"inferenceModel":model_used,
        "inferenceTokenBudget":if job_id.is_some(){json!(report["inferenceTokenBudget"].as_u64().unwrap_or(24_000))}else{Value::Null},
        "activeAutomationIds":active_automations(state)})
}
pub async fn latest(State(state):State<AppState>)->Json<Value>{
    Json(view(&state))
}
pub async fn find(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let _guard=FIND_LOCK.lock().await;
    let mode=body["mode"].as_str().unwrap_or("db");
    if !matches!(mode,"db"|"agent"){
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"지원하지 않는 발굴 방식입니다."}))).into_response();
    }
    let agent_id=body["agentId"].as_str().unwrap_or("").trim();
    if agent_id.len()>200{
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"에이전트 ID가 너무 깁니다."}))).into_response();
    }
    let selected_agent=if mode=="agent"{
        if agent_id.is_empty(){
            return (StatusCode::BAD_REQUEST,Json(json!({"error":"실행할 에이전트를 선택하세요."}))).into_response();
        }
        let Some(agent)=state.store.get("agent",agent_id)else{
            return (StatusCode::BAD_REQUEST,Json(json!({"error":"선택한 에이전트가 없습니다."}))).into_response();
        };
        if let Some(provider_id)=agent["providerId"].as_str().filter(|id|!id.is_empty()){
            if !state.store.get("provider",provider_id).is_some_and(|p|p["enabled"]!=false){
                return (StatusCode::BAD_REQUEST,Json(json!({"error":"이 에이전트가 사용하는 모델 제공자가 연결되지 않았습니다."}))).into_response();
            }
        }else if !state.store.all("provider").iter().any(|p|p["enabled"]!=false){
            return (StatusCode::BAD_REQUEST,Json(json!({"error":"이 에이전트를 실행할 모델 제공자가 없습니다."}))).into_response();
        }
        Some(agent)
    }else{None};
    // Do not start a second paid inference while the first one is running.
    let last=state.store.get("money_find_report",REPORT_ID).unwrap_or(Value::Null);
    if let Some(job)=last["reasoningJobId"].as_str()
        .and_then(|id|state.store.get("job",id)){
        if matches!(job["status"].as_str(),Some("queued"|"running")){
            return Json(view(&state)).into_response();
        }
    }
    let pool=match candidates(&state){
        Ok(pool)=>pool,
        Err(err)=>return (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":err}))).into_response()
    };
    if pool.is_empty(){
        engine::money_sources::schedule(&state);
        return Json(json!({"drafts":[],"sourceCount":state.store.money_recent_signals(120)
            .map(|a|a.len()).unwrap_or(0),"modelCalls":0,
            "message":"아직 구매·비용 변화 자료가 부족합니다. 수집은 자동으로 계속됩니다.",
            "activeAutomationIds":active_automations(&state)})).into_response();
    }
    if state.store.get("money_campaign",CAMPAIGN_ID).is_none(){
        let campaign=json!({"id":CAMPAIGN_ID,"name":"자동 돈벌이 발굴",
            "brief":"여러 산업과 지역의 시장 변화에서 구매자가 지불할 만한 기회를 발굴",
            "capitalWon":null,"hardwareInvestmentWon":null,
            "targetMonthlyProfitWon":null,"hoursPerWeek":null,"skills":"","markets":"",
            "distribution":"","exclusions":"","preferences":"","createdAt":chrono::Utc::now().to_rfc3339()});
        if state.store.put("money_campaign",&campaign).is_none(){
            return (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":"발굴 초안 저장소 생성 실패"}))).into_response();
        }
    }
    let previous=state.store.get("money_find_report",REPORT_ID).unwrap_or(Value::Null);
    let eligible=pool.into_iter().filter(|draft|{
        draft["id"].as_str().is_some_and(|id|
            state.store.get("money_candidate",id)
                .is_none_or(|saved|saved["status"]!="rejected"))
    }).collect::<Vec<_>>();
    // How many unseen drafts this click surfaces. The previous hard cap of
    // three made a 460-signal archive look empty.
    let limit=body["limit"].as_u64().unwrap_or(9).clamp(1,24) as usize;
    let (next,shown,next_cursor)=next_drafts(&eligible,&previous,limit);
    let mut chosen=Vec::<Value>::new();
    for draft in &next{
        let Some(id)=draft["id"].as_str()else{continue};
        if state.store.get("money_candidate",id).is_none(){
            let Some(saved)=state.store.put("money_candidate",draft)else{continue};
            state.bus.record("money_candidate",&saved);
        }
        if let Some(saved)=state.store.get("money_candidate",id){
            if saved["status"]!="rejected"{chosen.push(saved);}
        }
    }
    let ids=chosen.iter().filter_map(|x|x["id"].as_str()).map(str::to_string)
        .collect::<Vec<_>>();
    // Local/free models need a larger prompt budget: the research brief plus
    // stored signals easily exceeds 8k, which cancelled the call before it
    // ever reached the provider.
    let inference_budget=body["tokenBudget"].as_u64().unwrap_or(24_000).clamp(4_000,120_000);
    let mut report=json!({"id":REPORT_ID,"candidateIds":ids,"cursor":next_cursor,"inferenceTokenBudget":inference_budget,
        "shownCandidateIds":shown,"selectionMethod":"source_quality_then_unseen",
        "sourceCount":state.store.money_recent_signals(120).map(|a|a.len()).unwrap_or(0),
        "searchedAt":chrono::Utc::now().to_rfc3339(),"modelCalls":0,
        "selectedAgentId":if selected_agent.is_some(){json!(agent_id)}else{json!("db")},
        "selectedAgentName":selected_agent.as_ref().map(|a|a["name"].clone()).unwrap_or(Value::Null),
        "reasoningJobId":null});
    if state.store.put("money_find_report",&report).is_none(){
        return (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":"발굴 결과 저장 실패"}))).into_response()
    }
    // Investigation/archival is free of LLM calls. If a configured provider is
    // available, exactly ONE native, bounded, tool-free inference job elaborates
    // this research into genuinely novel commercial hypotheses.
    // Research is shown first and costs no tokens. The paid inference is an
    // explicit second step, so an agent selection alone never spends budget.
    let want_inference=body["infer"].as_bool().unwrap_or(true);
    if selected_agent.is_some()&&want_inference{
        let response=super::start(State(state.clone()),Path(CAMPAIGN_ID.to_owned()),
            Json(json!({"mode":"infer","tokenBudget":inference_budget,"agentId":agent_id}))).await;
        if response.status()==StatusCode::ACCEPTED{
            report["reasoningJobId"]=state.store.get("money_campaign",CAMPAIGN_ID)
                .and_then(|c|c.get("lastJobId").cloned()).unwrap_or(Value::Null);
            let _=state.store.put("money_find_report",&report);
        }else{
            report["inferenceStartError"]=json!(format!("모델 추론 작업 대기 등록 실패 (HTTP {})",response.status()));
            let _=state.store.put("money_find_report",&report);
        }
    }
    state.bus.record("money_find_report",&report);
    Json(view(&state)).into_response()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn selected_existing_agent_reaches_native_job_input_and_survives_refresh(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-money-chosen-agent-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("provider",&json!({"id":"p1","name":"Local test endpoint",
            "enabled":true,"kind":"openai","baseUrl":"http://127.0.0.1:9/v1",
            "model":"old-default"})).unwrap();
        state.store.put("agent",&json!({"id":"agent-1","name":"My existing agent",
            "providerId":"p1","model":"user-selected-27b","systemPrompt":"Analyze opportunities"})).unwrap();
        state.store.put("money_signal",&json!({"id":"buyer:test",
            "title":"Supplier issues new factory service purchasing request",
            "url":"https://buyer.example.org/need","sourceName":"test",
            "changeTags":["buyer_request"],"topics":["manufacturing"],
            "capturedAt":"2026-11-17T00:00:00Z"})).unwrap();
        let (first,second)=tokio::join!(
            find(State(state.clone()),Json(json!({"mode":"agent","agentId":"agent-1"}))),
            find(State(state.clone()),Json(json!({"mode":"agent","agentId":"agent-1"})))
        );
        assert_eq!(first.status(),StatusCode::OK);
        assert_eq!(second.status(),StatusCode::OK);
        let queued=state.store.all("job").into_iter().filter(|j|
            j["kind"]=="money_discover" && j["status"]=="queued").count();
        assert_eq!(queued,1,"overlapping clicks must share one paid inference job");
        let visible=latest(State(state.clone())).await.0;
        assert_eq!(visible["selectedAgentId"],"agent-1");
        assert_eq!(visible["selectedAgentName"],"My existing agent");
        let job_id=visible["inferenceJobId"].as_str().expect("model task queued");
        let job=state.store.get("job",job_id).expect("durable inference job");
        assert_eq!(job["input"]["agentId"],"agent-1");
        assert!(job["input"]["providerId"].is_null());
        assert!(job["input"]["model"].is_null());
        assert_eq!(job["input"]["mode"],"infer");
        assert_eq!(job["input"]["toolsEnabled"],false);
        assert_eq!(job["input"]["tokenBudget"],24_000);
        // A queued inference has not spent model tokens.
        assert_eq!(visible["modelCalls"],0);
        assert!(visible["inferenceUsage"].is_null());
        // Progress comes from the actual owning session usage ledger.
        state.store.put("session",&json!({"id":"money-job-session","providerId":"p1",
            "model":"user-selected-27b"})).unwrap();
        state.store.patch_if("job",job_id,"status",&json!("queued"),
            &json!({"sessionId":"money-job-session"})).unwrap();
        state.store.audit("model.usage",json!({
            "sessionId":"money-job-session","providerId":"p1",
            "model":"user-selected-27b","promptTokens":410,"completionTokens":90,
            "totalTokens":500,"cachedPromptTokens":120,"estimated":false
        }));
        // Unrelated sessions cannot make this one appear more expensive.
        state.store.audit("model.usage",json!({
            "sessionId":"another-job","promptTokens":100000,
            "completionTokens":1000,"totalTokens":101000,"estimated":false
        }));
        let active=latest(State(state.clone())).await.0;
        assert_eq!(active["modelCalls"],1);
        assert_eq!(active["inferenceUsage"]["totalTokens"],500);
        assert_eq!(active["inferenceUsage"]["cachedPromptTokens"],120);
        assert_eq!(active["inferenceUsage"]["unpricedCalls"],1);
        assert!(active["inferenceUsage"]["costWon"].is_null());
        assert_eq!(active["inferenceModel"]["model"],"user-selected-27b");
        let invalid=find(State(state.clone()),Json(json!({"mode":"agent","agentId":"nonexistent"}))).await;
        assert_eq!(invalid.status(),StatusCode::BAD_REQUEST);
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn one_click_persists_drafts_without_any_model_or_expense(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-money-one-click-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let source=json!({"id":"buy-1","title":"Government agency publishes software purchase request",
            "url":"https://example.com/buyer","sourceName":"Example buyer",
            "changeTags":["buyer_request"],"topics":["software"],
            "capturedAt":"2026-11-17T00:00:00Z"});
        state.store.put("money_signal",&source).unwrap();
        state.store.put("provider",&json!({"id":"p1","enabled":true,"kind":"openai",
            "model":"sample-model","baseUrl":"http://127.0.0.1:9/v1"})).unwrap();
        let response=find(State(state.clone()),Json(json!({"mode":"db"}))).await;
        assert_eq!(response.status(),StatusCode::OK);
        let first=latest(State(state.clone())).await.0;
        assert_eq!(first["modelCalls"],0);
        assert_eq!(first["selectedAgentId"],"db");
        assert!(first["inferenceJobId"].is_null());
        let drafts=first["drafts"].as_array().unwrap();
        assert_eq!(drafts.len(),1);
        assert_eq!(drafts[0]["status"],"hypothesis");
        assert!(drafts[0]["economics"]["priceWon"].is_null());
        assert_eq!(drafts[0]["evidence"].as_array().unwrap().len(),1);
        assert!(state.store.get("money_campaign",CAMPAIGN_ID).is_some());
        let id=drafts[0]["id"].as_str().unwrap().to_string();
        find(State(state.clone()),Json(json!({}))).await;
        assert!(state.store.get("money_candidate",&id).is_some());
        assert_eq!(state.store.money_recent_records("money_candidate",10).unwrap().len(),1);
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn two_independent_sources_produce_an_explicit_causal_conjecture(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-money-abduction-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("money_signal",&json!({"id":"supplier","url":"https://supply.example.org/release",
            "title":"Factory software costs decline after release"})).unwrap();
        state.store.put("money_signal",&json!({"id":"customer","url":"https://buyer.example.org/requirements",
            "title":"Construction buyer requests cost accounting tools"})).unwrap();
        let pair=json!({"id":"tension-1","supplySignalId":"supplier","buyerSignalId":"customer",
            "mechanism":"new_tool_to_buyer","buyerChange":"buyer_request","researchPriority":86,
            "nextProof":"Check compatibility"});
        let hypothesis=candidate_from_pair(&state,&pair).unwrap();
        let steps=hypothesis["inferenceSteps"].as_array().unwrap();
        assert!(steps.len()>=4);
        assert!(steps[0].as_str().unwrap().contains("Factory software"));
        assert!(steps[1].as_str().unwrap().contains("Construction buyer"));
        assert!(hypothesis["counterfactual"].as_str().unwrap().contains("규격"));
        assert!(hypothesis["economics"]["priceWon"].is_null());
        assert_eq!(hypothesis["evidence"].as_array().unwrap().len(),2);
        assert!(hypothesis["buyerEvidence"].as_array().unwrap().is_empty());
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn discovery_priority_respects_publication_dates_not_crawl_time(){
        let now=chrono::DateTime::parse_from_rfc3339("2026-11-17T00:00:00Z")
            .unwrap().with_timezone(&chrono::Utc);
        let fresh=json!({"origin":"source_pair_draft","researchPriority":70,
            "buyerSignal":"buyer_request","evidence":[
                {"publishedAt":"2026-11-15T00:00:00Z","capturedAt":"2026-11-17T00:00:00Z"},
                {"publishedAt":"2026-11-16T00:00:00Z","capturedAt":"2026-11-17T00:00:00Z"}]});
        let stale=json!({"origin":"source_pair_draft","researchPriority":70,
            "buyerSignal":"buyer_request","evidence":[
                {"publishedAt":"2025-01-01T00:00:00Z","capturedAt":"2026-11-17T00:00:00Z"},
                {"publishedAt":"2025-01-01T00:00:00Z","capturedAt":"2026-11-17T00:00:00Z"}]});
        assert!(discovery_priority(&fresh,&now)>discovery_priority(&stale,&now));
        assert!(publication_age(&stale["evidence"],&now).unwrap()>180);
        assert_eq!(publication_age(&json!([{"capturedAt":"2026-11-17T00:00:00Z"}]),&now),None);
    }
    #[test]
    fn unseen_leads_first_rejected_excluded_and_history_is_bounded(){
        let pool=(0..5).map(|i|json!({"id":format!("lead-{i}"),
            "status":if i==2{"rejected"}else{"hypothesis"}})).collect::<Vec<_>>();
        let (first,shown,cursor)=next_drafts(&pool,&Value::Null,3);
        assert_eq!(first.iter().filter_map(|row|row["id"].as_str()).collect::<Vec<_>>(),
            vec!["lead-0","lead-1","lead-3"]);
        let (second,history,_)=next_drafts(&pool,&json!({"cursor":cursor,"shownCandidateIds":shown}),9);
        assert_eq!(second[0]["id"],"lead-4");
        assert!(second.iter().all(|row|row["id"]!="lead-2"));
        assert_eq!(history.len(),4);
    }
    #[test]
    fn source_pair_has_real_urls_no_fake_budget(){
        let (product,_,_)=choose_model("new_tool_to_buyer");
        assert!(product.contains("구매"));
        let source=json!({"id":"s","url":"https://example.com/rfp","title":"Customer has public software procurement","changeTags":["buyer_request"],"topics":["software"]});
        let draft=candidate_from_signal(&source).unwrap();
        assert_eq!(draft["status"],"hypothesis");
        assert!(draft["economics"]["priceWon"].is_null());
        assert_eq!(draft["evidence"].as_array().unwrap().len(),1);
        assert!(candidate_from_signal(&json!({"id":"x","url":"javascript:bad","title":"Long enough headline","changeTags":["buyer_request"]})).is_none());
    }
}
