//! Money laboratory: domain reasoning, grounded candidate ingestion and economics.
//! Uses the existing native Rust job runtime. This is not a second agent harness.
// Single removable Money capability; shared Rust engine executes all effects.
pub mod asymmetry;
pub mod brief;
pub mod buyer;
pub mod catalog;
pub mod folding;
pub mod health;
pub mod intelligence;
pub mod kit;
pub mod mechanism;
pub mod rank;
pub mod retrieval;
pub mod sources;
pub mod strategy;

use crate::engine::jobs::JobContext;
use crate::state::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const MAX_CANDIDATES: usize = 12;

fn bounded(value: &Value, max: usize) -> String {
    value.as_str().unwrap_or("").chars().take(max).collect()
}
fn number(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0 && *n <= 1e14)
}
fn rounded(n: f64) -> Value {
    if n.is_finite() && n.abs() < 9e15 { json!(n.round() as i64) } else { Value::Null }
}

/// Unit economics are deterministic: model guesses remain labeled as assumptions.
/// Missing costs are NOT interpreted as zero.
pub fn economics(value: &Value) -> Value {
    let e = value.get("economics").unwrap_or(value);
    let price = number(e, "priceWon");
    let variable = number(e, "variableCostWon");
    let acquisition = number(e, "acquisitionCostWon");
    let fixed = number(e, "monthlyFixedWon");
    let units = number(e, "monthlyUnits");
    let setup = number(e, "setupCostWon");
    let refunds = number(e, "refundRatePct").filter(|n| *n <= 100.0);
    let contribution = match (price, variable, acquisition, refunds) {
        (Some(p),Some(v),Some(a),Some(r)) => Some(p*(1.0-r/100.0)-v-a),
        _ => None,
    };
    let profit = match (contribution, fixed, units) {
        (Some(c),Some(f),Some(u)) => Some(c*u-f),
        _ => None,
    };
    let break_even = match (contribution, fixed) {
        (Some(c),Some(f)) if c > 0.0 => Some((f/c).ceil()),
        _ => None,
    };
    let payback = match (setup, profit) {
        (Some(s),Some(p)) if p > 0.0 => Some(s/p),
        _ => None,
    };
    json!({
        "contributionWon":contribution.map(rounded).unwrap_or(Value::Null),
        "monthlyProfitWon":profit.map(rounded).unwrap_or(Value::Null),
        "breakEvenUnits":break_even.map(rounded).unwrap_or(Value::Null),
        "paybackMonths":payback.map(|n|json!((n*10.0).round()/10.0)).unwrap_or(Value::Null),
        "complete":contribution.is_some() && fixed.is_some() && units.is_some(),
        "negativeContribution":contribution.is_some_and(|n|n<=0.0)
    })
}
fn compact_context(state:&AppState,campaign_id:&str) -> String {
    let items=state.store.money_recent_candidates(campaign_id,20).unwrap_or_default();
    items.into_iter().take(15).map(|v| {
        format!("- {} | 고객={} | 차별성={} | 상태={} | 폐기근거={}",
            bounded(&v["title"],90),bounded(&v["customer"],90),
            bounded(&v["mechanism"],150),bounded(&v["status"],30),bounded(&v["killSignal"],120))
    }).collect::<Vec<_>>().join("\n")
}

fn shingles(text:&str)->std::collections::HashSet<String>{
    let chars=text.to_lowercase().chars().filter(|c|c.is_alphanumeric()).take(900).collect::<Vec<_>>();
    chars.windows(3).map(|w|w.iter().copied().collect::<String>()).collect()
}
fn overlap(left:&str,right:&str)->f64{
    let a=shingles(left);let b=shingles(right);
    if a.is_empty()||b.is_empty(){return 0.0;}
    let common=a.intersection(&b).count() as f64;
    common/(a.len()+b.len()-common as usize) as f64
}
fn candidate_novelty(candidate:&Value,known:&[Value])->f64{
    let title=bounded(&candidate["title"],180);
    let customer=bounded(&candidate["customer"],300);
    let mechanism=bounded(&candidate["mechanism"],1200);
    let mut max_overlap=0.0f64;
    for prior in known {
        let name_overlap=overlap(&title,&bounded(&prior["title"],180));
        let buyer_overlap=overlap(&customer,&bounded(&prior["customer"],300));
        let mechanism_overlap=overlap(&mechanism,&bounded(&prior["mechanism"],1200));
        let score=0.20*name_overlap+0.25*buyer_overlap+0.55*mechanism_overlap;
        max_overlap=max_overlap.max(score);
    }
    (1.0-max_overlap).clamp(0.0,1.0)
}
fn experiment_context(state:&AppState,campaign_id:&str)->String{
    state.store.money_recent_experiments(campaign_id,10).unwrap_or_default().into_iter()
        .map(|e|format!("- 실험={} 상태={} 매출보고={}원 지출보고={}원 관찰={} 결과={}",
            bounded(&e["name"],70),bounded(&e["status"],20),e["reportedRevenueWon"],e["reportedCostWon"],
            bounded(&e["observations"],180),bounded(&e["result"],150)))
        .collect::<Vec<_>>().join("\n")
}

/// Retrieve from several complementary SQL queries, then fuse ranks and
/// select distinct sources without sending the full archive to the model.
fn knowledge_context(state:&AppState,campaign:&Value) -> String {
    let queries=[
        (bounded(&campaign["brief"],450),3.0),
        (format!("{} {}",bounded(&campaign["markets"],150),
            bounded(&campaign["skills"],250)),2.0),
        (format!("{} {}",bounded(&campaign["distribution"],180),
            bounded(&campaign["preferences"],150)),1.0),
    ];
    let mut rankings=Vec::new();
    for (query,weight) in queries{
        if query.trim().chars().count()<3{continue;}
        if let Ok(found)=state.store.money_fact_search(query.trim(),30){
            rankings.push((weight,found));
        }
    }
    let recent=state.store.money_fact_recent(120).unwrap_or_default();
    let facts=crate::engine::money_retrieval::select(&rankings,&recent,22);
    facts.into_iter().map(|f|format!(
        "- [{}] {} | {} | {}",
        bounded(&f["verification"],26),bounded(&f["claim"],240),
        bounded(&f["url"],220),bounded(&f["originCampaignId"],50)
    )).collect::<Vec<_>>().join("\n")
}

fn fact_id(url:&str,claim:&str)->String {
    let normalized=format!("{}|{}",url.trim().to_lowercase(),claim.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase());
    let digest=Sha256::digest(normalized.as_bytes());
    format!("fact:{:x}",digest)
}

/// Save hard-evidence candidates separately, never as verified knowledge.
/// Stage evidence; the caller commits it together with all candidates. This
/// eliminates partially-saved results when a tool, model, or worker fails.
fn collect_evidence(source:&Value,campaign_id:&str,job_id:&str)->Vec<Value>{
    let mut evidence=Vec::new();
    for row in source["evidence"].as_array().into_iter().flatten().take(20){
        let claim=bounded(&row["claim"],700);
        let url=bounded(&row["url"],1800);
        if claim.chars().count()<12 || !url.starts_with("https://"){continue;}
        evidence.push(json!({"id":fact_id(&url,&claim),"claim":claim,"url":url,
            "kind":bounded(&row["kind"],45),"verification":"model_claim_unverified",
            "originCampaignId":campaign_id,"originJobId":job_id,
            "createdAt":chrono::Utc::now().to_rfc3339()}));
    }
    evidence
}

