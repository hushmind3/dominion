//! Deterministic cross-market economic tension search.
//!
//! Unlike an LLM-generated brainstorm, each hypothesis is created from an
//! identifiable pair of dated source observations with distinct economic roles.
//! The resulting opportunity is a RESEARCH QUESTION, never a validated deal.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{HashMap,HashSet};

#[derive(Clone, Copy,Debug)]
struct Mechanism {
    supply:&'static str,
    buyer:&'static str,
    kind:&'static str,
    question:&'static str,
    next_proof:&'static str,
}
const PATTERNS:&[Mechanism]=&[
    Mechanism{ supply:"cost_drop",buyer:"buyer_request",kind:"buyer_supply_spread",
        question:"공개 구매 요청의 실제 요구 규격을 최근 저렴해진 공급 기술로 충족할 수 있는가?",
        next_proof:"원문 구매문서·마감일·수요기관·규격·기존 계약 단가와 공급 견적 확인"},
    Mechanism{ supply:"launch",buyer:"buyer_request",kind:"new_tool_to_buyer",
        question:"새로 나온 도구가 실제 공개된 구매자의 반복업무를 줄일 수 있는가?",
        next_proof:"공식 구매문서와 새 기술의 라이선스·기능 적합성 및 담당 부서 확인"},
    Mechanism{ supply:"cost_drop",buyer:"cost_rise",kind:"input_substitution",
        question:"원가가 낮아진 기술을 비용이 상승하는 업종의 기존 구매·노동 방식에 적용할 수 있는가?",
        next_proof:"공급 견적, 기능 대체 가능성, 현지 고객의 현재 지출을 확인"},
    Mechanism{ supply:"launch",buyer:"shortage",kind:"supply_substitution",
        question:"새로 등장한 기술·상품이 공급 부족으로 기다리는 고객을 실질적으로 대체할 수 있는가?",
        next_proof:"규격·인증·납기 일치 여부와 실제 발주 담당자 확인"},
    Mechanism{ supply:"cost_drop",buyer:"demand",kind:"demand_cost_spread",
        question:"원가가 떨어진 공급수단을 수요가 나타나는 구매처까지 더 싸고 빠르게 전달할 수 있는가?",
        next_proof:"구매자의 계약 금액·구매 주기와 실구매 견적 확인"},
    Mechanism{ supply:"launch",buyer:"cost_rise",kind:"automation_transfer",
        question:"새 기술이 비용 상승 업종에서 반복적으로 발생하는 유료 업무를 자동화할 수 있는가?",
        next_proof:"현재 지불액, 자동화 가능 작업 비율, 대체 거부 비용 확인"},
    Mechanism{ supply:"launch",buyer:"policy",kind:"compliance_product",
        question:"새 기술로 변화한 규정에 대응하는 상품이나 인증 지원을 만들 수 있는가?",
        next_proof:"원문 규정·적용 대상·법적 제공자격·유료 수요 확인"},
    Mechanism{ supply:"cost_drop",buyer:"shortage",kind:"bottleneck_bypass",
        question:"가격이 낮아진 대체 공급이나 부품으로 공급망 병목을 제거할 수 있는가?",
        next_proof:"대체 규격, 고객의 구매권한, 최소주문량·납기 확인"},
];
fn tags<'a>(v:&'a Value,key:&str)->Vec<&'a str>{
    v[key].as_array().into_iter().flatten().filter_map(Value::as_str).take(12).collect()
}
fn source_name(v:&Value)->&str{v["sourceName"].as_str().unwrap_or("")}
fn item_id(v:&Value)->&str{v["id"].as_str().unwrap_or("")}
/// Different RSS feed names do not imply independent reporting: e.g. two
/// sections of one publisher or a mirrored headline from one aggregator.
fn source_host(v:&Value)->String{
    let raw=v["url"].as_str().unwrap_or("");
    let Ok(url)=reqwest::Url::parse(raw)else{return String::new()};
    if url.scheme()!="https"{return String::new();}
    let Some(host)=url.host_str()else{return String::new()};
    let host=host.trim_start_matches("www.").to_ascii_lowercase();
    let labels=host.split('.').collect::<Vec<_>>();
    let suffixes=["co.kr","or.kr","go.kr","com.cn","com.tw","co.jp","co.uk","com.au","co.nz"];
    let depth=if suffixes.iter().any(|suffix|host.ends_with(&format!(".{suffix}"))){3}else{2};
    if labels.len()<depth{return host;}
    labels[labels.len()-depth..].join(".")
}
/// Only published dates are recency evidence. Missing dates stay unknown.
fn age_penalty(v:&Value,now:&chrono::DateTime<chrono::Utc>)->Option<f64>{
    let raw=v["publishedAt"].as_str().unwrap_or("");
    if raw.is_empty(){return Some(6.0);}
    let parsed=chrono::DateTime::parse_from_rfc3339(raw)
        .or_else(|_|chrono::DateTime::parse_from_rfc2822(raw));
    let Ok(date)=parsed else{return Some(12.0)};
    let age=now.signed_duration_since(date.with_timezone(&chrono::Utc)).num_days();
    if !(-2..=180).contains(&age){return None;}
    Some(match age {..=14=>0.0,15..=45=>5.0,46..=90=>12.0,_=>20.0})
}
fn bounded(v:&Value,key:&str,max:usize)->String{
    v[key].as_str().unwrap_or("").chars().take(max).collect()
}
fn pair_score(supply:&Value,buyer:&Value,pattern:Mechanism)->Option<f64>{
    let a=tags(supply,"topics");
    let b=tags(buyer,"topics");
    if item_id(supply).is_empty()||item_id(buyer).is_empty()
        ||item_id(supply)==item_id(buyer)
        ||!tags(supply,"changeTags").contains(&pattern.supply)
        ||!tags(buyer,"changeTags").contains(&pattern.buyer)
        ||a.is_empty()||b.is_empty(){return None;}
    // Require independent origins and at least one cross-domain difference.
    if source_name(supply).is_empty()||source_name(buyer).is_empty()
        ||source_name(supply)==source_name(buyer){return None;}
    let supply_host=source_host(supply);
    let buyer_host=source_host(buyer);
    if supply_host.is_empty()||buyer_host.is_empty()||supply_host==buyer_host{
        return None;
    }
    let differences=a.iter().filter(|t|!b.contains(t)).count()
        +b.iter().filter(|t|!a.contains(t)).count();
    if differences==0{return None;}
    let common=a.iter().filter(|t|b.contains(t)).count();
    let now=chrono::Utc::now();
    let freshness=age_penalty(supply,&now)?+age_penalty(buyer,&now)?;
    let pages=(supply["contentStatus"]=="page_fetched") as u8
        +(buyer["contentStatus"]=="page_fetched") as u8;
    // Cross-sector invention remains possible, but a pair with a real
    // shared subject outranks an unlinked pair of buzzword headlines.
    let bridge=if common>0{17.0}else{-8.0};
    let buyer_intent=if pattern.buyer=="buyer_request"{12.0}else{0.0};
    let score=40.0+bridge+(differences.min(4) as f64)*5.0
        +(common.min(2) as f64)*7.0+pages as f64*5.0
        +buyer_intent-freshness;
    Some(score.clamp(0.0,90.0))
}
fn make_id(kind:&str,supply_id:&str,buyer_id:&str)->String{
    format!("tension:{:x}",Sha256::digest(format!("{kind}:{supply_id}:{buyer_id}").as_bytes()))
}
pub fn discover(signals:&[Value],max:usize)->Vec<Value>{
    let mut ranked=Vec::new();
    let mut seen=HashSet::new();
    // At most 120 independent records = at most 86k comparisons, no corpus scan.
    let window=signals.iter().take(120).collect::<Vec<_>>();
    for pattern in PATTERNS.iter().copied(){
        for (si,a) in window.iter().enumerate(){
            for (bi,b) in window.iter().enumerate(){
                if si==bi{continue;}
                let Some(score)=pair_score(a,b,pattern)else{continue;};
                let id=make_id(pattern.kind,item_id(a),item_id(b));
                if !seen.insert(id.clone()){continue;}
                let a_topics=tags(a,"topics");
                let b_topics=tags(b,"topics");
                ranked.push(json!({"id":id,"mechanism":pattern.kind,
                    "question":pattern.question,"nextProof":pattern.next_proof,
                    "supplySignalId":item_id(a),"buyerSignalId":item_id(b),
                    "supplyHeadline":bounded(a,"title",210),
                    "buyerHeadline":bounded(b,"title",210),
                    "supplyUrl":bounded(a,"url",1200),"buyerUrl":bounded(b,"url",1200),
                    "supplySource":source_name(a),"buyerSource":source_name(b),
                    "supplyTopics":a_topics,"buyerTopics":b_topics,
                    "supplyChange":pattern.supply,"buyerChange":pattern.buyer,
                    "researchPriority":score.round() as u64,
                    "topicBridgeStatus":if a_topics.iter().any(|t|b_topics.contains(t)){
                        "shared_topic_not_verified_causality"
                    }else{"cross_sector_bridge_unverified"},
                    "evidenceStatus":"two_independent_source_mentions_not_causality",
                    "purchaseEvidence":false,
                    "createdAt":chrono::Utc::now().to_rfc3339()}));
            }
        }
    }
    // Prevent same two articles consuming the full shortlist.
    ranked.sort_by(|a,b|b["researchPriority"].as_u64().cmp(&a["researchPriority"].as_u64())
        .then_with(||a["id"].as_str().cmp(&b["id"].as_str())));
    let mut used=HashMap::<String,usize>::new();
    let mut result=Vec::new();
    for item in ranked{
        let pair=format!("{}:{}",item["supplySignalId"],item["buyerSignalId"]);
        if *used.get(&pair).unwrap_or(&0)>=1 {continue;}
        let kind=item["mechanism"].as_str().unwrap_or("").to_string();
        if *used.get(&kind).unwrap_or(&0)>=3{continue;}
        // One viral story must not account for most of the shortlist merely
        // by matching several different industry keywords or news sections.
        let supply_key=format!("supply:{}",item["supplySignalId"].as_str().unwrap_or(""));
        let buyer_key=format!("buyer:{}",item["buyerSignalId"].as_str().unwrap_or(""));
        if *used.get(&supply_key).unwrap_or(&0)>=2
            ||*used.get(&buyer_key).unwrap_or(&0)>=2{continue;}
        *used.entry(pair).or_default()+=1;
        *used.entry(kind).or_default()+=1;
        *used.entry(supply_key).or_default()+=1;
        *used.entry(buyer_key).or_default()+=1;
        result.push(item);
        if result.len()>=max.clamp(1,30){break;}
    }
    result
}
pub fn briefing(tensions:&[Value],max:usize)->String{
    tensions.iter().take(max.min(9)).enumerate().map(|(i,v)|format!(
        "{}. [{}] 조사 질문: {}\n  공급쪽: {} ({})\n  구매문제쪽: {} ({})\n  다음 검증: {}",
        i+1,v["mechanism"].as_str().unwrap_or(""),
        v["question"].as_str().unwrap_or(""),
        v["supplyHeadline"].as_str().unwrap_or(""),v["supplyUrl"].as_str().unwrap_or(""),
        v["buyerHeadline"].as_str().unwrap_or(""),v["buyerUrl"].as_str().unwrap_or(""),
        v["nextProof"].as_str().unwrap_or("")
    )).collect::<Vec<_>>().join("\n")
}
#[cfg(test)]
mod tests{
    use super::*;
    fn article(id:&str,source:&str,topic:&[&str],change:&str)->Value{
        json!({"id":id,"sourceName":source,"title":format!("Source {id}"),
            "topics":topic,"changeTags":[change],
            "url":format!("https://{}.example/{id}",source.to_lowercase().replace(' ', "-")),"contentStatus":"page_fetched"})
    }
    #[test] fn discovers_cost_disparity_across_independent_industries(){
        let data=vec![article("a","source A",&["hardware","manufacturing"],"cost_drop"),
            article("b","source B",&["construction"],"cost_rise")];
        let out=discover(&data,12);
        assert_eq!(out.len(),1);
        assert_eq!(out[0]["mechanism"],"input_substitution");
        assert_eq!(out[0]["purchaseEvidence"],false);
        assert!(briefing(&out,2).contains("https://source-a.example/a"));
    }
    #[test] fn same_source_or_same_sector_is_not_independent_evidence(){
        let data=vec![article("a","same",&["hardware"],"cost_drop"),
            article("b","same",&["construction"],"cost_rise")];
        assert!(discover(&data,12).is_empty());
        let data=vec![article("a","A",&["hardware"],"cost_drop"),
            article("b","B",&["hardware"],"cost_rise")];
        assert!(discover(&data,12).is_empty());
    }
    #[test] fn two_feeds_from_same_publisher_are_not_independent_sources(){
        let a=article("a","Feed A",&["hardware"],"cost_drop");
        let mut b=article("b","Feed B",&["construction"],"cost_rise");
        b["url"]=a["url"].clone();
        assert!(discover(&[a,b],12).is_empty());
    }
    #[test] fn one_viral_supply_article_cannot_fill_the_entire_shortlist(){
        let mut signals=vec![article("supply","supplier",&["hardware"],"cost_drop")];
        for i in 0..6{
            signals.push(article(&format!("buyer{i}"),&format!("publisher{i}"),
                &["construction"],"cost_rise"));
        }
        let found=discover(&signals,30);
        assert_eq!(found.len(),2);
        assert!(found.iter().all(|item|item["evidenceStatus"]
            =="two_independent_source_mentions_not_causality"));
    }
    #[test] fn buyer_signal_joins_supplier_change_but_is_not_a_paid_order(){
        let data=vec![article("s","tech source",&["hardware"],"cost_drop"),
            article("b","buyer source",&["construction"],"buyer_request")];
        let out=discover(&data,10);
        assert_eq!(out.len(),1);
        assert_eq!(out[0]["mechanism"],"buyer_supply_spread");
        assert_eq!(out[0]["purchaseEvidence"],false);
    }
    #[test] fn subdomains_of_one_publisher_cannot_corrobate_each_other(){
        let mut a=article("a","News A",&["hardware"],"cost_drop");
        let mut b=article("b","News B",&["construction"],"cost_rise");
        a["url"]=json!("https://tech.example.co.kr/a");
        b["url"]=json!("https://industry.example.co.kr/b");
        assert_eq!(source_host(&a),"example.co.kr");
        assert_eq!(source_host(&b),"example.co.kr");
        assert!(discover(&[a,b],10).is_empty());
    }
    #[test] fn shared_topic_bridge_outranks_unlinked_pairs(){
        let weak=discover(&[
            article("w1","W1",&["robotics","hardware"],"cost_drop"),
            article("w2","W2",&["food","tourism"],"buyer_request")
        ],10);
        let strong=discover(&[
            article("s1","S1",&["robotics","hardware"],"cost_drop"),
            article("s2","S2",&["robotics","manufacturing"],"buyer_request")
        ],10);
        assert_eq!(weak[0]["topicBridgeStatus"],"cross_sector_bridge_unverified");
        assert_eq!(strong[0]["topicBridgeStatus"],"shared_topic_not_verified_causality");
        assert!(strong[0]["researchPriority"].as_u64()>weak[0]["researchPriority"].as_u64());
    }
    #[test] fn stale_market_source_does_not_create_a_new_research_trend(){
        let mut a=article("a","A",&["hardware"],"cost_drop");
        let b=article("b","B",&["construction"],"cost_rise");
        a["publishedAt"]=json!((chrono::Utc::now()-chrono::Duration::days(200)).to_rfc3339());
        assert!(discover(&[a.clone(),b.clone()],10).is_empty());
        a["publishedAt"]=json!((chrono::Utc::now()-chrono::Duration::days(2)).to_rfc3339());
        assert_eq!(discover(&[a,b],10).len(),1);
    }
    #[test] fn there_is_no_candidate_without_economic_complementary_signals(){
        let data=vec![article("a","A",&["hardware"],"launch"),
            article("b","B",&["construction"],"launch")];
        assert!(discover(&data,12).is_empty());
    }
}
