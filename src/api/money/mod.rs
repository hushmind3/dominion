//! Persistent Money Lab API. All writes are domain-scoped; the native agent
//! runtime, global memory and the main branch require no new execution engine.
mod find;
mod implement;
mod review;
use crate::{engine, state::AppState};
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json, Value};
use super::response::error;

fn capped(value:&Value,max:usize)->String{
    value.as_str().unwrap_or("").trim().chars().take(max).collect()
}
fn valid_won(value:&Value,max:f64)->Option<f64>{
    value.as_f64().filter(|n|n.is_finite()&&*n>=0.0&&*n<=max)
}
fn fail(message:&str)->Response{error(StatusCode::BAD_REQUEST,message)}
fn now()->String{chrono::Utc::now().to_rfc3339()}

fn experiment_cost(e:&Value)->f64{
    ["reportedCostWon","marketingCostWon","modelCostWon","laborCostWon"].iter()
        .filter_map(|key|e[*key].as_f64().filter(|v|v.is_finite()&&*v>=0.0)).sum()
}
fn recovery_summary(campaign:&Value,ledger:&Value)->Value{
    let revenue=ledger["revenue"].as_f64().unwrap_or(0.0);
    let direct=ledger["direct"].as_f64().unwrap_or(0.0);
    let marketing=ledger["marketing"].as_f64().unwrap_or(0.0);
    let model=ledger["model"].as_f64().unwrap_or(0.0);
    let labor=ledger["labor"].as_f64().unwrap_or(0.0);
    let total_cost=direct+marketing+model+labor;
    let net=revenue-total_cost;
    let hardware=campaign["hardwareInvestmentWon"].as_f64().unwrap_or(0.0).max(0.0);
    json!({"campaignId":campaign["id"],"reportedRevenueWon":revenue,
        "reportedCostWon":total_cost,"reportedNetWon":net,
        "hardwareInvestmentWon":hardware,"unrecoveredWon":(hardware-net).max(0.0),
        "recoveryPct":if hardware>0.0{
            json!(((net/hardware).max(0.0)*1000.0).round()/10.0)
        }else{Value::Null},
        "completedExperiments":ledger["finished"],
        "experimentCount":ledger["experiments"],
        "source":"user_reported_not_audited"})
}

fn decorate(mut candidate:Value,experiments:&[Value],capital:f64)->Value{
    candidate["analysis"]=engine::money::economics(&candidate);
    candidate["priority"]=engine::money_rank::rank_candidate(&candidate,experiments,capital);
    candidate["forecastAccuracy"]=engine::money_rank::forecast_error(&candidate,experiments);
    let id=candidate["id"].as_str().unwrap_or("");
    let experiments=experiments.iter().filter(|e|e["candidateId"]==id).collect::<Vec<_>>();
    let total_revenue=experiments.iter().filter_map(|e|e["reportedRevenueWon"].as_f64()).sum::<f64>();
    let total_cost=experiments.iter().map(|e|experiment_cost(e)).sum::<f64>();
    candidate["observations"]=json!({
        "experiments":experiments.len(),"reportedRevenueWon":total_revenue,
        "reportedCostWon":total_cost,"reportedNetWon":total_revenue-total_cost
    });
    candidate
}

/// Compact research/accumulation counters for the Money screen. Cheap reads
/// only: no model calls and no full candidate rendering.
pub async fn stats(State(state):State<AppState>)->Json<Value>{
    let signals=state.store.count_kind("money_signal");
    let candidates=state.store.all("money_candidate");
    let model= candidates.iter().filter(|c|c["origin"]=="model").count();
    let drafts=candidates.len()-model;
    let tensions=state.store.count_kind("money_tension");
    let asymmetries=state.store.count_kind("money_asymmetry");
    let mut ingest=state.store.all("money_ingest_report");
    ingest.sort_by(|a,b|b["capturedAt"].as_str().cmp(&a["capturedAt"].as_str()));
    let last=ingest.first().cloned().unwrap_or(Value::Null);
    let mut by_model=std::collections::BTreeMap::<String,(u64,u64)>::new();
    for job in state.store.all("job").into_iter().filter(|j|j["kind"]=="money_discover"){
        let name=job["result"]["modelUsed"]["model"].as_str()
            .or_else(||job["input"]["model"].as_str())
            .unwrap_or("미지정").to_string();
        let entry=by_model.entry(name).or_insert((0,0));
        match job["status"].as_str(){
            Some("completed")=>entry.0+=1,
            Some("failed"|"needs_review"|"cancelled")=>entry.1+=1,
            _=>{}
        }
    }
    let models=by_model.into_iter().map(|(name,(ok,fail))|
        json!({"model":name,"completed":ok,"failed":fail}))
        .collect::<Vec<_>>();
    let autopilot=state.store.all("money_autopilot").into_iter().find(|a|a["enabled"]==true)
        .map(|a|json!({"enabled":true,"rounds":a["completedRounds"],
            "nextAt":a["nextAt"],"rotation":a["freeModels"].as_array().map(|v|v.len()).unwrap_or(0),
            "lastFailure":a["lastFailure"]}))
        .unwrap_or(json!({"enabled":false}));
    Json(json!({"signals":signals,"candidates":candidates.len(),"modelCandidates":model,
        "templateDrafts":drafts,"tensions":tensions,"asymmetries":asymmetries,
        "lastIngest":last,"models":models,"autopilot":autopilot}))
}

