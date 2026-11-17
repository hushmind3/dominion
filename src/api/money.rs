//! Persistent Money Lab API. All writes are domain-scoped; the native agent
//! runtime, global memory and the main branch require no new execution engine.
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

/// Compact indexed/aggregated dashboard read: bounded JSON decoding.
pub async fn stats(State(state):State<AppState>)->Response{
    match state.store.money_dashboard_stats(){
        Ok(stats)=>Json(stats).into_response(),
        Err(message)=>error(StatusCode::INTERNAL_SERVER_ERROR,&message),
    }
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
    let options=match engine::job_control::options(&state,&input){Ok(v)=>v,Err(err)=>return fail(&err)};
    for (key,value) in options.as_object().into_iter().flatten(){input[key]=value.clone();}
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
            engine::money::schedule_autopilot(&state);
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
        "evidence":[{"claim":buyer_title,"url":buyer_url,"kind":"source_observation"},
                    {"claim":supply_title,"url":supply_url,"kind":"source_observation"}],
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
            Json(json!({"candidate":decorate(v,&experiments,capital),
                "experiments":experiments})).into_response()
        },
        None=>error(StatusCode::NOT_FOUND,"사업 후보를 찾지 못했습니다.")
    }
}
pub async fn update_candidate(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(mut prior)=state.store.get("money_candidate",&id)else{return error(StatusCode::NOT_FOUND,"사업 후보를 찾지 못했습니다.");};
    if let Some(status)=body["status"].as_str(){
        if !matches!(status,"hypothesis"|"researching"|"testing"|"validated"|"rejected"|"launched"){
            return fail("지원하지 않는 사업 상태입니다.");
        }
        prior["status"]=json!(status);
    }
    for key in ["firstTest","killSignal","distribution","defensibility","notes"]{
        if let Some(value)=body[key].as_str(){prior[key]=json!(value.chars().take(3000).collect::<String>());}
    }
    if let Some(patch)=body["economics"].as_object(){
        let mut economics=prior["economics"].as_object().cloned().unwrap_or_default();
        for key in ["priceWon","variableCostWon","acquisitionCostWon","monthlyFixedWon",
            "monthlyUnits","setupCostWon","refundRatePct"]{
            if let Some(value)=patch.get(key){
                if value.is_null(){economics.insert(key.into(),Value::Null);}
                else if let Some(n)=valid_won(value,if key=="refundRatePct"{100.0}else{1e14}){
                    economics.insert(key.into(),json!(n));
                }else{return fail("경제성 값은 0 이상의 유효한 숫자 또는 비어 있는 값이어야 합니다.");}
            }
        }
        prior["economics"]=json!(economics);
    }
    prior["updatedAt"]=json!(now());
    match state.store.put("money_candidate",&prior){
        Some(v)=>{
            state.bus.record("money_candidate",&v);
            let experiments=state.store.money_recent_experiments(
                v["campaignId"].as_str().unwrap_or(""),150).unwrap_or_default();
            let capital=state.store.get("money_campaign",v["campaignId"].as_str().unwrap_or(""))
                .and_then(|c|c["capitalWon"].as_f64()).unwrap_or(-1.0);
            Json(decorate(v,&experiments,capital)).into_response()
        },
        None=>error(StatusCode::INTERNAL_SERVER_ERROR,"사업 후보 변경을 저장하지 못했습니다.")
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
    let cost_field=|name:&str|->Result<f64,String>{
        match body.get(name){
            Some(value)=>valid_won(value,1e14).ok_or_else(||format!("{name} 값이 올바르지 않습니다.")),
            None=>Ok(0.0)
        }
    };
    let marketing=match cost_field("marketingCostWon"){Ok(v)=>v,Err(err)=>return fail(&err)};
    let model=match cost_field("modelCostWon"){Ok(v)=>v,Err(err)=>return fail(&err)};
    let labor=match cost_field("laborCostWon"){Ok(v)=>v,Err(err)=>return fail(&err)};
    let experiment=json!({"candidateId":id,"campaignId":candidate["campaignId"],"name":name,"status":status,
        "hypothesis":capped(&body["hypothesis"],1500),
        "method":capped(&body["method"],2000),
        "observations":capped(&body["observations"],3000),
        "reportedRevenueWon":revenue,"reportedCostWon":cost,
        "marketingCostWon":marketing,"modelCostWon":model,"laborCostWon":labor,
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
    #[test] fn hardware_payback_uses_net_after_all_reported_costs(){
        let campaign=json!({"id":"c1","hardwareInvestmentWon":1_000_000});
        let ledger=json!({"revenue":750000,"direct":100000,"marketing":50000,
            "model":25000,"labor":25000,"finished":1,"experiments":1});
        let result=recovery_summary(&campaign,&ledger);
        assert_eq!(result["reportedNetWon"],550000.0);
        assert_eq!(result["unrecoveredWon"],450000.0);
        assert_eq!(result["recoveryPct"],55.0);
    }
    #[test] fn dashboard_counts_large_history_in_sql_without_loading_all_rows(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("money_signal",&json!({"id":"source-1"})).unwrap();
        store.put("money_tension",&json!({"id":"t-1"})).unwrap();
        store.put("money_asymmetry",&json!({"id":"a-1"})).unwrap();
        store.put("money_candidate",&json!({"id":"c-1","origin":"model"})).unwrap();
        store.put("money_candidate",&json!({"id":"c-2","origin":"source_pair_draft"})).unwrap();
        store.put("money_ingest_report",&json!({"id":"r-1","capturedAt":"2026-11-01T00:00:00Z"})).unwrap();
        store.put("money_ingest_report",&json!({"id":"r-2","capturedAt":"2026-11-17T00:00:00Z"})).unwrap();
        store.put("job",&json!({"id":"j-1","kind":"money_discover","status":"completed",
            "result":{"modelUsed":{"model":"free-a"}}})).unwrap();
        store.put("job",&json!({"id":"j-2","kind":"money_discover","status":"failed",
            "input":{"model":"free-a"}})).unwrap();
        store.put("job",&json!({"id":"j-3","kind":"prompt","status":"completed"})).unwrap();
        store.put("money_autopilot",&json!({"id":"auto","enabled":true,
            "completedRounds":4,"freeModels":[{"model":"a"},{"model":"b"}]})).unwrap();
        let stats=store.money_dashboard_stats().unwrap();
        assert_eq!(stats["signals"],1);
        assert_eq!(stats["tensions"],1);
        assert_eq!(stats["asymmetries"],1);
        assert_eq!(stats["candidates"],2);
        assert_eq!(stats["modelCandidates"],1);
        assert_eq!(stats["templateDrafts"],1);
        assert_eq!(stats["lastIngest"]["id"],"r-2");
        assert_eq!(stats["autopilot"]["rotation"],2);
        assert_eq!(stats["autopilot"]["rounds"],4);
        assert_eq!(stats["models"].as_array().unwrap().len(),1);
        assert_eq!(stats["models"][0]["completed"],1);
        assert_eq!(stats["models"][0]["failed"],1);
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
