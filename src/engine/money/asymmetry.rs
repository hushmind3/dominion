//! Evidence-paired information-asymmetry discovery. Pure deterministic logic:
//! never infer that a product/service is ABSENT from a country merely because
//! no search result was collected. Pairs are prompts for falsifiable research.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{HashMap,HashSet};

const AXES:[&str;4]=["regional","temporal","channel","interpretive"];
fn field<'a>(v:&'a Value,key:&str)->&'a str{v[key].as_str().unwrap_or("")}
fn words(v:&Value,key:&str)->Vec<String>{
    v[key].as_array().into_iter().flatten()
        .filter_map(Value::as_str).take(10).map(|s|s.to_string()).collect()
}
fn published(v:&Value)->Option<chrono::DateTime<chrono::Utc>>{
    let raw=v["publishedAt"].as_str().unwrap_or("");
    chrono::DateTime::parse_from_rfc3339(raw)
        .or_else(|_|chrono::DateTime::parse_from_rfc2822(raw))
        .ok().map(|time|time.with_timezone(&chrono::Utc))
}
fn digest(raw:&str)->String{format!("{:x}",Sha256::digest(raw.as_bytes()))}
fn make_id(axis:&str,a:&str,b:&str)->String{
    let (x,y)=if a<b{(a,b)}else{(b,a)};
    format!("asymmetry:{}",digest(&format!("{axis}:{x}:{y}")))
}
fn valid_signal(v:&Value)->bool{
    !field(v,"id").is_empty() && field(v,"url").starts_with("https://")
        && !field(v,"sourceName").is_empty() && !words(v,"topics").is_empty()
}
fn pair_axis(a:&Value,b:&Value,axis:&str,common:&str)->Option<(f64,String,String)>{
    let changed_a=words(a,"changeTags");let changed_b=words(b,"changeTags");
    let region_a=field(a,"marketRegion");let region_b=field(b,"marketRegion");
    let kind_a=field(a,"sourceKind");let kind_b=field(b,"sourceKind");
    // Different RSS search channels are not independent publishers. Match
    // the same source-identity rules as Money mechanism search and reject
    // exact story mirrors even when URLs or feed section names differ.
    let publisher_a=crate::engine::money_mechanism::source_host(a);
    let publisher_b=crate::engine::money_mechanism::source_host(b);
    let story_a=field(a,"storyClusterId");
    let story_b=field(b,"storyClusterId");
    let different_sources=!publisher_a.is_empty()
        && !publisher_b.is_empty() && publisher_a!=publisher_b
        && field(a,"url")!=field(b,"url")
        && (story_a.is_empty()||story_b.is_empty()||story_a!=story_b);
    let directed=|which:&[String],term:&str|which.iter().any(|value|value==term);
    match axis {
        "regional" if region_a!=region_b
            && !["","GLOBAL","UNKNOWN"].contains(&region_a)
            && !["","GLOBAL","UNKNOWN"].contains(&region_b)
            && different_sources=>{
            Some((62.0,format!("같은 {common} 분야에서 {region_a}와 {region_b}의 적용·판매·구매 조건이 다른가?"),
                "양 시장의 실제 가격·고객 지출·대체재를 각각 수집해 차이를 입증해야 함".into()))
        },
        "temporal" if different_sources=>{
            let left=published(a)?;let right=published(b)?;
            let days=(left-right).num_days().unsigned_abs();
            if days<4 {return None;}
            let shift=changed_a.iter().any(|x|changed_b.iter().any(|y|x!=y));
            if !shift{return None;}
            Some((50.0+(days.min(21) as f64)*0.45,
                format!("{common} 분야의 {days}일 간격 신호가 기술·비용·규제 변화의 선행 조건인가?"),
                "동일 지표의 2개 시점 실측값과 변화 원인을 확보해야 함".into()))
        },
        "channel" if kind_a!=kind_b && !kind_a.is_empty()&&!kind_b.is_empty()
            && different_sources=>{
            Some((53.0,format!("{common} 분야에서 {kind_a} 공급정보와 {kind_b} 수요/불만 신호 사이 유통 장벽은?"),
                "동일 제품·기능의 실제 거래가격, 구매 절차, 유통 비용 확인".into()))
        },
        "interpretive" if different_sources
            && (directed(&changed_a,"policy")||directed(&changed_b,"policy"))
            && (kind_a=="policy_feed"||kind_b=="policy_feed")=>{
            Some((58.0,format!("{common} 분야의 규정 공고와 기술·고객 변화 사이 새로운 준수 비용 또는 판매 기회는?"),
                "원문 규정의 관할·발효일·법적 적용 대상을 우선 확인".into()))
        },
        _=>None
    }
}
/// Score pairs, cap source concentration, and preserve independent evidence.
pub fn discover(records:&[Value],max_results:usize)->Vec<Value>{
    let records=records.iter().filter(|v|valid_signal(v)).take(120).collect::<Vec<_>>();
    let mut ranked=Vec::new();
    let mut seen=HashSet::new();
    for (i,a) in records.iter().enumerate(){
        for b in records.iter().skip(i+1){
            if field(a,"sourceName")==field(b,"sourceName")
                ||field(a,"url")==field(b,"url"){continue;}
            let publisher_a=crate::engine::money_mechanism::source_host(a);
            let publisher_b=crate::engine::money_mechanism::source_host(b);
            if publisher_a.is_empty() || publisher_b.is_empty()
                ||publisher_a==publisher_b{continue;}
            let story_a=field(a,"storyClusterId");
            if !story_a.is_empty() && story_a==field(b,"storyClusterId"){continue;}
            let topics_a=words(a,"topics");let topics_b=words(b,"topics");
            let common=topics_a.iter().find(|t|topics_b.contains(t));
            let Some(common)=common else{continue;};
            for axis in AXES{
                let Some((priority,question,missing_proof))=pair_axis(a,b,axis,common)else{continue;};
                let id=make_id(axis,field(a,"id"),field(b,"id"));
                if !seen.insert(id.clone()){continue;}
                ranked.push(json!({"id":id,"axis":axis,
                    "sourceA":field(a,"id"),"sourceB":field(b,"id"),
                    "urlA":field(a,"url"),"urlB":field(b,"url"),
                    "sourceNameA":field(a,"sourceName"),"sourceNameB":field(b,"sourceName"),
                    "publisherA":publisher_a,"publisherB":publisher_b,
                    "sourceIndependence":"distinct_publisher_domains_not_independent_reporting_proven",
                    "regionA":field(a,"marketRegion"),"regionB":field(b,"marketRegion"),
                    "kindA":field(a,"sourceKind"),"kindB":field(b,"sourceKind"),
                    "titleA":field(a,"title"),"titleB":field(b,"title"),
                    "topic":common,"hypothesis":question,"missingProof":missing_proof,
                    "researchPriority":priority.round() as u64,
                    "proofStatus":"two_source_comparison_not_verified_gap",
                    "absenceProved":false,
                    "sourceCount":2,"createdAt":chrono::Utc::now().to_rfc3339()}));
            }
        }
    }
    ranked.sort_by(|a,b|b["researchPriority"].as_u64().cmp(&a["researchPriority"].as_u64())
        .then_with(||a["id"].as_str().cmp(&b["id"].as_str())));
    let mut result=Vec::new();let mut by_axis=HashMap::<String,usize>::new();
    let mut used=HashSet::new();
    for row in ranked {
        let axis=field(&row,"axis").to_string();
        let pair=format!("{}-{}",field(&row,"sourceA"),field(&row,"sourceB"));
        if used.contains(&pair)||*by_axis.get(&axis).unwrap_or(&0)>=8{continue;}
        *by_axis.entry(axis).or_default()+=1;
        used.insert(pair);
        result.push(row);
        if result.len()>=max_results.clamp(1,32){break;}
    }
    result
}
/// An agent may reference ONLY a precomputed pair and the exact source IDs.
/// Looking up the two primary source rows happens in the domain ingest path.
pub fn validate_pair(pair:&Value,record:&Value,source_a:&Value,source_b:&Value)->Result<(),String>{
    let id=field(pair,"id");
    let axis=field(pair,"axis");
    if id.is_empty()||!AXES.contains(&axis) {return Err("invalid asymmetry id or axis".into());}
    if field(record,"id")!=id||field(record,"axis")!=axis{
        return Err("asymmetry pair is not in the indexed research corpus".into());
    }
    if field(pair,"sourceA")!=field(record,"sourceA")
        ||field(pair,"sourceB")!=field(record,"sourceB") {
        return Err("pair references do not match stored evidence".into());
    }
    if field(record,"sourceA")!=field(source_a,"id")
        ||field(record,"sourceB")!=field(source_b,"id")
        ||!valid_signal(source_a)||!valid_signal(source_b){
        return Err("pair source records are missing or invalid".into());
    }
    if field(record,"urlA")!=field(source_a,"url")
        ||field(record,"urlB")!=field(source_b,"url"){
        return Err("paired URL provenance changed".into());
    }
    let origin_a=crate::engine::money_mechanism::source_host(source_a);
    let origin_b=crate::engine::money_mechanism::source_host(source_b);
    if origin_a.is_empty()||origin_b.is_empty()||origin_a==origin_b{
        return Err("paired source origins are not independent publishers".into());
    }
    let overlap=words(source_a,"topics").into_iter()
        .any(|tag|words(source_b,"topics").contains(&tag));
    if !overlap{return Err("source topics are not comparable".into());}
    if pair_axis(source_a,source_b,axis,field(record,"topic")).is_none(){
        return Err("market evidence does not satisfy requested asymmetry axis".into());
    }
    Ok(())
}
pub fn briefing(pairs:&[Value],limit:usize)->String{
    pairs.iter().take(limit.min(8)).map(|p|format!(
        "ID={}; 축={}; 가설={}; A: {} ({}) B: {} ({}); 검증 전: {}",
        field(p,"id"),field(p,"axis"),field(p,"hypothesis"),
        field(p,"titleA"),field(p,"urlA"),field(p,"titleB"),field(p,"urlB"),
        field(p,"missingProof")
    )).collect::<Vec<_>>().join("\n")
}
#[cfg(test)]
mod tests{
    use super::*;
    fn signal(id:&str,region:&str,source:&str,kind:&str)->Value{
        let host=source.chars().filter(|ch|ch.is_ascii_alphanumeric())
            .collect::<String>().to_ascii_lowercase();
        json!({"id":id,"url":format!("https://{host}.test/{id}"),
            "sourceName":source,"title":format!("비용 관련 {id}"),
            "marketRegion":region,"sourceKind":kind,
            "topics":["hardware","software"],"changeTags":["launch"],
            "publishedAt":"2026-11-11T12:00:00Z"})
    }
    #[test] fn compares_regions_without_asserting_absence(){
        let a=signal("1","CN","Chinese publication","news");
        let b=signal("2","KR","Korean publication","news");
        let res=discover(&[a.clone(),b.clone()],10);
        assert_eq!(res.len(),1);
        assert_eq!(res[0]["axis"],"regional");
        assert_eq!(res[0]["absenceProved"],false);
        assert!(validate_pair(&res[0],&res[0],&a,&b).is_ok());
    }
    #[test] fn rejects_single_or_fabricated_sources(){
        let a=signal("1","CN","A","news");
        assert!(discover(&[a.clone()],10).is_empty());
        let b=signal("2","KR","B","news");
        let r=discover(&[a.clone(),b.clone()],10).remove(0);
        let fake=json!({"id":r["id"],"axis":"regional","sourceA":"fake","sourceB":"2"});
        assert!(validate_pair(&fake,&r,&a,&b).is_err());
    }
    #[test] fn duplicate_syndicated_stories_cannot_create_two_source_market_hypotheses(){
        let mut a=signal("1","CN","Chinese seller","news");
        let mut b=signal("2","KR","Korean buyer","news");
        a["storyClusterId"]=json!("shared-title");
        b["storyClusterId"]=json!("shared-title");
        assert!(discover(&[a.clone(),b.clone()],12).is_empty());
        b["storyClusterId"]=json!("different-title");
        assert!(!discover(&[a.clone(),b.clone()],12).is_empty());
        b["publisherDomain"]=json!("news.google.com");
        assert!(discover(&[a,b],12).is_empty());
    }
    #[test] fn two_rss_channels_for_one_publisher_are_not_independent(){
        let a=signal("a","CN","Market news feed","news");
        let mut b=signal("b","KR","Market buyer feed","news");
        b["publisherDomain"]=json!("www.marketnewsfeed.test");
        assert!(discover(&[a,b],12).is_empty());
    }

    #[test] fn temporal_comparison_requires_dated_independent_observations(){
        let a=signal("1","CN","A","news");
        let mut b=signal("2","CN","B","news");
        b["publishedAt"]=json!("2026-11-01T00:00:00Z");
        b["changeTags"]=json!(["cost_rise"]);
        assert!(discover(&[a,b],10).iter().any(|v|v["axis"]=="temporal"));
    }
}
