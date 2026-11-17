//! Deterministic, explainable Money opportunity ranking and strategy feedback.
//! All estimates are heuristics; only user-reported completed experiments are
//! eligible for outcome credit. No model calls or parameter fine tuning.
use serde_json::{json,Value};
use std::collections::HashMap;

const MODES:[&str;4]=["signals","discover","cross","challenge"];
fn nonnegative(v:&Value)->Option<f64>{
    v.as_f64().filter(|v|v.is_finite()&&*v>=0.0&&*v<=1e14)
}
pub fn experiment_net(v:&Value)->Option<f64>{
    if v["status"]!="finished"{return None;}
    let revenue=nonnegative(&v["reportedRevenueWon"])?;
    let direct=nonnegative(&v["reportedCostWon"])?;
    // Missing spending data is unknown, never a zero-cost observation.
    let extra=["marketingCostWon","modelCostWon","laborCostWon"].iter()
        .map(|k|nonnegative(&v[*k]))
        .collect::<Option<Vec<_>>>()?.iter().sum::<f64>();
    Some(revenue-direct-extra)
}
fn candidate_mode(v:&Value)->&str{
    v["discoveryMode"].as_str().filter(|v|MODES.contains(v)).unwrap_or("discover")
}
pub fn strategy_stats(candidates:&[Value],experiments:&[Value])->Vec<Value>{
    let by_id=candidates.iter().filter_map(|c|
        c["id"].as_str().map(|id|(id.to_string(),candidate_mode(c))))
        .collect::<HashMap<_,_>>();
    MODES.into_iter().map(|mode|{
        let finished=experiments.iter().filter(|e|e["candidateId"].as_str()
            .and_then(|id|by_id.get(id)).is_some_and(|stored|*stored==mode)
            &&e["status"]=="finished").collect::<Vec<_>>();
        let relevant=finished.iter().filter_map(|e|experiment_net(e)).collect::<Vec<_>>();
        let incomplete=finished.len().saturating_sub(relevant.len()) as u64;
        let completed=relevant.len() as u64;
        let positive=relevant.iter().filter(|v|**v>0.0).count() as u64;
        let net=relevant.iter().sum::<f64>();
        // Beta(1,1) prior suppresses overconfidence from single wins.
        let posterior=(positive as f64+1.0)/(completed as f64+2.0);
        json!({"mode":mode,"finishedExperiments":completed,
            "excludedIncompleteCostReports":incomplete,"reportedPositiveNet":positive,"reportedNetWon":net,
            "smoothedPositiveRate":(posterior*1000.0).round()/1000.0,
            "source":"user_reported_not_payment_verified"})
    }).collect()
}
pub fn next_mode(round:u64,candidates:&[Value],experiments:&[Value])->&'static str{
    // Hard-coded exploration guarantees periodic novel directions.
    let baseline=MODES[round as usize%MODES.len()];
    let stats=strategy_stats(candidates,experiments);
    let total=stats.iter().map(|s|s["finishedExperiments"].as_u64().unwrap_or(0)).sum::<u64>();
    if total<4||round%5==0{return baseline;}
    let mut chosen_index=round as usize%MODES.len();
    let mut best=f64::NEG_INFINITY;
    for (i,_mode) in MODES.iter().enumerate(){
        let s=&stats[i];
        let n=s["finishedExperiments"].as_u64().unwrap_or(0) as f64;
        let success=s["smoothedPositiveRate"].as_f64().unwrap_or(0.5);
        let exploration=(2.0*(total as f64+1.0).ln()/(n+1.0)).sqrt()*0.16;
        let mean_net=s["reportedNetWon"].as_f64().unwrap_or(0.0)/(n.max(1.0));
        let payoff=(mean_net/100000.0).clamp(-1.0,1.0)*0.15;
        let score=success+exploration+payoff;
        if score>best {best=score;chosen_index=i;}
    }
    MODES[chosen_index]
}
pub fn rank_candidate(candidate:&Value,experiments:&[Value],capital_won:f64)->Value{
    let mut score=20.0;
    let mut reasons=Vec::<String>::new();
    let present=|key:&str|candidate[key].as_str().is_some_and(|v|!v.trim().is_empty());
    if present("customer")&&present("whyPay"){score+=15.0;reasons.push("구체적인 구매자·지불동기".into());}
    if present("distribution"){score+=9.0;reasons.push("고객 접근 경로".into());}
    if present("firstBuyer")&&present("firstPaymentStep"){
        score+=12.0;reasons.push("결제 담당자와 첫 유료 거래 경로".into());
    }else{score-=5.0;reasons.push("첫 유료 고객 경로 불명확".into());}
    let purchase_requests=candidate["buyerEvidence"].as_array().into_iter().flatten()
        .filter(|v|crate::engine::money_buyer::buying_signal(v)).count();
    if purchase_requests>0{
        score+=(purchase_requests.min(2)*7) as f64;
        reasons.push("저장된 원문과 대조된 구매 표현(실제 결제와 구분)".into());
    }

    if present("firstTest")&&present("killSignal"){score+=11.0;reasons.push("검증 실험과 폐기 기준".into());}
    let sources=candidate["evidence"].as_array().into_iter().flatten()
        .filter_map(|e|e["url"].as_str()).filter(|url|url.starts_with("https://"))
        .collect::<std::collections::HashSet<_>>().len();
    if sources>0 {score+=(sources.min(3) as f64)*4.0;reasons.push("추적 가능한 출처".into());}
    else {score-=6.0;reasons.push("출처 부족".into());}
    let econ=&candidate["economics"];
    let price=nonnegative(&econ["priceWon"]);
    let variable=nonnegative(&econ["variableCostWon"]);
    let acquisition=nonnegative(&econ["acquisitionCostWon"]);
    let fixed=nonnegative(&econ["monthlyFixedWon"]);
    let units=nonnegative(&econ["monthlyUnits"]);
    let refund=nonnegative(&econ["refundRatePct"]).filter(|v|*v<=100.0);
    let unit_margin=match (price,variable,acquisition,refund){
        (Some(p),Some(v),Some(a),Some(r))=>Some(p*(1.0-r/100.0)-v-a),
        _=>None
    };
    let forecast=match (unit_margin,units,fixed){
        (Some(m),Some(n),Some(f))=>Some(m*n-f),
        _=>None
    };
    match unit_margin{
        Some(m) if m>0.0=>{score+=7.0;reasons.push("가정 기준 양의 공헌이익".into());}
        Some(_)=>{score-=28.0;reasons.push("가정 기준 역마진".into());}
        None=>reasons.push("원가·고객획득비 미확인".into())
    }
    let setup=nonnegative(&econ["setupCostWon"]);
    if let Some(v)=setup{
        if capital_won.is_finite()&&capital_won>=0.0 {
            if v>capital_won{
                score-=20.0;reasons.push("사용자가 지정한 가용자본 초과".into());
            }else{score+=5.0;}
        }else{
            reasons.push("가용자본 미지정 · 투자금 제한 점수 미적용".into());
        }
    }
    let novelty=candidate["noveltyEstimate"].as_f64().filter(|n|n.is_finite()&&*n>=0.0&&*n<=1.0)
        .unwrap_or(0.5);
    score+=(novelty-0.5)*12.0;
    let completed=experiments.iter().filter(|e|e["candidateId"]==candidate["id"])
        .filter_map(experiment_net).collect::<Vec<_>>();
    let observed_net=completed.iter().sum::<f64>();
    let missing_costs=experiments.iter().filter(|e|
        e["candidateId"]==candidate["id"]&&e["status"]=="finished"
        &&experiment_net(e).is_none()).count();
    if missing_costs>0{reasons.push("비용 명세가 누락된 실험은 수익 학습에서 제외".into());}
    if !completed.is_empty(){
        score+=if observed_net>0.0{14.0}else if observed_net<0.0{-14.0}else{0.0};
        reasons.push(format!("실험 {}건 사용자 보고 순현금흐름",completed.len()));
    }
    let next=if missing_costs>0&&completed.is_empty(){
        "누락된 마케팅·모델·노동비를 기록하여 실험 순이익 확인"
    }else if purchase_requests==0&&completed.is_empty() {
        "실제 예산·구매 요청과 결제 담당자 확인"
    }else if completed.is_empty()&&sources==0{
        "공식 가격·고객 문제 등 1차 근거 확보"
    }else if completed.is_empty()&&unit_margin.is_none(){
        "가격·원가·고객획득비 확인"
    }else if completed.is_empty(){
        "실제 지불 의사·유료 주문 실험"
    }else if observed_net<=0.0{
        "지출 원인 및 고객 전환 실패 분석"
    }else{
        "반복 판매 가능성과 영업 확장 검증"
    };
    json!({"score":score.clamp(0.0,100.0).round() as u64,
        "scoreType":"decision_priority_heuristic_not_success_probability",
        "reasons":reasons,"nextBestTest":next,
        "reportedNetWon":observed_net,"finishedExperiments":completed.len(),
        "excludedIncompleteCostReports":missing_costs,
        "assumedMonthlyNetWon":forecast,
        "forecastStatus":if forecast.is_some(){"assumption_only"}else{"insufficient_inputs"},
        "sourceCount":sources,"publicPurchaseEvidenceLinks":purchase_requests,
        "buyerEvidenceStatus":if purchase_requests>0{"source_excerpt_buyer_intent"}else{"no_buyer_purchase_proof"}})
}
/// Compare a stated 30-day profit assumption with a completed experiment only
/// if its period is supplied. A 1-day experiment must not be compared directly
/// with a monthly forecast.
pub fn forecast_error(candidate:&Value,experiments:&[Value])->Value{
    let score=rank_candidate(candidate,&[],-1.0);
    let Some(predicted)=score["assumedMonthlyNetWon"].as_f64()else{
        return json!({"comparable":false,"reason":"missing_monthly_forecast"});
    };
    let mut periods=0.0;
    let mut actual=0.0;
    for exp in experiments{
        if exp["candidateId"]!=candidate["id"]{continue;}
        let Some(net)=experiment_net(exp)else{continue;};
        let Some(days)=nonnegative(&exp["periodDays"]).filter(|v|*v>0.0&&*v<=366.0)else{continue;};
        periods+=days;
        actual+=net;
    }
    if periods<=0.0{return json!({"comparable":false,"reason":"missing_completed_period"});}
    let observed_monthly=actual/periods*30.0;
    json!({"comparable":true,"assumedMonthlyNetWon":predicted,
        "observedMonthlyEquivalentWon":observed_monthly,
        "absoluteErrorWon":(predicted-observed_monthly).abs(),
        "sampleDays":periods,"source":"user_reported_not_audited"})
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn feedback_favors_positive_reported_outcomes_but_explores(){
        let candidates=vec![json!({"id":"one","discoveryMode":"signals"}),
            json!({"id":"two","discoveryMode":"cross"})];
        let mut outcomes=Vec::new();
        for i in 0..8{
            outcomes.push(json!({"candidateId":if i<4{"one"}else{"two"},"status":"finished",
                "reportedRevenueWon":if i<4{100000}else{0},
                "reportedCostWon":10000,
                "marketingCostWon":0,"modelCostWon":0,"laborCostWon":0}));
        }
        let stats=strategy_stats(&candidates,&outcomes);
        assert!(stats[0]["smoothedPositiveRate"].as_f64().unwrap()
            >stats[2]["smoothedPositiveRate"].as_f64().unwrap());
        assert_eq!(next_mode(1,&candidates,&outcomes),"signals");
        assert_eq!(next_mode(5,&candidates,&outcomes),"discover");
    }
    #[test] fn missing_costs_are_never_positive_adaptive_feedback(){
        let incomplete=json!({"candidateId":"one","status":"finished",
            "reportedRevenueWon":100000,"reportedCostWon":10000});
        assert_eq!(experiment_net(&incomplete),None);
        let candidates=vec![json!({"id":"one","discoveryMode":"signals"})];
        let stats=strategy_stats(&candidates,&[incomplete.clone()]);
        assert_eq!(stats[0]["finishedExperiments"],0);
        assert_eq!(stats[0]["excludedIncompleteCostReports"],1);
        let rank=rank_candidate(&candidates[0],&[incomplete],-1.0);
        assert_eq!(rank["excludedIncompleteCostReports"],1);
        let complete=json!({"status":"finished","reportedRevenueWon":100000,
            "reportedCostWon":10000,"marketingCostWon":1000,
            "modelCostWon":2000,"laborCostWon":3000});
        assert_eq!(experiment_net(&complete),Some(84000.0));
    }
    #[test] fn rank_rejects_negative_margin_and_capital_mismatch(){
        let candidate=json!({"id":"x","customer":"factory","whyPay":"saves labor",
            "economics":{"priceWon":10000,"variableCostWon":15000,
                "acquisitionCostWon":5000,"refundRatePct":0,"setupCostWon":1000000}});
        let rank=rank_candidate(&candidate,&[],500000.0);
        assert!(rank["score"].as_u64().unwrap()<30);
    }
    #[test] fn buyer_evidence_is_distinct_from_paid_customer(){
        let c=json!({"id":"x","customer":"manufacturer","whyPay":"cut cost",
            "firstBuyer":"procurement head","firstPaymentStep":"paid pilot",
            "buyerEvidence":[{"type":"public_purchase_request","url":"https://example.org/rfp"}]});
        let r=rank_candidate(&c,&[],1000000.0);
        assert_eq!(r["publicPurchaseEvidenceLinks"],0);
        assert_eq!(r["finishedExperiments"],0);
        assert_eq!(r["buyerEvidenceStatus"],"no_buyer_purchase_proof");
        let corroborated=json!({"id":"x","buyerEvidence":[{"type":"public_purchase_request",
            "url":"https://example.org/rfp","observationStatus":"source_excerpt_buyer_intent"}]});
        let observed=rank_candidate(&corroborated,&[],1000000.0);
        assert_eq!(observed["publicPurchaseEvidenceLinks"],1);
        assert_eq!(observed["buyerEvidenceStatus"],"source_excerpt_buyer_intent");
    }
    #[test] fn unspecified_capital_never_imposes_zero_budget(){
        let item=json!({"id":"candidate","economics":{"setupCostWon":2000000}});
        let open=rank_candidate(&item,&[],-1.0);
        let zero=rank_candidate(&item,&[],0.0);
        let allowed=rank_candidate(&item,&[],3000000.0);
        assert!(open["score"].as_u64().unwrap()>zero["score"].as_u64().unwrap());
        assert!(allowed["score"].as_u64().unwrap()>open["score"].as_u64().unwrap());
        assert!(open["reasons"].as_array().unwrap().iter().any(|r|
            r.as_str().unwrap_or("").contains("가용자본 미지정")));
    }
    #[test] fn forecast_error_requires_actual_observation_period(){
        let c=json!({"id":"x","economics":{"priceWon":100000,"variableCostWon":50000,
            "acquisitionCostWon":0,"refundRatePct":0,"monthlyFixedWon":0,"monthlyUnits":20}});
        let no_period=vec![json!({"candidateId":"x","status":"finished",
            "reportedRevenueWon":900000,"reportedCostWon":100000,
            "marketingCostWon":0,"modelCostWon":0,"laborCostWon":0})];
        assert_eq!(forecast_error(&c,&no_period)["comparable"],false);
        let yes=vec![json!({"candidateId":"x","status":"finished","periodDays":30,
            "reportedRevenueWon":900000,"reportedCostWon":100000,
            "marketingCostWon":0,"modelCostWon":0,"laborCostWon":0})];
        let out=forecast_error(&c,&yes);
        assert_eq!(out["absoluteErrorWon"],200000.0);
    }
}