pub async fn overview(State(state):State<AppState>)->Json<Value>{
    let campaigns=state.store.money_recent_records("money_campaign",100).unwrap_or_default();
    let candidates=state.store.money_recent_records("money_candidate",300).unwrap_or_default();
    let reports=state.store.money_recent_records("money_report",100).unwrap_or_default();
    let experiments=state.store.money_recent_records("money_experiment",300).unwrap_or_default();
    let summaries=campaigns.iter().map(|c|{
        let ledger=state.store.money_ledger_totals(c["id"].as_str().unwrap_or(""))
            .unwrap_or(json!({}));
        recovery_summary(c,&ledger)
    }).collect::<Vec<_>>();
    let mut ranked=candidates.into_iter().map(|candidate|{
        let campaign_capital=campaigns.iter().find(|c|c["id"]==candidate["campaignId"])
            .and_then(|c|c["capitalWon"].as_f64()).unwrap_or(-1.0);
        decorate(candidate,&experiments,campaign_capital)
    }).collect::<Vec<_>>();
    ranked.sort_by(|a,b|b["priority"]["score"].as_u64().cmp(&a["priority"]["score"].as_u64())
        .then_with(||b["createdAt"].as_str().cmp(&a["createdAt"].as_str())));
    let hotlist=ranked.iter().filter(|c|c["status"]!="rejected").take(8).cloned().collect::<Vec<_>>();
    let market=state.store.money_recent_signals(80).unwrap_or_default();
    let researchTensions=state.store.money_best_tensions(15).unwrap_or_default();
    let marketTrends=state.store.money_topic_trends().unwrap_or_default();
    let graphSample=state.store.money_graph_recent(40).unwrap_or_default();
    let explorationStrategies=engine::money_rank::strategy_stats(&ranked,&experiments);
    let techMetricChanges=state.store.money_recent_metric_changes(18).unwrap_or_default();
    let pendingAsymmetries=state.store.money_pending_asymmetries(20).unwrap_or_default();
    let counts=state.store.money_corpus_stats().unwrap_or(json!({}));
    let latest_scan=state.store.money_latest_ingestion().ok().flatten();
    let operational=state.store.money_operational_scorecard()
        .unwrap_or_else(|e|json!({"error":e,"windowHours":24}));
    Json(json!({"campaigns":campaigns,
        "operationalScorecard":operational,
        "campaignSummaries":summaries,
        "candidates":ranked,
        "hotlist":hotlist,"researchTensions":researchTensions,
        "marketTrends":marketTrends,"graphSample":graphSample,
        "explorationStrategies":explorationStrategies,
        "techMetricChanges":techMetricChanges,
        "pendingAsymmetries":pendingAsymmetries,
        "reports":reports,
        "experiments":experiments,
        "marketSignals":market,
        "marketStats":counts,
        "marketLastScan":latest_scan,
        "autopilots":state.store.all("money_autopilot") }))
}

/// Search the existing source-backed SQLite knowledge index. No model/tool calls.
/// The archive is already populated by the independent Money collector.
pub async fn research(
    State(state):State<AppState>,
    Query(params):Query<std::collections::HashMap<String,String>>,
)->Response{
    let query=params.get("q").map(|s|s.trim()).unwrap_or("");
    if query.chars().count()>120{
        return error(StatusCode::BAD_REQUEST,"검색어는 최대 120자입니다.");
    }
    // Empty search is a short read from the existing source archive, not the
    // most recent model-authored claims in the shared facts index.
    let hits=if query.is_empty(){
        state.store.money_recent_signals(16).map(|signals|signals.into_iter().map(|row|
            json!({"id":row["id"],"claim":format!("{} {}",
                capped(&row["title"],200),capped(&row["description"],250)).trim(),
                "url":row["url"],"sourceName":row["sourceName"],
                "capturedAt":row["capturedAt"],"verification":"archived_source_observation"}))
            .collect::<Vec<_>>())
    }else{
        // Existing indexed bigram/FTS5 query covers the full durable archive.
        // Search never invokes the LLM or interprets a headline as a paid order.
        state.store.money_fact_search(query,40).map(|facts|facts.into_iter()
            .filter(|row|row["kind"]=="market_source")
            .take(16).map(|row|json!({
                "id":row["id"],"claim":row["claim"],"url":row["url"],
                "sourceName":row["sourceName"],"capturedAt":row["createdAt"],
                "verification":row["verification"],
            })).collect::<Vec<_>>())
    };
    match hits{
        Ok(hits)=>Json(json!({"query":query,"hits":hits,"mode":"indexed_sqlite_no_model",
            "sourceType":"archived_source_observations","modelCalls":0})).into_response(),
        Err(err)=>error(StatusCode::INTERNAL_SERVER_ERROR,&err)
    }
}