/// Cross-sector observations and two-hop graph neighborhoods are intentionally
/// bounded. This is information retrieval, NOT a claim that co-mention is causal.
fn market_context(state:&AppState)->String {
    let signals=state.store.money_recent_signals(24).unwrap_or_default();
    let trends=state.store.money_topic_trends().unwrap_or_default();
    let cues=crate::engine::money_intelligence::cross_domain_cues(&signals,8);
    let seeds=trends.iter().take(3).filter_map(|v|v["topic"].as_str())
        .map(crate::engine::money_intelligence::sector_node_id)
        .collect::<Vec<_>>();
    let edges=state.store.money_graph_neighbors(&seeds,12).unwrap_or_default();
    let recent=signals.iter().take(8).map(|v|format!(
        "- [{}] {} | {} | 기사본문={} | 관측일={}",
        bounded(&v["sourceName"],40),bounded(&v["title"],150),
        bounded(&v["url"],180),
        if v["contentStatus"]=="page_fetched"{"일부 수집"}else{"제목만"},
        bounded(&v["capturedAt"],35)
    )).collect::<Vec<_>>().join("\n");
    let changes=trends.iter().take(8).map(|v|format!(
        "- {}: 최근7일 {}건, 이전7일 {}건, 관측 출처 {}개 (언급량 변화이지 구매 수요 아님)",
        bounded(&v["topic"],30),v["current7d"],v["previous7d"],v["sourceDiversity"]
    )).collect::<Vec<_>>().join("\n");
    let links=cues.iter().take(6).map(|v|format!(
        "- 분야 결합 검토 {} + {} | {} (동일 기사 공언급, 인과관계 미검증)",
        bounded(&v["first"],30),bounded(&v["second"],30),bounded(&v["sourceUrl"],170)
    )).collect::<Vec<_>>().join("\n");
    let metrics=state.store.money_recent_metric_changes(15).unwrap_or_default();
    let platform_changes=metrics.iter().take(6).map(|m|format!(
        "- {}: {} {} → {} ({}%) | {} (플랫폼 활동량 지표, 실제 구매 수요 아님)",
        bounded(&m["entityId"],100),bounded(&m["metric"],25),
        m["previousValue"],m["latestValue"],m["relativeDeltaPct"],
        bounded(&m["url"],160)
    )).collect::<Vec<_>>().join("\n");
    let tensions=state.store.money_best_tensions(8).unwrap_or_default();
    let money_questions=crate::engine::money_mechanism::briefing(&tensions,6);
    let graph=edges.iter().filter(|v|v["predicate"]!="mentions_sector")
        .take(6).map(|v|format!(
        "- {} --{}--> {} [{}]",
        bounded(&v["fromLabel"],65),bounded(&v["predicate"],30),
        bounded(&v["toLabel"],65),bounded(&v["verification"],30)
    )).collect::<Vec<_>>().join("\n");
    format!("최근 수집된 뉴스(독립 수집, 모델 추론 비용 없음):\n{recent}\n시장 언급 변화:\n{changes}\n교차 분야 연구 단서:\n{links}\nDB 그래프 관계:\n{graph}\n기술 플랫폼 관측 수치 변화:\n{platform_changes}\n공급 변화 × 구매 문제에서 알고리즘이 만든 검증 가설(아직 거래 근거 없음):\n{money_questions}")
}

fn validate_asymmetry_claims(state:&AppState,opportunity:&Value)->Result<Vec<String>,String>{
    let operators=opportunity["operators"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).collect::<Vec<_>>().join(" ").to_lowercase();
    let asserted=opportunity["asymmetryClaim"]==true
        ||operators.contains("asymmetry")||operators.contains("negative-space");
    let refs=opportunity["asymmetryPairs"].as_array().cloned().unwrap_or_default();
    if asserted && refs.is_empty(){
        return Err("asymmetry assertion has no independently paired source evidence".into());
    }
    let mut result=Vec::new();
    for pair in refs.iter().take(8){
        let id=pair["id"].as_str().ok_or("asymmetry pair ID missing")?;
        let Some(record)=state.store.get("money_asymmetry",id)else{
            return Err("asymmetry pair is not indexed".into());
        };
        let Some(left)=record["sourceA"].as_str()
            .and_then(|id|state.store.get("money_event",id))else{
            return Err("asymmetry left source is missing".into());
        };
        let Some(right)=record["sourceB"].as_str()
            .and_then(|id|state.store.get("money_event",id))else{
            return Err("asymmetry right source is missing".into());
        };
        crate::engine::money_asymmetry::validate_pair(pair,&record,&left,&right)?;
        result.push(id.to_string());
    }
    Ok(result)
}