pub async fn create_campaign(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let name=capped(&body["name"],100);
    let brief=capped(&body["brief"],3000);
    if name.is_empty()||brief.is_empty(){return fail("이름과 사업 발굴 목표를 입력하세요.");}
    // No user-supplied budget, revenue target or time restriction means
    // unrestricted discovery, not an invented zero-budget or monthly target.
    let capital=match body.get("capitalWon"){
        Some(value) if !value.is_null()=>match valid_won(value,1e14){
            Some(n)=>Some(n),None=>return fail("가용 자본은 0원 이상의 숫자여야 합니다.")},
        _=>None
    };
    let target=match body.get("targetMonthlyProfitWon"){
        Some(value) if !value.is_null()=>match valid_won(value,1e14){
            Some(n)=>Some(n),None=>return fail("목표 월이익은 0원 이상의 숫자여야 합니다.")},
        _=>None
    };
    let hours=match body.get("hoursPerWeek"){
        Some(value) if !value.is_null()=>match value.as_f64()
            .filter(|n|n.is_finite()&&*n>=0.0&&*n<=168.0){
            Some(n)=>Some(n),None=>return fail("주당 투입 가능 시간은 0~168시간이어야 합니다.")},
        _=>None
    };
    let hardware=match body.get("hardwareInvestmentWon") {
        Some(v) if !v.is_null()=>match valid_won(v,1e14){
            Some(n)=>Some(n),None=>return fail("장비 투자금이 올바르지 않습니다.")},
        _=>None
    };
    let candidate=json!({"name":name,"brief":brief,"capitalWon":capital,"targetMonthlyProfitWon":target,
        "hardwareInvestmentWon":hardware,
        "hoursPerWeek":hours,"skills":capped(&body["skills"],1000),
        "distribution":capped(&body["distribution"],1000),"exclusions":capped(&body["exclusions"],700),
        "preferences":capped(&body["preferences"],700),"markets":capped(&body["markets"],350),
        "createdAt":now()});
    match state.store.put("money_campaign",&candidate){
        Some(saved)=>{state.bus.record("money_campaign",&saved);(StatusCode::CREATED,Json(saved)).into_response()},
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,"사업 설정을 저장하지 못했습니다.")
    }
}

pub async fn campaign(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match state.store.get("money_campaign",&id){
        Some(item)=>Json(item).into_response(),
        None=>error(StatusCode::NOT_FOUND,"사업 탐색을 찾지 못했습니다.")
    }
}

pub async fn update_campaign(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(prior)=state.store.get("money_campaign",&id)else{
        return error(StatusCode::NOT_FOUND,"캠페인이 없습니다.");
    };
    let hardware=match valid_won(&body["hardwareInvestmentWon"],1e14){
        Some(v)=>v,None=>return fail("장비 투자금은 0원 이상의 숫자여야 합니다.")
    };
    let saved=state.store.mutate_record("money_campaign",&id,prior,|item|{
        item["hardwareInvestmentWon"]=json!(hardware);
        item["updatedAt"]=json!(now());
        Ok(vec![])
    });
    match saved{
        Ok(record)=>{state.bus.record("money_campaign",&record);Json(record).into_response()},
        Err(err)=>error(StatusCode::INTERNAL_SERVER_ERROR,&err)
    }
}