/// Abductive market reasoning: DB observations are evidence; new businesses
/// must be explicitly conjectural. This route does not request web research.
pub fn inference_prompt(state:&AppState,campaign:&Value)->String{
    let pairs=state.store.money_best_tensions(12).unwrap_or_default();
    let pair_context=pairs.iter().take(8).map(|p|format!(
        "- 관계={} | 공급관측={} ({}) | 구매문제={} ({}) | 검증할 질문={} | 출처 신뢰=원문 미검증",
        bounded(&p["mechanism"],60),bounded(&p["supplyHeadline"],130),
        bounded(&p["supplyUrl"],180),bounded(&p["buyerHeadline"],130),
        bounded(&p["buyerUrl"],180),bounded(&p["question"],180)
    )).collect::<Vec<_>>().join("\n");
    let signals=state.store.money_recent_signals(45).unwrap_or_default();
    let signal_context=signals.iter().filter(|s|!s["changeTags"].as_array()
        .is_none_or(|tags|tags.is_empty())).take(15).map(|s|format!(
        "- 관측={} | 변화유형={} | 산업={} | URL={} | 본문={}",
        bounded(&s["title"],130),s["changeTags"],s["topics"],
        bounded(&s["url"],180),bounded(&s["description"],180)
    )).collect::<Vec<_>>().join("\n");
    let query=bounded(&campaign["brief"],300);
    let memory=knowledge_context(state,campaign);
    let known=state.store.money_recent_candidates(
        campaign["id"].as_str().unwrap_or(""),35).unwrap_or_default();
    let previous=known.iter().take(10).map(|c|format!(
        "- {} | 구매자={} | 수익구조={}",
        bounded(&c["title"],95),bounded(&c["customer"],95),
        bounded(&c["mechanism"],140))).collect::<Vec<_>>().join("\n");
    // A minimal example establishes the output protocol for small local models.
    // Compact output: the model spends tokens on the surviving hypotheses,
    // not on restating every field for candidates it already rejected.
    let schema=json!({"overview":"자료에서 도출한 핵심 한 줄",
        "rejected":[{"title":"버린 후보","reason":"근거·구매자·인과 중 무엇이 없어서 버렸는가"}],
        "opportunities":[{
            "title":"새 사업 제안 이름",
            "customer":"결제권자 (확실하지 않으면 가정이라고 표기)",
            "mechanism":"관측 A -> 관측 B -> 새 거래로 이어지는 인과 구조",
            "moneyMechanism":"누가 무엇을 왜 지불하는가; 가격 없으면 금액 미상",
            "offer":"납품 가능한 제품·서비스",
            "firstTest":"우선 검증할 미확인 인과 고리",
            "killSignal":"폐기할 조건",
            "evidence":[{"claim":"직접 관측된 내용","url":"https://기록된-출처-주소"}]
        }]});
    format!(r#"DOMINION MONEY: DB 기반 귀추(가설 발견) 경제 추론.
자료를 다시 검색하거나 기사 요약하지 마라. 아래 기록은 이미 수집한 관측 자료다.
관측된 사실에서 출발해 **자료에 직접 적혀 있지 않은** 돈벌이를 발명하라.
예: 어떤 분야의 공급 원가 하락 + 전혀 다른 분야의 반복 비용 증가
→ 기술 이전/수요 중개/위험 제거가 가능한가 → 누가 무엇을 지불할까?
단순히 기사 2개의 제목을 잇거나 고정된 '자동화 서비스'를 복제하는 것은 결과가 아니다.
가장 다른 원리의 3개 가설만 선택한다. 가능하면 다른 구매자·채널·이익 메커니즘을 사용한다.
각 가설에서:
1. 관측(출처 원문으로 추적 가능)과 추측(검증 전)를 분리한다.
2. A→B→C로 3단계 이상 인과 경로를 제시한다. 인과 고리별로 관측/가정을 구분한다.
3. 반사실 조건: 기술이 대체되지 않거나, 고객이 돈을 내지 않거나, 경쟁사가 이미 해결했을 때 왜 실패하는가?
4. 구매자·첫 유료 계약·실제 만들어 팔 결과물을 구체적으로 제안한다.
5. 과거 발굴과 비슷한 공급/고객/수익모델만 반복하면 제외한다.
6. 사용자가 명시하지 않은 자본·기간·업종 제약을 만들지 않는다.
7. 가격·이익·구매 계약·빈 시장을 증거 없이 사실로 쓰지 않는다. 수치는 없으면 null.
8. 제공된 URL만 근거로 사용한다. 원문에서 본 적 없는 직접인용은 만들지 않는다.
9. 실제 사업 실행은 하지 않는다. 검증 가능한 아이디어 초안만 작성한다.
10. 살아남은 가설만 최대 3개 쓴다. 버린 후보는 rejected에 제목과 이유 한 줄만 적고 상세 필드를 쓰지 않는다.
11. 스키마에 없는 필드는 만들지 않는다. 비어 있는 값은 빈 문자열 또는 null로 둔다.
JSON 객체 하나만 출력하라. JSON 문법 오류가 없도록 한다.
목표: {query}

관측된 경제적 갈등 및 결합 단서:
{pair_context}

최근 서로 다른 산업에서의 변화:
{signal_context}

기존 DB에서 관련성 검색한 관측/기억:
{memory}

기존 후보 (중복 제외):
{previous}

출력 스키마 예시(내용 복붙 금지):
{schema}
"#)
}

/// Domain-specific search operators produce structural novelty, not trending-product lists.
pub fn prompt(state:&AppState,campaign:&Value,mode:&str,focus:Option<&Value>) -> String {
    if mode=="infer"{return inference_prompt(state,campaign);}
    let previous = compact_context(state,campaign["id"].as_str().unwrap_or(""));
    let market_knowledge=knowledge_context(state,campaign);
    let experiment_knowledge=experiment_context(state,campaign["id"].as_str().unwrap_or(""));
    let market_intelligence=market_context(state);
    let pending_asymmetries=state.store.money_pending_asymmetries(12).unwrap_or_default();
    let pending_asymmetry_brief=crate::engine::money_asymmetry::briefing(&pending_asymmetries,8);
    let past_candidates=state.store.money_recent_candidates(campaign["id"].as_str().unwrap_or(""),200).unwrap_or_default();
    let past_experiments=state.store.money_recent_experiments(campaign["id"].as_str().unwrap_or(""),200).unwrap_or_default();
    let learning=crate::engine::money_rank::strategy_stats(&past_candidates,&past_experiments);
    let feedback=learning.iter().map(|v|format!("{}: 실험 {}건, 순현금흐름 {}원 (보고값)",
        bounded(&v["mode"],20),v["finishedExperiments"],v["reportedNetWon"]))
        .collect::<Vec<_>>().join("; ");
    let focus_text = focus.map(|v|format!(
        "\n선택한 기존 가설: {}\n고객: {}\n구조: {}\n왜 아직 검증 전인지: {}\n",
        bounded(&v["title"],120),bounded(&v["customer"],200),
        bounded(&v["mechanism"],500),bounded(&v["killSignal"],220)
    )).unwrap_or_default();
    let condition=|key:&str,unit:&str|campaign[key].as_f64()
        .map(|v|format!("{v}{unit}"))
        .unwrap_or_else(||"사용자가 지정하지 않음 · 탐색 제한 금지".to_string());
    let investment=campaign["hardwareInvestmentWon"].as_f64()
        .map(|n|format!("{n}원")).unwrap_or_else(||"미지정".to_string());
    let owner = format!(
        "가용 자본={}, 목표 월 순이익={}, 주당 투입={}, 보유기술={}, 영업/고객접점={}, 피할 분야={}, 선호={}, 지역={}",
        condition("capitalWon","원"),condition("targetMonthlyProfitWon","원"),
        condition("hoursPerWeek","시간"),bounded(&campaign["skills"],600),
        bounded(&campaign["distribution"],600),bounded(&campaign["exclusions"],500),
        bounded(&campaign["preferences"],500),bounded(&campaign["markets"],200)
    );
    let instructions=crate::engine::money_strategy::discovery_brief(mode);
    let entrepreneur_panel=crate::engine::money_strategy::panel_directive(mode);
    format!(r#"당신은 DOMINION MONEY의 경제적 발명·검증 에이전트입니다.
목표: 사업 아이디어를 많이 말하는 것이 아니라 현실에서 매출과 순현금흐름을 낳을 수 있는 독창적 가설을 찾아 실험 가능하게 만드는 것입니다.
사용자 조건: {owner}
사용자가 지정하지 않은 자본·수익 목표·노동시간·산업·지역·판매방식 조건을 만들어 후보를 배제하거나 순위에 반영하지 마라. 빈 조건은 의도적으로 열린 탐색이다.
실제 기기 투자 회수 목표={investment}. 처음에는 고객 결제 증거가 없는 신규 가설을 수익 실적으로 착각하지 마라.
{entrepreneur_panel}
사용자 질문: {brief}
서로 다른 시장·언어의 기사, 공급자 공시, 공식 가격, 오픈소스 릴리스를 탐색하라. 최신 정보만 보지 말고 변화 전후의 차이가 누가 돈을 버는지를 연결하라.
탐색 모드: {mode}
지시: {instructions}
{focus_text}
이미 발견한 아이디어(중복 회피, 실패조건은 교훈으로 재사용):
{previous}
과거 다른 조사에서도 축적된 시장 근거(검색 인덱스에서 발췌한 제한된 기록. 미검증 자료는 검증 전까지 사실로 단정하지 말 것):
{market_knowledge}
과거 실험 결과(사용자 보고 값, 결제 검증과 구별할 것):
{experiment_knowledge}
탐색 전략 성과 통계(실제 실험 데이터만, 수익 예측률과 다름): {feedback}
{market_intelligence}
미해결 정보비대칭 페어(원본이 아닌 2개 사건의 ID·지역·URL만, 최대 8건):
{pending_asymmetry_brief}

실행 절차:
1. 사용 가능한 web_search/http_fetch 등의 읽기 도구로 시장 현실을 조사하라. 모델이 아는 이야기를 사실처럼 쓰지 마라. 접근이 막혔다면 정보 부재를 표시하라.
2. 최소 5종의 상이한 경제적 발명 연산자를 사용하라: cross-domain transfer, cost inversion, unbundling, aggregation, byproduct monetization, channel arbitrage, regulatory adaptation, labor substitution, negative-space search, distribution reuse 등. 기존 유행 상품 베끼기는 목표가 아니다.
3. 각 아이디어의 '왜 고객이 기존보다 더 비싸게 지불할까/왜 나에게 돈을 줄까'를 인과적으로 설명하라. 경쟁자 복제·대체재·재고·플랫폼 수수료·광고·AS·법규·실패 확률과 자본회전을 검토하라.
4. 개별 가설을 최소 2개의 다른 사업모델(직판/라이선스/중개/OEM/B2B 등)로 변형하고, 가장 자본효율적인 모델을 제시하라.
4-1. 가장 중요한 목표는 실제 돈을 받는 것이다. 고객은 추상적 산업이 아니라 결제 권한이 있는 조직/구매자 역할이어야 한다. 현재 비용 지출, 공개 발주/구매 요청/판매가격, 구매자 접근 경로를 조사하고 FIRST PAID CUSTOMER까지 가장 짧은 경로를 찾는다.
4-2. 경쟁제품의 실제 판매가격 또는 조달 단가를 가능한 한 1차 출처에서 찾아 비교한다. 정보가 없으면 예상가격을 null로 두고 수익을 추정하지 않는다. 고객이 존재한다는 추측을 판매 확약으로 기재하지 않는다.
4-3. 정보비대칭을 주장하는 후보는 위 페어의 id, axis, sourceA, sourceB를 정확히 따라 적어라. 서로 다른 2개 시장의 자료가 존재한다는 사실만으로 '한국에 없다'가 입증되는 것은 아니다. 임의의 페어·출처를 만들면 후보가 거절된다.
5. 구매 근거는 링크만으로 점수를 받을 수 없다. 기존 DB에 보관된 원문과 대조할 수 있도록 buyerEvidence.quote에 구매·발주·예산 관련 원문의 정확한 연속 구절을 넣어라. 원문을 못 찾으면 빈 문자열을 쓰고 근거를 만들지 마라.
5-1. 웹으로 확인한 내용은 근거 URL, 확인 못한 것은 assumption으로 엄격히 구분한다. 기사에서 언급된 수요와 실제 구매 의향은 별개다. 실제 구매자 의사표시·견적 요청·예산이 있는 자료가 있으면 buyerEvidence에 근거와 함께 넣는다. URL을 지어내지 마라. 경제 수치가 비어 있으면 null로 두고 임의의 숫자로 채우지 마라.
6. 시험 부정행위, 소비자 기만, 개인정보 침해 등을 사업 장점으로 설계하지 마라.
7. 각각의 구현 방법은 implementationPlan에 MVP 구성→원천 데이터→자동화→결과 검증 순서로 구체화한다. 실제 사업 구현·계약·외부 연락은 사용자에게 보고하고 허락받은 뒤 진행한다.
7-1. 실제 구입·광고비 지출·계약·대외 연락은 하지 않는다. 다음 수요검증 방법과 폐기 조건을 정한다.
8. 수집한 뉴스/페이지의 변화 사실을 sourceSignals로 정리하라. 뉴스 없는 순수 발명 후보도 허용한다. 원문 확인 전 데이터는 '미검증 추정'으로 기록하라.
8-1. 기술·고객·공급망의 연결을 links에 주체·대상·관계로 기록하라. 기존 지식 그래프의 모델 주장과 기사 공언급을 검증된 인과관계로 취급하지 마라.
9. 최종 출력은 설명글 없이 아래 스키마에 해당하는 유효한 JSON 객체 하나만 작성한다. 모든 아이디어를 서로 다른 고객/돈 버는 메커니즘으로 구분하라.

{{"overview":"시장 탐색 결론","sourceSignals":[{{"change":"원문에서 관측한 변화", "url":"https://...", "publishedAt":"YYYY-MM-DD 또는 null", "category":"technology|demand|supply|price|regulation|distribution","businessConsequence":"어떤 비용·수요·가격이 변화했나"}}],"opportunities":[
{{"title":"상품/사업 이름","customer":"정확한 지불 주체","problem":"고비용 문제",
"offer":"판매할 결과물","implementationPlan":"MVP 구현 순서와 필요한 도구·데이터·검증단계","mechanism":"기존과 다른 이익 창출 구조","operators":["cross-domain transfer"],
"links":[{{"from":"기술·자원","to":"고객 문제","relation":"addresses"}}],
"asymmetryClaim":false,"asymmetryPairs":[{{"id":"DB의 asymmetry:...","axis":"regional|temporal|channel|interpretive","sourceA":"DB의 event ID","sourceB":"DB의 event ID"}}],
"firstBuyer":"실제 결제 결정권자 또는 조직","firstPaymentStep":"첫 유료 거래를 만드는 제안·검증 절차","buyerEvidence":[{{"type":"public_purchase_request|official_budget|customer_complaint|assumption","claim":"관측 또는 가정","url":"https://... 또는 null","quote":"원문에서 그대로 인용한 구매 요청 또는 예산 표현, 찾지 못했다면 빈 문자열"}}],"alternativeSuppliers":["경쟁/대체 공급"] ,
"whyPay":"고객이 돈을 낼 이유","moneyMechanism":"누구의 비용을 줄이고 얼마를 받을 수 있는가", "distribution":"첫 고객 확보 경로","defensibility":"복제 어려운 요인",
"alternatives":["다른 수익구조 1","다른 수익구조 2"],"whyNow":"촉발 요인","firstTest":"최소비용 실험",
"killSignal":"폐기할 구체적인 조건","assumptions":["확인 필요 사항"],"risks":["핵심 위험"],
"evidence":[{{"claim":"관측 사실","url":"https://...","kind":"primary or secondary"}}],
"economics":{{"priceWon":null,"variableCostWon":null,"acquisitionCostWon":null,
"monthlyFixedWon":null,"monthlyUnits":null,"setupCostWon":null,"refundRatePct":null}}
}}]}}
"#,
        owner=owner,investment=investment,entrepreneur_panel=entrepreneur_panel,brief=bounded(&campaign["brief"],1200),mode=mode,
        instructions=instructions,focus_text=focus_text,previous=previous,market_knowledge=market_knowledge,experiment_knowledge=experiment_knowledge,
        market_intelligence=market_intelligence,feedback=feedback,pending_asymmetry_brief=pending_asymmetry_brief
    )
}

fn parse_output(raw:&str) -> Result<Value,String> {
    let trimmed=raw.trim();
    // Local/compat models often wrap otherwise valid JSON in markdown fences.
    let s=if trimmed.starts_with("```"){
        trimmed.split_once('\n').map(|(_,body)|body.trim().strip_suffix("```")
            .unwrap_or(body.trim()).trim()).unwrap_or(trimmed)
    }else{trimmed};
    let parsed=match serde_json::from_str::<Value>(s){
        Ok(value)=>value,
        Err(_)=>{
            let start=s.find('{').ok_or_else(||"JSON 시작을 찾지 못했습니다.".to_string())?;
            let end=s.rfind('}').ok_or_else(||"JSON 끝을 찾지 못했습니다.".to_string())?;
            serde_json::from_str::<Value>(&s[start..=end])
                .map_err(|e|format!("구조화된 사업 결과를 읽지 못했습니다: {e}"))?
        }
    };
    // Recover common small-model shapes without another paid repair call.
    let normalized=if parsed.is_array(){
        json!({"overview":"","opportunities":parsed})
    }else if parsed["opportunities"].is_array(){
        parsed
    }else if parsed["candidates"].is_array(){
        json!({"overview":parsed["overview"],"opportunities":parsed["candidates"]})
    }else if parsed["opportunities"].is_object(){
        json!({"overview":parsed["overview"],"opportunities":[parsed["opportunities"]]})
    }else if parsed["title"].is_string() && parsed["customer"].is_string()
        && parsed["mechanism"].is_string(){
        json!({"overview":"","opportunities":[parsed]})
    }else{
        return Err("사업 후보 배열(opportunities/candidates)이 없습니다.".into());
    };
    // An empty, valid scan is a research result, not a parser error.
    Ok(normalized)
}

pub async fn run(context:&JobContext,job:&Value)->Result<Value,String>{
    let campaign_id=job["input"]["campaignId"].as_str().ok_or("사업 탐색 캠페인 ID가 없습니다.")?;
    let campaign=context.state.store.get("money_campaign",campaign_id).ok_or("사업 탐색 캠페인을 찾지 못했습니다.")?;
    let response=super::jobs::run_prompt(context,job).await?;
    if response["status"]=="needs_review"{return Ok(response);}
    let session=response["sessionId"].as_str().ok_or("완료된 작업 세션이 없습니다.")?;
    let msg_id=response["messageId"].as_str().ok_or("모델 결과 메시지가 없습니다.")?;
    let messages=context.state.store.try_messages(session)?;
    let msg=messages.iter().find(|m|m["id"]==msg_id).ok_or("모델 응답 메시지를 찾지 못했습니다.")?;
    let raw=msg["content"].as_str().unwrap_or("");
    // The native execution session contains the actual selected provider/model,
    // including automatic routing, not merely the requested defaults.
    let session_record=context.state.store.get("session",session).unwrap_or(Value::Null);
    let model_used=session_record["model"].clone();
    let provider_used=session_record["providerId"].clone();
    let parsed=match parse_output(raw){
        Ok(v)=>v,
        Err(error)=>{
            let report=json!({"id":format!("report:{}",context.job_id),"campaignId":campaign_id,
                "jobId":context.job_id,"sessionId":session,"raw":raw.chars().take(60000).collect::<String>(),
                "parseError":error,"modelUsed":model_used,"providerUsed":provider_used,
                "createdAt":chrono::Utc::now().to_rfc3339()});
            context.state.store.put("money_report",&report).ok_or("원시 보고서를 저장하지 못했습니다.")?;
            return Ok(json!({"status":"needs_review","reason":error,"sessionId":session,"reportId":report["id"]}));
        }
    };
    // Fetch a small, distinct set of referenced public buyer documents so
    // evidence outside the RSS archive can be checked without model tokens.
    let buyer_source_fetch=crate::engine::money_buyer::capture_referenced_sources(
        &context.state,&parsed,4).await;
    let mut saved_ids=Vec::new();
    let mut pending_candidates=Vec::new();
    let mut pending_facts=Vec::new();
    let mut pending_edges=Vec::new();
    let signal_items=parsed["sourceSignals"].as_array().cloned().unwrap_or_default();
    let mut skipped_duplicates=0usize;
    let mut rejected_asymmetry=0usize;
    let mut evaluated_asymmetry_ids=std::collections::BTreeSet::new();
    // Exact IDs in this job's immutable input are the only hypotheses the
    // model was asked to review. An interrupted or malformed result never
    // reaches this commit path, so it cannot exhaust the pending queue.
    for id in job["input"]["asymmetryBatchIds"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).take(8){
        if context.state.store.get("money_asymmetry",id)
            .is_some_and(|record|record["status"]=="pending"){
            evaluated_asymmetry_ids.insert(id.to_string());
        }
    }
    let mut known=context.state.store.money_recent_candidates(campaign_id,200).unwrap_or_default();
    let mut buyer_source_cache=std::collections::HashMap::<String,Option<Value>>::new();
    for signal in signal_items.iter().take(30) {
        let change=bounded(&signal["change"],750);
        let source=json!({"evidence":[{"claim":change,
            "url":bounded(&signal["url"],1800),"kind":"news_or_market_signal"}]});
        pending_facts.extend(collect_evidence(&source,campaign_id,&context.job_id));
    }
    let candidates=parsed["opportunities"].as_array().expect("validated");
    for (index,item) in candidates.iter().take(MAX_CANDIDATES).enumerate(){
        if bounded(&item["title"],160).trim().is_empty()
            || bounded(&item["customer"],300).trim().is_empty()
            || bounded(&item["mechanism"],1000).trim().is_empty(){continue;}
        let ids=match validate_asymmetry_claims(&context.state,item) {
            Ok(ids)=>ids,
            Err(_)=>{rejected_asymmetry+=1;continue;}
        };
        let novelty=candidate_novelty(item,&known);
        // High overlap is a heuristic, not proof of equivalence. Keep truly
        // differentiated mechanisms and buyers even when titles resemble.
        if novelty<0.11 && known.iter().any(|prior|
            overlap(&bounded(&item["customer"],300),&bounded(&prior["customer"],300))>0.75
            && overlap(&bounded(&item["mechanism"],1200),&bounded(&prior["mechanism"],1200))>0.84){
            skipped_duplicates+=1;
            continue;
        }
        // Model-proposed buyer quotations never upgrade themselves to proof.
        // Match against the permanent source corpus, with one indexed read
        // per distinct URL for the entire discovery job.
        let buyer_evidence=item["buyerEvidence"].as_array().into_iter().flatten().take(12)
            .map(|entry|{
                let url=bounded(&entry["url"],1800);
                let source=buyer_source_cache.entry(url.clone())
                    .or_insert_with(||context.state.store.money_signal_by_url(&url)
                        .ok().flatten());
                crate::engine::money_buyer::annotate(entry,source.as_ref())
            }).collect::<Vec<_>>();
        let candidate=json!({
            "id":format!("money:{}:{}",context.job_id,index),"campaignId":campaign_id,
            "jobId":context.job_id,"sessionId":session,"status":"hypothesis",
            "discoveryMode":job["input"]["mode"],"links":item["links"],
            "asymmetryPairIds":ids.clone(),"asymmetryClaim":item["asymmetryClaim"]==true,
            "inferenceSteps":item["inferenceSteps"],"counterfactual":bounded(&item["counterfactual"],1100),
            "asymmetryStatus":if ids.is_empty(){"not_claimed"}else{"two_source_pair_not_verified_absence"},
            "title":bounded(&item["title"],160),"customer":bounded(&item["customer"],300),
            "problem":bounded(&item["problem"],1000),"offer":bounded(&item["offer"],1000),
            "implementationPlan":bounded(&item["implementationPlan"],1200),
            "mechanism":bounded(&item["mechanism"],1500),"moneyMechanism":bounded(&item["moneyMechanism"],1100),"operators":item["operators"],
            "whyPay":bounded(&item["whyPay"],1200),
            "firstBuyer":bounded(&item["firstBuyer"],350),
            "firstPaymentStep":bounded(&item["firstPaymentStep"],1100),
            "buyerEvidence":buyer_evidence,"alternativeSuppliers":item["alternativeSuppliers"],
            "distribution":bounded(&item["distribution"],1000),
            "defensibility":bounded(&item["defensibility"],1000),
            "alternatives":item["alternatives"],"whyNow":bounded(&item["whyNow"],1000),
            "firstTest":bounded(&item["firstTest"],1200),"killSignal":bounded(&item["killSignal"],1200),
            "assumptions":item["assumptions"],"risks":item["risks"],
            "evidence":item["evidence"],"evidenceStatus":"unverified",
            "economics":item["economics"],"createdAt":chrono::Utc::now().to_rfc3339(),
            "origin":"model","expectedProfitIsVerified":false,
            "noveltyEstimate":((novelty*1000.0).round()/1000.0),"noveltyMethod":"character_trigram_overlap"
        });
        for id in &ids{evaluated_asymmetry_ids.insert(id.clone());}
        pending_facts.extend(collect_evidence(item,campaign_id,&context.job_id));
        pending_edges.extend(crate::engine::money_intelligence::candidate_edges(&candidate));
        known.push(candidate.clone());
        saved_ids.push(candidate["id"].clone());
        pending_candidates.push(candidate);
    }
    let report=json!({"id":format!("report:{}",context.job_id),"campaignId":campaign_id,
        "jobId":context.job_id,"sessionId":session,
        "modelUsed":model_used,"providerUsed":provider_used,
        "overview":bounded(&parsed["overview"],4000),"sourceSignals":signal_items,
        "opportunityBriefs":crate::engine::money_brief::summarize(&pending_candidates,8),
        "signalCount":pending_facts.len(),"graphEdgeCount":pending_edges.len(),
        "candidateIds":saved_ids,"skippedNearDuplicates":skipped_duplicates,
        "buyerSourceFetch":buyer_source_fetch,
        "matchedBuyerQuoteCount":pending_candidates.iter().flat_map(|c|
            c["buyerEvidence"].as_array().into_iter().flatten())
            .filter(|e|crate::engine::money_buyer::buying_signal(e)).count(),
        "rejectedFabricatedAsymmetryClaims":rejected_asymmetry,
        "evaluatedAsymmetryIds":evaluated_asymmetry_ids.iter().cloned().collect::<Vec<_>>(),
        "createdAt":chrono::Utc::now().to_rfc3339()});
    context.state.store.money_commit_discovery(&report,&pending_candidates,&pending_facts,&pending_edges,
        &evaluated_asymmetry_ids.into_iter().collect::<Vec<_>>())
        .map_err(|e|format!("Money 결과 원자적 저장 실패: {e}"))?;
    // Campaign is a workspace label; preserve user-edited fields while updating state.
    if let Some(current)=context.state.store.get("money_campaign",campaign_id){
        let _=context.state.store.patch_if("money_campaign",campaign_id,"id",&current["id"],
            &json!({"lastReportId":report["id"],"lastCompletedJobId":context.job_id}));
    }
    context.state.bus.record("money_report",&report);
    Ok(json!({"sessionId":session,"messageId":msg_id,"reportId":report["id"],"candidateIds":saved_ids,"campaignName":campaign["name"]}))
}

/// Cycle modes deliberately alternate breadth, distant recombination and
/// falsification so a small local model does not repeatedly sample one niche.
pub fn exploration_mode(round:u64)->&'static str{
    match round%6 {0|3=>"signals",1=>"discover",2|4=>"cross",_=>"challenge"}
}