pub async fn start(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(campaign)=state.store.get("money_campaign",&id)else{return error(StatusCode::NOT_FOUND,"사업 탐색을 찾지 못했습니다.");};
    let mode=body["mode"].as_str().unwrap_or("discover");
    if !matches!(mode,"signals"|"discover"|"cross"|"challenge"|"mutate"|"infer"){return fail("지원하지 않는 탐색 방식입니다.");}
    let current_job=campaign["lastJobId"].as_str().and_then(|key|state.store.get("job",key));
    if current_job.as_ref().is_some_and(|job|matches!(job["status"].as_str(),Some("queued"|"running"))){
        return error(StatusCode::CONFLICT,"이 캠페인에서 실행 중인 작업이 있습니다.");
    }
    let focus=body["candidateId"].as_str().and_then(|key|state.store.get("money_candidate",key));
    if mode=="mutate" && focus.is_none(){return fail("재조합할 사업 후보를 선택하세요.");}
    if let Some(candidate)=&focus{
        if candidate["campaignId"].as_str()!=Some(id.as_str()){return fail("다른 탐색의 후보입니다.");}
    }
    let prompt=engine::money::prompt(&state,&campaign,mode,focus.as_ref());
    let agent=body["agentId"].as_str().filter(|v|!v.trim().is_empty());
    if agent.is_some_and(|agent|state.store.get("agent",agent).is_none()){return fail("선택한 에이전트가 존재하지 않습니다.");}
    let mut input=json!({"prompt":prompt,"campaignId":id,"mode":mode,
        "toolsEnabled":mode!="infer","toolsExecutionEnabled":mode!="infer","tokenBudget":body["tokenBudget"].as_u64().unwrap_or(12000),
        "asymmetryBatchIds":state.store.money_pending_asymmetries(8).unwrap_or_default()
            .iter().filter_map(|v|v["id"].as_str()).collect::<Vec<_>>()});
    for key in ["agentId","providerId","model","reasoningEffort"]{
        if let Some(value)=body[key].as_str().filter(|v|!v.trim().is_empty()&&v.len()<=200){
            input[key]=json!(value);
        }
    }
    // Money owns its bounded research cycle and structured result parser.
    // Do not route a single discovery round through the generic multi-attempt
    // repair loop; the ordinary job runtime still enforces user cancellation,
    // model usage and the existing tool approval policy.
    input["autonomous"]=json!(false);
    let queued=json!({"id":uuid::Uuid::new_v4().to_string(),"kind":"money_discover",
        "status":"queued","queuedAt":now(),"input":input,
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]});
    match state.store.money_claim_job(&id,&queued,None){
        Ok(Some(job))=>{
            state.bus.record("job",&job);
            state.store.audit("job.queued",json!({"kind":"money_discover","jobId":job["id"]}));
            (StatusCode::ACCEPTED,Json(job)).into_response()
        },
        Ok(None)=>error(StatusCode::CONFLICT,"이 사업에서 다른 탐색이 진행 중입니다."),
        Err(message)=>error(StatusCode::INTERNAL_SERVER_ERROR,&message)
    }
}

/// Explicit opt-in for bounded unattended exploration. No open-ended
/// autonomous spending or infinite context accumulation.
pub async fn rotation(State(state):State<AppState>)->Json<Value>{
    Json(json!({"models":free_rotation(&state)}))
}
pub async fn autopilot_status(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if state.store.get("money_campaign",&id).is_none(){return error(StatusCode::NOT_FOUND,"사업 탐색이 없습니다.");}
    Json(state.store.get("money_autopilot",&id).unwrap_or(json!({"id":id,"enabled":false}))).into_response()
}

/// Verified free chat models, resolved to this install's provider IDs by name.
/// Local runtimes are deliberately excluded.
const FREE_ROTATION:&[(&str,&str)]=&[
    ("z.ai","glm-4.7-flash"),
    ("z.ai","glm-4.5-flash"),
    ("Cloudflare Workers AI","@cf/zai-org/glm-4.7-flash"),
    ("Cloudflare Workers AI","@cf/nvidia/nemotron-3-120b-a12b"),
    ("Cloudflare Workers AI","@cf/google/gemma-4-26b-a4b-it"),
    ("Groq","qwen/qwen3.8-27b"),
    ("Groq","openai/gpt-oss-120b"),
];
fn free_rotation(state:&AppState)->Vec<Value>{
    let providers=state.store.all("provider");
    FREE_ROTATION.iter().filter_map(|(name,model)|{
        let provider=providers.iter().find(|p|p["name"].as_str()==Some(name)
            && p["enabled"]!=false && p["local"]!=true)?;
        let id=provider["id"].as_str()?;
        Some(json!({"providerId":id,"model":model,"providerName":name}))
    }).collect()
}

pub async fn autopilot_set(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    // The one-click Money home can enable a bounded collection/autopilot
    // before its first evidence query. Create only the well-known default
    // workspace, never silently invent arbitrary campaign identifiers.
    if id=="money:auto" && body["enabled"]==true && state.store.get("money_campaign",&id).is_none(){
        let initial=json!({"id":"money:auto","name":"자동 돈벌이 발굴",
            "brief":"서로 다른 산업과 시장에서 실제 지불 수요를 연구",
            "capitalWon":null,"hardwareInvestmentWon":null,
            "targetMonthlyProfitWon":null,"hoursPerWeek":null,
            "skills":"","markets":"","preferences":"","distribution":"","exclusions":"",
            "createdAt":chrono::Utc::now().to_rfc3339()});
        let _=state.store.put_if_absent("money_campaign",&initial);
    }
    let Some(campaign)=state.store.get("money_campaign",&id) else{
        return error(StatusCode::NOT_FOUND,"사업 탐색이 없습니다.");
    };
    let current=chrono::Utc::now();
    let prior=state.store.get("money_autopilot",&id).unwrap_or(json!({"id":id,"enabled":false}));
    if body["enabled"]==false{
        let result=state.store.mutate_record("money_autopilot",&id,prior,|row|{
            let remaining=row["endsAt"].as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|v|(v.with_timezone(&chrono::Utc)-current).num_seconds().max(0)).unwrap_or(0);
            if row["enabled"]==true{
                row["remainingSeconds"]=json!(remaining);
            }
            row["enabled"]=json!(false);
            row["stopReason"]=json!("paused_by_owner");
            row["updatedAt"]=json!(current.to_rfc3339());
            Ok(vec![])
        });
        return match result {
            Ok(saved)=>Json(saved).into_response(),
            Err(err)=>error(StatusCode::INTERNAL_SERVER_ERROR,&err),
        };
    }
    if body["enabled"]!=true{return fail("enabled는 true 또는 false여야 합니다.");}
    if let Some(id)=campaign["lastJobId"].as_str(){
        if state.store.get("job",id).is_some_and(|j|
            matches!(j["status"].as_str(),Some("queued"|"running")))
            && body["resume"]!=true{
            return error(StatusCode::CONFLICT,"이전 탐색 작업이 실행 중입니다.");
        }
    }
    let resume=body["resume"]==true;
    let auto=if resume {
        if prior["stopReason"]!="paused_by_owner"||prior["runId"].as_str().is_none(){
            return fail("중지했던 탐색만 이어서 실행할 수 있습니다.");
        }
        if prior["continuous"]!=true && prior["remainingSeconds"].as_i64().unwrap_or(0)<=0 {
            return fail("이전 탐색의 남은 운영 시간이 없습니다. 새 탐색을 시작하세요.");
        }
        let mut next=prior.clone();
        next["enabled"]=json!(true);
        next["stopReason"]=Value::Null;
        next["nextAt"]=json!(current.to_rfc3339());
        next["endsAt"]=if prior["continuous"]==true {Value::Null}
            else{json!((current+chrono::Duration::seconds(prior["remainingSeconds"].as_i64().unwrap_or(0))).to_rfc3339())};
        next["updatedAt"]=json!(current.to_rfc3339());
        next
    }else{
        let continuous=body["continuous"]==true;
        let duration=body["durationHours"].as_u64().unwrap_or(24);
        let interval=body["intervalMinutes"].as_u64().unwrap_or(20);
        let max_rounds=body["maxRounds"].as_u64().unwrap_or(36);
        let tokens=body["tokenBudget"].as_u64().unwrap_or(12000);
        if (!continuous&&!(1..=24).contains(&duration))||!(10..=240).contains(&interval)
            ||!(1..=144).contains(&max_rounds)||!(1024..=120000).contains(&tokens)
            ||tokens.saturating_mul(max_rounds)>12_000_000 {
            return fail("자율 탐색 한도: 최대 24시간, 10~240분 간격, 144회, 회당 120000토큰, 최대 1200만 토큰입니다.");
        }
        if let Some(agent)=body["agentId"].as_str().filter(|v|!v.is_empty()){
            if state.store.get("agent",agent).is_none(){return fail("존재하지 않는 에이전트입니다.");}
        }
        json!({"id":id,"runId":uuid::Uuid::new_v4().to_string(),
            "enabled":true,"continuous":continuous,"durationHours":duration,"intervalMinutes":interval,
            "maxRounds":max_rounds,"tokenBudget":tokens,"maximumReservedTokens":tokens*max_rounds,
            "scheduledRounds":0,"dailyScheduledRounds":0,
            "dayKey":current.timestamp().div_euclid(86400),
            "completedRounds":0,"lastAccountedJobId":null,"lastJobId":null,
            "agentId":capped(&body["agentId"],100),"providerId":capped(&body["providerId"],100),
            "model":capped(&body["model"],150),
            // Optional round-robin list of {providerId, model} pairs. Each round
            // uses the next entry, so one slow or blocked free model does not
            // stall the whole run.
            "freeModels":if body["freeRotation"]==true{free_rotation(&state)}
                else{body["freeModels"].as_array().cloned().unwrap_or_default()},
            "consecutiveFailures":0,
            "createdAt":current.to_rfc3339(),
            "nextAt":current.to_rfc3339(),
            "remainingSeconds":if continuous{Value::Null}else{json!(duration*3600)},
            "endsAt":if continuous{Value::Null}else{json!((current+chrono::Duration::hours(duration as i64)).to_rfc3339())},
            "stopReason":null})
    };
    // The enabled check runs inside SQLite's write transaction. Concurrent
    // requests never reset each other's generation or replay round one.
    let result=state.store.mutate_record("money_autopilot",&id,prior,|row|{
        if row["enabled"]==true{return Err("already_active".into());}
        *row=auto.clone();
        Ok(vec![])
    });
    match result {
        Ok(saved)=>{
            engine::money::tick(&state);
            Json(state.store.get("money_autopilot",&id).unwrap_or(saved)).into_response()
        },
        Err(err) if err=="already_active"=>
            error(StatusCode::CONFLICT,"이미 실행 중인 자율 탐색입니다."),
        Err(err)=>error(StatusCode::INTERNAL_SERVER_ERROR,&err)
    }
}

/// Save a source-backed research draft as a selectable Money candidate.
 /// It is not a model conclusion, a known buyer, or a commercial commitment.