/// Bounded unattended local-model research, invoked by the existing worker
/// timer. Only one active job per campaign; failed or uncertain runs pause.
/// No changes to the shared agent execution kernel.
fn stop_run(state:&AppState,id:&str,run_id:&str,reason:&str,detail:&str){
    let Some(prior)=state.store.get("money_autopilot",id)else{return;};
    let _=state.store.mutate_record("money_autopilot",id,prior,|row|{
        if row["runId"].as_str()!=Some(run_id)||row["enabled"]!=true{
            return Err("stale run stop ignored".into());
        }
        row["enabled"]=json!(false);
        row["stopReason"]=json!(reason);
        row["lastError"]=json!(detail);
        row["updatedAt"]=json!(chrono::Utc::now().to_rfc3339());
        Ok(vec![])
    });
}

/// Continuous 24/7 mode obeys per-day quotas and can be paused indefinitely;
/// finite mode keeps its deadline. All claims are transactional, run IDs are
/// generation-scoped, and no past job ID can be replayed after restarting.
pub fn schedule_autopilot(state:&AppState){
    let now=chrono::Utc::now();
    for mut auto in state.store.all("money_autopilot"){
        if auto["enabled"]!=true{continue;}
        let Some(id)=auto["id"].as_str().map(str::to_string)else{continue;};
        let Some(run_id)=auto["runId"].as_str().map(str::to_string)else{continue;};
        let Some(campaign)=state.store.get("money_campaign",&id)else{
            stop_run(state,&id,&run_id,"campaign_missing","campaign deleted");continue;
        };
        if auto["continuous"]!=true {
            let end=auto["endsAt"].as_str()
                .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|v|v.with_timezone(&chrono::Utc));
            if end.is_none_or(|end|now>=end){
                stop_run(state,&id,&run_id,"deadline","");continue;
            }
        }
        if let Some(last_id)=auto["lastJobId"].as_str(){
            let Some(last)=state.store.get("job",last_id)else{
                stop_run(state,&id,&run_id,"job_missing","missing previous job");continue;
            };
            match last["status"].as_str(){
                Some("queued"|"running")=>continue,
                Some("completed")=>{
                    if last["result"]["status"]=="needs_review"{
                        stop_run(state,&id,&run_id,"job_needs_review","model output review required");continue;
                    }
                    if auto["lastAccountedJobId"]!=last["id"]{
                        let result=state.store.mutate_record("money_autopilot",&id,auto.clone(),|row|{
                            if row["enabled"]!=true||row["runId"].as_str()!=Some(run_id.as_str()){
                                return Err("stale run completion".into());
                            }
                            if row["lastAccountedJobId"]!=last["id"]{
                                row["completedRounds"]=json!(row["completedRounds"].as_u64().unwrap_or(0)+1);
                                row["lastAccountedJobId"]=last["id"].clone();
                                row["consecutiveFailures"]=json!(0);
                                row["lastFailure"]=Value::Null;
                            }
                            Ok(vec![])
                        });
                        let Ok(saved)=result else{continue;};
                        auto=saved;
                    }
                },
                Some("failed"|"cancelled"|"needs_review")=>{
                    // A free model that returns broken JSON costs one round, not
                    // the whole run. Stop only after repeated consecutive misses.
                    let failures=auto["consecutiveFailures"].as_u64().unwrap_or(0)+1;
                    let limit=auto["maxConsecutiveFailures"].as_u64().unwrap_or(6);
                    if failures>=limit{
                        stop_run(state,&id,&run_id,"job_needs_review",
                            last["error"].as_str().unwrap_or("job failed"));continue;
                    }
                    let reason=last["error"].as_str().unwrap_or("job failed").to_string();
                    let result=state.store.mutate_record("money_autopilot",&id,auto.clone(),|row|{
                        if row["runId"].as_str()!=Some(run_id.as_str())||row["enabled"]!=true{
                            return Err("stale run failure".into());
                        }
                        row["consecutiveFailures"]=json!(failures);
                        row["lastFailure"]=json!(reason);
                        row["scheduledRounds"]=json!(row["scheduledRounds"].as_u64().unwrap_or(0)+1);
                        row["dailyScheduledRounds"]=json!(row["dailyScheduledRounds"].as_u64().unwrap_or(0)+1);
                        row["lastJobId"]=Value::Null;
                        row["nextAt"]=json!(now.to_rfc3339());
                        Ok(vec![])
                    });
                    let Ok(saved)=result else{continue;};
                    auto=saved;
                },
                _=>{stop_run(state,&id,&run_id,"unexpected_job_status","");continue;}
            }
        }
        let round=auto["scheduledRounds"].as_u64().unwrap_or(0);
        if auto["continuous"]==true {
            let day_key=now.timestamp().div_euclid(86400);
            if auto["dayKey"].as_i64()!=Some(day_key) {
                let result=state.store.mutate_record("money_autopilot",&id,auto.clone(),|row|{
                    if row["runId"].as_str()!=Some(run_id.as_str())||row["enabled"]!=true{
                        return Err("stale run day rollover".into());
                    }
                    if row["dayKey"].as_i64()!=Some(day_key) {
                        row["dayKey"]=json!(day_key);
                        row["dailyScheduledRounds"]=json!(0);
                    }
                    Ok(vec![])
                });
                let Ok(saved)=result else{continue;};
                auto=saved;
            }
            if auto["dailyScheduledRounds"].as_u64().unwrap_or(0)
                >=auto["maxRounds"].as_u64().unwrap_or(36){continue;}
        }else if round>=auto["maxRounds"].as_u64().unwrap_or(36){
            stop_run(state,&id,&run_id,"max_rounds","");continue;
        }
        if auto["nextAt"].as_str()
            .and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|v|v.with_timezone(&chrono::Utc)>now){continue;}
        let history=state.store.money_recent_candidates(&id,200).unwrap_or_default();
        let outcomes=state.store.money_recent_experiments(&id,200).unwrap_or_default();
        let mode=crate::engine::money_rank::next_mode(round,&history,&outcomes);
        let mut input=json!({"prompt":prompt(state,&campaign,mode,None),
            "asymmetryBatchIds":state.store.money_pending_asymmetries(8).unwrap_or_default()
                .iter().filter_map(|v|v["id"].as_str()).collect::<Vec<_>>(),
            "campaignId":id,"mode":mode,"toolsEnabled":true,
            // Money is the bounded research controller. Do not nest the generic
            // three-attempt autonomous repair loop around each Money round.
            "autonomous":false,"toolsExecutionEnabled":true,
            "tokenBudget":auto["tokenBudget"].as_u64().unwrap_or(12000),
            "moneyAutoRound":round+1,"moneyAutoRunId":run_id});
        for field in ["agentId","providerId","model"]{
            if let Some(v)=auto[field].as_str().filter(|s|!s.is_empty()){
                input[field]=json!(v);
            }
        }
        // Free-model rotation: each round takes the next pair, so a slow model
        // cools down alone instead of stopping the run.
        let rotation=auto["freeModels"].as_array().cloned().unwrap_or_default();
        if !rotation.is_empty(){
            let index=(round as usize)%rotation.len();
            if let Some(entry)=rotation.get(index){
                for key in ["providerId","model"]{
                    if let Some(v)=entry[key].as_str().filter(|s|!s.is_empty()){
                        input[key]=json!(v);
                    }
                }
                if let Some(map)=input.as_object_mut(){map.remove("agentId");}
                input["rotationIndex"]=json!(index);
            }
        }
        let job_id=format!("money:auto:{id}:{run_id}:{}",round+1);
        let queued=json!({"id":job_id,"kind":"money_discover","status":"queued",
            "queuedAt":now.to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
            "input":input,"attempts":0,"stepIndex":0,"stepTotal":0,"events":[]});
        let mut next_auto=auto.clone();
        let interval=auto["intervalMinutes"].as_i64().unwrap_or(30).clamp(10,240);
        next_auto["lastJobId"]=json!(job_id);
        next_auto["scheduledRounds"]=json!(round+1);
        next_auto["dailyScheduledRounds"]=json!(auto["dailyScheduledRounds"].as_u64().unwrap_or(0)+1);
        next_auto["lastMode"]=json!(mode);
        next_auto["lastQueuedAt"]=json!(now.to_rfc3339());
        next_auto["nextAt"]=json!((now+chrono::Duration::minutes(interval)).to_rfc3339());
        match state.store.money_claim_job(&id,&queued,Some((&run_id,round,&next_auto))){
            Ok(Some(job))=>{
                state.bus.record("job",&job);
                state.store.audit("job.queued",
                    json!({"kind":"money_discover","jobId":job_id,
                        "campaignId":id,"runId":run_id,"round":round+1}));
            },
            Ok(None)=>{}, // stale tick, manual job or disabled generation
            Err(err)=>stop_run(state,&id,&run_id,"enqueue_failed",&err),
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn small_model_json_fences_and_alternative_candidate_shapes_are_recovered(){
        let example=r#"{"title":"B2B service","customer":"operations lead","mechanism":"automation"}"#;
        let single=parse_output(example).unwrap();
        assert_eq!(single["opportunities"].as_array().unwrap().len(),1);
        let array=parse_output(&format!("```json\\n[{}]\\n```",example).replace("\\n","\n")).unwrap();
        assert_eq!(array["opportunities"].as_array().unwrap().len(),1);
        assert!(parse_output("found demand in article").is_err());
    }
    #[test] fn economics_never_invents_missing_costs(){
        let x=economics(&json!({"economics":{"priceWon":100000,"variableCostWon":25000,"monthlyUnits":20}}));
        assert!(x["monthlyProfitWon"].is_null());
        assert_eq!(x["complete"],false);
    }
    #[test] fn economics_computes_contribution_and_break_even(){
        let x=economics(&json!({"priceWon":100000,"variableCostWon":25000,"acquisitionCostWon":10000,
            "refundRatePct":0,"monthlyFixedWon":130000,"monthlyUnits":10,"setupCostWon":520000}));
        assert_eq!(x["contributionWon"],65000);
        assert_eq!(x["breakEvenUnits"],2);
        assert_eq!(x["monthlyProfitWon"],520000);
        assert_eq!(x["paybackMonths"],1.0);
    }
    #[test] fn novelty_discriminates_repeats_from_new_buyers(){
        let known=vec![json!({"title":"음성 계산기","customer":"건설 현장",
            "mechanism":"음성 인식으로 공사 현장 설비 수량을 계산한다"})];
        let repeat=json!({"title":"음성 계산기","customer":"건설 현장",
            "mechanism":"음성 인식으로 공사 현장 설비 수량을 계산한다"});
        let different=json!({"title":"냉장 물류 라이선스","customer":"식자재 프랜차이즈",
            "mechanism":"냉장 차량 센서 분석을 월 구독 방식으로 납품한다"});
        assert!(candidate_novelty(&repeat,&known)<0.05);
        assert!(candidate_novelty(&different,&known)>0.5);
    }
    #[test] fn discovery_commit_is_atomic_and_never_overwrites_manual_decisions(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let report=json!({"id":"report-1","campaignId":"c1","candidateIds":["c-1"]});
        let first=json!({"id":"c-1","title":"시장간 거래","status":"hypothesis"});
        let evidence=json!({"id":"fact-1","claim":"신규 기술 공급 가격이 크게 하락함","url":"https://example.org/source"});
        assert!(store.money_commit_discovery(&report,&[first.clone()],&[evidence],&[],&[]).unwrap());
        let mut changed=store.get("money_candidate","c-1").unwrap();
        changed["status"]=json!("validated");
        store.put("money_candidate",&changed).unwrap();
        assert!(!store.money_commit_discovery(&report,&[first],&[],&[],&[]).unwrap());
        assert_eq!(store.get("money_candidate","c-1").unwrap()["status"],"validated");
        assert_eq!(store.money_corpus_stats().unwrap()["money_fact"],1);
    }
    #[test] fn public_market_ingestion_deduplicates_sources_and_indexes_content(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let item=json!({"id":"signal:1","title":"중국 반도체 장비 공급가격 하락",
            "url":"https://example.org/source-1","sourceName":"test","description":"한국 산업에서 수요 증가",
            "capturedAt":"2026-11-16T11:00:00Z","articleExcerpt":"저렴한 첨단 부품"});
        assert_eq!(store.money_ingest_signals(&[item.clone(),item.clone()]).unwrap(),1);
        assert_eq!(store.money_ingest_signals(&[item]).unwrap(),0);
        assert_eq!(store.money_corpus_stats().unwrap()["money_signal"],1);
        assert_eq!(store.money_corpus_stats().unwrap()["money_fact"],1);
        assert_eq!(store.money_recent_signals(10).unwrap().len(),1);
    }
    #[test] fn later_scrape_enriches_source_and_search_index_without_duplication(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let headline=json!({"id":"signal:enrich","title":"한국 부품 가격 변화",
            "url":"https://example.org/enrich","sourceName":"test",
            "description":"공급망 모니터링","capturedAt":"2026-11-16T10:00:00Z",
            "articleExcerpt":"","contentStatus":"headline_observed"});
        assert_eq!(store.money_ingest_signals(&[headline.clone()]).unwrap(),1);
        let mut fetched=headline.clone();
        fetched["capturedAt"]=json!("2026-11-16T12:00:00Z");
        fetched["articleExcerpt"]=json!("장기공급계약재편 ".repeat(30));
        fetched["contentStatus"]=json!("page_fetched");
        assert_eq!(store.money_ingest_signals(&[fetched.clone()]).unwrap(),0);
        assert_eq!(store.money_ingest_signals(&[fetched]).unwrap(),0);
        let result=store.get("money_signal","signal:enrich").unwrap();
        assert_eq!(result["contentStatus"],"page_fetched");
        assert_eq!(result["capturedAt"],"2026-11-16T10:00:00Z");
        assert_eq!(result["lastFetchedAt"],"2026-11-16T12:00:00Z");
        let hits=store.money_fact_search("장기공급계약재편",10).unwrap();
        assert!(hits.iter().any(|fact|fact["id"]=="source:signal:enrich"));
        assert_eq!(store.money_corpus_stats().unwrap()["money_signal"],1);
        assert_eq!(store.money_corpus_stats().unwrap()["money_fact"],1);
    }
    #[test] fn autopilot_generation_cannot_reuse_old_jobs_or_double_queue(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("money_campaign",&json!({"id":"c1","lastJobId":null})).unwrap();
        store.put("money_autopilot",&json!({"id":"c1","runId":"r1",
            "enabled":true,"scheduledRounds":0})).unwrap();
        let job=json!({"id":"job:r1:1","kind":"money_discover",
            "status":"queued","input":{"mode":"signals"}});
        let next=json!({"id":"c1","runId":"r1","enabled":true,
            "scheduledRounds":1,"lastJobId":"job:r1:1"});
        assert!(store.money_claim_job("c1",&job,Some(("r1",0,&next))).unwrap().is_some());
        assert!(store.money_claim_job("c1",&job,Some(("r1",0,&next))).unwrap().is_none());
        let stale=json!({"id":"job:stale","kind":"money_discover","status":"queued",
            "input":{"mode":"cross"}});
        assert!(store.money_claim_job("c1",&stale,Some(("r1",0,&next))).unwrap().is_none());
        assert_eq!(store.get("money_campaign","c1").unwrap()["lastJobId"],"job:r1:1");
    }
    #[test] fn schedule_modes_are_diverse(){
        assert_eq!((0..6).map(exploration_mode).collect::<Vec<_>>(),vec!["signals","discover","cross","signals","cross","challenge"]);
    }
    #[test] fn persistent_knowledge_recall_is_indexed_and_deduplicated(){
        let store=crate::db::Store::open(std::path::Path::new(":memory:")).unwrap();
        let sample=json!({"id":"knowledge1","claim":"중국 기술로 의료 장비 비용 절감 가능성",
            "url":"https://example.org/market","verification":"model_claim_unverified"});
        store.money_fact_put(&sample).unwrap();
        store.money_fact_put(&sample).unwrap();
        let found=store.money_fact_search("의료 장비 비용",10).unwrap();
        assert_eq!(found.len(),1);
        assert_eq!(found[0]["verification"],"model_claim_unverified");
        assert!(store.money_fact_search("무관한 단어 xxyyx",10).unwrap().is_empty());
    }
    #[test] fn fact_identity_is_stable(){
        assert_eq!(fact_id(" https://example.org/a ","a b"),fact_id("https://example.org/a","a   b"));
    }
    #[test] fn empty_but_valid_market_scan_is_not_an_error(){
        assert!(parse_output(r#"{"overview":"기회 없음","sourceSignals":[],"opportunities":[]}"#).is_ok());
    }
    #[test] fn parses_json_in_fence(){
        assert!(parse_output("~~~\n{\"opportunities\":[{\"title\":\"a\"}]}\n~~~").is_ok());
        assert!(parse_output("not json").is_err());
    }
}

/// Called from the host's existing timer, but does no collection work when
/// Money has not been explicitly enabled by the user.
pub fn tick(state:&AppState){
    if !state.store.all("money_autopilot").iter().any(|v|v["enabled"]==true){
        return;
    }
    health::sample(state);
    sources::schedule(state);
    catalog::schedule(state);
    folding::schedule(state);
    schedule_autopilot(state);
}

/// One adapter for every Money job. The host owns its queue, cancellation,
/// token accounting, permissions and provider/tool execution.
pub async fn dispatch(context:&JobContext,job:&Value)->Result<Value,String>{
    match job["kind"].as_str(){
        Some("money_discover")=>run(context,job).await,
        Some("money_ingest")=>sources::run(context,job).await,
        Some("money_catalog")=>catalog::run(context,job).await,
        Some("money_fold")=>folding::run(context,job).await,
        _=>Err("unsupported Money job kind".into()),
    }
}