pub async fn promote_draft(
    State(state):State<AppState>,
    Path((campaign_id,tension_id)):Path<(String,String)>,
)->Response{
    if state.store.get("money_campaign",&campaign_id).is_none(){
        return error(StatusCode::NOT_FOUND,"발굴 프로젝트가 없습니다.");
    }
    let Some(tension)=state.store.get("money_tension",&tension_id)else{
        return error(StatusCode::NOT_FOUND,"연결된 시장 근거가 없습니다.");
    };
    let Some(supply_id)=tension["supplySignalId"].as_str()else{return fail("공급 근거가 없습니다.");};
    let Some(buyer_id)=tension["buyerSignalId"].as_str()else{return fail("수요 근거가 없습니다.");};
    if supply_id==buyer_id{return fail("서로 독립된 출처가 필요합니다.");}
    let Some(supply)=state.store.get("money_signal",supply_id)else{return fail("원본 공급 출처가 없습니다.");};
    let Some(buyer)=state.store.get("money_signal",buyer_id)else{return fail("원본 고객 문제 출처가 없습니다.");};
    let supply_url=capped(&supply["url"],1500);
    let buyer_url=capped(&buyer["url"],1500);
    if supply_url.is_empty()||buyer_url.is_empty()||supply_url==buyer_url{
        return fail("독립적인 원문 URL 2개를 확인해야 합니다.");
    }
    let id=format!("money:draft:{campaign_id}:{tension_id}");
    if let Some(saved)=state.store.get("money_candidate",&id){
        return Json(saved).into_response();
    }
    let question=capped(&tension["question"],900);
    let proof=capped(&tension["nextProof"],900);
    let supply_title=capped(&supply["title"],220);
    let buyer_title=capped(&buyer["title"],220);
    let candidate=json!({
        "id":id,"campaignId":campaign_id,"jobId":null,"sessionId":null,
        "status":"hypothesis","origin":"source_pair_draft","evidenceStatus":"two_source_research_question_only",
        "title":format!("시장 연결 초안 · {}",buyer_title).chars().take(160).collect::<String>(),
        "offer":question,"customer":"구매 문제 원문에 등장하는 고객 또는 조직 · 검증 전",
        "problem":buyer_title,"mechanism":question,"whyPay":"구매 문제와 공급 변화가 결합되면 비용 절감이나 대체 판매 기회가 생길 수 있다는 조사 가설",
        "firstBuyer":"","firstPaymentStep":proof,"implementationPlan":format!(
            "1. 구매 문제 원문 확인: {} 2. 대체 공급의 원문·규격 확인: {} 3. 결제 주체, 구매 단가, 수요 규모 확인 4. 소규모 유료 실험 검토",
            buyer_title,supply_title),
        "firstTest":proof,"killSignal":"가격·규격·결제권자 조사에서 유료 수요가 발견되지 않으면 폐기",
        "distribution":"","defensibility":"","operators":["source_pair"],"links":[],
        "buyerEvidence":[],"assumptions":["원문 간 인과관계 및 실제 유료 수요 미검증"],
        "evidence":[
            engine::money::provenance::evidence(&buyer,&buyer_title,"buyer_side_observation"),
            engine::money::provenance::evidence(&supply,&supply_title,"supply_side_observation")],
        "economics":{"priceWon":null,"variableCostWon":null,"acquisitionCostWon":null,
                     "monthlyFixedWon":null,"monthlyUnits":null,"setupCostWon":null,"refundRatePct":null},
        "createdAt":now()
    });
    match state.store.put("money_candidate",&candidate){
        Some(saved)=>{
            state.bus.record("money_candidate",&saved);
            (StatusCode::CREATED,Json(saved)).into_response()
        }
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,"후보 초안 저장에 실패했습니다.")
    }
}

pub async fn candidate(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match state.store.get("money_candidate",&id){
        Some(v)=>{
            let experiments=state.store.all("money_experiment").into_iter()
                .filter(|e|e["candidateId"]==id).collect::<Vec<_>>();
            let campaign_id=v["campaignId"].as_str().unwrap_or("");
            let capital=state.store.get("money_campaign",campaign_id)
                .and_then(|c|c["capitalWon"].as_f64()).unwrap_or(-1.0);
            let decisions=state.store.money_candidate_decisions(&id,50)
                .unwrap_or_default();
            Json(json!({"candidate":decorate(v,&experiments,capital),
                "experiments":experiments,"decisions":decisions})).into_response()
        },
        None=>error(StatusCode::NOT_FOUND,"사업 후보를 찾지 못했습니다.")
    }
}
/// Zero-LLM draft. No publication, payment processing or external outreach.
pub async fn create_validation_kit(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response {
    let Some(candidate)=state.store.get("money_candidate",&id)else{
        return error(StatusCode::NOT_FOUND,"사업 후보를 찾지 못했습니다.");
    };
    let Some(campaign)=candidate["campaignId"].as_str()
        .and_then(|id|state.store.get("money_campaign",id))else{
        return error(StatusCode::NOT_FOUND,"캠페인을 찾지 못했습니다.");
    };
    let kit=match engine::money_kit::create(&candidate,&campaign,body["contactUrl"].as_str()){
        Ok(v)=>v,Err(err)=>return fail(&err)
    };
    match state.store.put("money_validation_kit",&kit){
        Some(saved)=>(StatusCode::CREATED,Json(saved)).into_response(),
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,"검증 자료를 저장하지 못했습니다.")
    }
}

pub async fn record_experiment(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(candidate)=state.store.get("money_candidate",&id)else{return error(StatusCode::NOT_FOUND,"사업 후보를 찾지 못했습니다.");};
    let name=capped(&body["name"],200);
    if name.is_empty(){return fail("실험 이름이 필요합니다.");}
    let revenue=valid_won(&body["reportedRevenueWon"],1e14).unwrap_or(0.0);
    let cost=valid_won(&body["reportedCostWon"],1e14).unwrap_or(0.0);
    if body.get("reportedRevenueWon").is_some()&&valid_won(&body["reportedRevenueWon"],1e14).is_none()
    ||body.get("reportedCostWon").is_some()&&valid_won(&body["reportedCostWon"],1e14).is_none(){
        return fail("매출과 실험비는 0 이상의 유효한 금액이어야 합니다.");
    }
    let status=body["status"].as_str().unwrap_or("planned");
    if !matches!(status,"planned"|"running"|"finished"|"failed"){return fail("실험 상태가 올바르지 않습니다.");}
    if status=="finished"&&(body.get("reportedRevenueWon").is_none()
        ||body.get("reportedCostWon").is_none()){
        return fail("완료된 실험은 보고 매출과 직접 비용을 명시해야 합니다.");
    }
    let cost_field=|name:&str|->Result<Value,String>{
        match body.get(name){
            Some(value)=>valid_won(value,1e14).map(|v|json!(v))
                .ok_or_else(||format!("{name} 값이 올바르지 않습니다.")),
            None=>Ok(Value::Null)
        }
    };
    let marketing=match cost_field("marketingCostWon"){Ok(v)=>v,Err(err)=>return fail(&err)};
    let model=match cost_field("modelCostWon"){Ok(v)=>v,Err(err)=>return fail(&err)};
    let labor=match cost_field("laborCostWon"){Ok(v)=>v,Err(err)=>return fail(&err)};
    let complete_costs=marketing.is_number()&&model.is_number()&&labor.is_number();
    let experiment=json!({"candidateId":id,"campaignId":candidate["campaignId"],"name":name,"status":status,
        "hypothesis":capped(&body["hypothesis"],1500),
        "method":capped(&body["method"],2000),
        "observations":capped(&body["observations"],3000),
        "reportedRevenueWon":revenue,"reportedCostWon":cost,
        "marketingCostWon":marketing,"modelCostWon":model,"laborCostWon":labor,
        "costReportingComplete":complete_costs,
        "result":capped(&body["result"],1500),
        "evidenceUrl":capped(&body["evidenceUrl"],2000),
        "source":"user_reported","createdAt":now()});
    match state.store.put("money_experiment",&experiment){
        Some(saved)=>{state.bus.record("money_experiment",&saved);(StatusCode::CREATED,Json(saved)).into_response()},
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,"실험을 기록하지 못했습니다.")
    }
}

#[cfg(test)]
mod tests{
    use super::*;

    #[tokio::test]
    async fn money_home_can_enable_autopilot_before_any_research_draft_exists(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-money-start-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let response=autopilot_set(State(state.clone()),Path("money:auto".into()),
            Json(json!({"enabled":true,"continuous":true,"intervalMinutes":30,
                "maxRounds":2,"tokenBudget":1024}))).await;
        assert_eq!(response.status(),StatusCode::OK);
        assert!(state.store.get("money_campaign","money:auto").is_some());
        let auto=state.store.get("money_autopilot","money:auto").unwrap();
        assert_eq!(auto["enabled"],true);
        // Other IDs still require explicit creation by the user.
        let foreign=autopilot_set(State(state.clone()),Path("arbitrary".into()),
            Json(json!({"enabled":true}))).await;
        assert_eq!(foreign.status(),StatusCode::NOT_FOUND);
        assert!(state.store.get("money_campaign","arbitrary").is_none());
        let _=std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn unspecified_experiment_costs_remain_unknown(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("money-cost-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("money_candidate",&json!({"id":"c1","campaignId":"camp"})).unwrap();
        let incomplete=record_experiment(State(state.clone()),Path("c1".into()),Json(json!({
            "name":"trial","status":"finished","reportedRevenueWon":10000,"reportedCostWon":1000
        }))).await;
        assert_eq!(incomplete.status(),StatusCode::CREATED);
        let row=state.store.all("money_experiment").pop().unwrap();
        assert_eq!(row["costReportingComplete"],false);
        assert!(row["marketingCostWon"].is_null());
        assert_eq!(engine::money_rank::experiment_net(&row),None);
        let complete=record_experiment(State(state.clone()),Path("c1".into()),Json(json!({
            "name":"fully costed","status":"finished","reportedRevenueWon":10000,
            "reportedCostWon":1000,"marketingCostWon":0,"modelCostWon":0,"laborCostWon":0
        }))).await;
        assert_eq!(complete.status(),StatusCode::CREATED);
        assert!(state.store.all("money_experiment").iter().any(|row|
            row["costReportingComplete"]==true
            &&engine::money_rank::experiment_net(row)==Some(9000.0)));
        let missing=record_experiment(State(state.clone()),Path("c1".into()),Json(json!({
            "name":"missing revenue","status":"finished","reportedCostWon":100
        }))).await;
        assert_eq!(missing.status(),StatusCode::BAD_REQUEST);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test] fn hardware_payback_uses_net_after_all_reported_costs(){
        let campaign=json!({"id":"c1","hardwareInvestmentWon":1_000_000});
        let ledger=json!({"revenue":750000,"direct":100000,"marketing":50000,
            "model":25000,"labor":25000,"finished":1,"experiments":1});
        let result=recovery_summary(&campaign,&ledger);
        assert_eq!(result["reportedNetWon"],550000.0);
        assert_eq!(result["unrecoveredWon"],450000.0);
        assert_eq!(result["recoveryPct"],55.0);
    }
    #[test] fn invalid_amounts_are_rejected(){
        assert_eq!(valid_won(&json!(-1),1000.0),None);
        assert_eq!(valid_won(&json!(1200),1000.0),None);
        assert_eq!(valid_won(&json!(500),1000.0),Some(500.0));
    }
}

#[cfg(test)]
mod source_draft_tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};

    #[tokio::test]
    async fn source_backed_draft_can_be_selected_without_a_model(){
        let dir=std::env::temp_dir().join(format!("dominion-money-draft-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{
            port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("money_campaign",&json!({"id":"c","name":"research","brief":"market"})).unwrap();
        state.store.put("money_signal",&json!({"id":"s","url":"https://supply.example/item",
            "title":"Lower cost source","sourceName":"Supply"})).unwrap();
        state.store.put("money_signal",&json!({"id":"b","url":"https://buyer.example/request",
            "title":"Buyer problem","sourceName":"Buyer"})).unwrap();
        state.store.put("money_tension",&json!({"id":"t","supplySignalId":"s",
            "buyerSignalId":"b","question":"Could a new supply replace existing work?",
            "nextProof":"Check actual paying buyer"})).unwrap();
        let path=Path(("c".to_string(),"t".to_string()));
        let response=promote_draft(State(state.clone()),path).await;
        assert_eq!(response.status(),StatusCode::CREATED);
        let candidate=state.store.get("money_candidate","money:draft:c:t").unwrap();
        assert_eq!(candidate["origin"],"source_pair_draft");
        assert_eq!(candidate["evidence"].as_array().unwrap().len(),2);
        assert_eq!(candidate["evidence"][0]["sourceId"],"b");
        assert_eq!(candidate["evidence"][1]["sourceId"],"s");
        assert_eq!(candidate["evidence"][0]["quoteMatchStatus"],"stored_title_exact");
        assert!(candidate["economics"]["priceWon"].is_null());
        assert_eq!(candidate["status"],"hypothesis");
        let duplicate=promote_draft(State(state.clone()),Path(("c".into(),"t".into()))).await;
        assert_eq!(duplicate.status(),StatusCode::OK);
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn missing_project_does_not_create_market_draft(){
        let dir=std::env::temp_dir().join(format!("dominion-money-no-project-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{
            port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let response=promote_draft(State(state),Path(("missing".into(),"t".into()))).await;
        assert_eq!(response.status(),StatusCode::NOT_FOUND);
        let _=std::fs::remove_dir_all(dir);
    }
}

    
/// All Money HTTP routes are mounted through this optional router.
pub fn routes()->axum::Router<AppState>{
    use axum::routing::{get,post};
    axum::Router::new()
        .route("/api/money",get(overview))
        .route("/api/money/find",get(find::latest).post(find::find))
        .route("/api/money/research",get(research))
        .route("/api/money/stats",get(stats))
        .route("/api/money/rotation",get(rotation))
        .route("/api/money/campaigns",post(create_campaign))
        .route("/api/money/campaigns/:id",get(campaign).post(update_campaign))
        .route("/api/money/campaigns/:id/run",post(start))
        .route("/api/money/campaigns/:id/drafts/:tension_id/promote",post(promote_draft))
        .route("/api/money/campaigns/:id/autopilot",get(autopilot_status).post(autopilot_set))
        .route("/api/money/candidates",get(review::archive))
        .route("/api/money/candidates/:id",get(candidate).post(review::update_candidate))
        .route("/api/money/candidates/:id/history",get(review::history))
        .route("/api/money/candidates/:id/experiments",post(record_experiment))
        .route("/api/money/candidates/:id/implement",post(implement::launch))
        .route("/api/money/candidates/:id/validation-kit",post(create_validation_kit))
}
