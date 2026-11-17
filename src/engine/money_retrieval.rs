//! Bounded, no-model-token retrieval for long-lived Money research.
//! Fuse several indexed SQLite result lists, then select corroborating and
//! cross-domain context without flooding a small model with repeated headlines.
//! Ranking is a retrieval heuristic, NEVER evidence that a market claim is true.
use serde_json::Value;
use std::collections::{HashMap,HashSet};

const RRF_K:f64=60.0;
const MAX_CONTEXT:usize=22;

struct Row {
    fact:Value,
    priority:f64,
    grams:HashSet<String>,
    domain:String,
}

fn grams(text:&str)->HashSet<String>{
    let chars=text.to_lowercase().chars()
        .filter(|ch|ch.is_alphanumeric()).take(600).collect::<Vec<_>>();
    chars.windows(3).map(|group|group.iter().copied().collect::<String>()).collect()
}
fn similarity(a:&HashSet<String>,b:&HashSet<String>)->f64{
    if a.is_empty()||b.is_empty(){return 0.0;}
    let common=a.intersection(b).count();
    common as f64/(a.len()+b.len()-common) as f64
}
fn domain(row:&Value)->String{
    let url=row["url"].as_str().unwrap_or("");
    let authority=url.split_once("://").map(|(_,rest)|rest).unwrap_or(url)
        .split(|ch:char|ch=='/'||ch=='?'||ch=='#').next().unwrap_or("");
    authority.trim_start_matches("www.").to_lowercase()
}

/// Ranked lists are returned by existing SQL indexes, not by extra LLM calls.
/// Reciprocal-rank fusion avoids comparing incomparable FTS/bigram raw scores.
/// Recent evidence is a weak recall channel; it cannot displace a strong match
/// merely because one publisher released more headlines.
pub fn select(rankings:&[(f64,Vec<Value>)],recent:&[Value],limit:usize)->Vec<Value>{
    let mut pool=HashMap::<String,(Value,f64)>::new();
    let mut add=|rows:&[Value],weight:f64|{
        if !weight.is_finite()||weight<=0.0{return;}
        for (index,fact) in rows.iter().take(120).enumerate(){
            let Some(id)=fact["id"].as_str().filter(|s|!s.is_empty())else{continue;};
            let score=weight/(RRF_K+index as f64+1.0);
            let entry=pool.entry(id.to_string()).or_insert_with(||(fact.clone(),0.0));
            entry.1+=score;
        }
    };
    for (weight,rows) in rankings.iter().take(4){add(rows,*weight);}
    add(recent,0.25);
    let mut rows=pool.into_values().map(|(fact,priority)|{
        Row{grams:grams(fact["claim"].as_str().unwrap_or("")),
            domain:domain(&fact),fact,priority}
    }).collect::<Vec<_>>();
    rows.sort_by(|a,b|a.fact["id"].as_str().cmp(&b.fact["id"].as_str()));
    let best=rows.iter().map(|r|r.priority).fold(0.0_f64,f64::max);
    if best<=0.0{return Vec::new();}
    let mut chosen=Vec::<usize>::new();
    let mut origins=HashMap::<String,usize>::new();
    while chosen.len()<limit.min(MAX_CONTEXT).min(rows.len()){
        let mut winner:Option<(usize,f64)>=None;
        for (i,row) in rows.iter().enumerate(){
            if chosen.contains(&i){continue;}
            let overlap=chosen.iter().map(|j|similarity(&row.grams,&rows[*j].grams))
                .fold(0.0_f64,f64::max);
            // Near-identical claims from different IDs must not become
            // "independent confirmations" just by changing the URL.
            if overlap>=0.84{continue;}
            let source_repeats=origins.get(&row.domain).copied().unwrap_or(0).min(4);
            let model_penalty=if row.fact["verification"].as_str().unwrap_or("")
                .starts_with("model_claim"){0.04}else{0.0};
            let score=0.74*(row.priority/best)-0.26*overlap
                -0.11*source_repeats as f64-model_penalty;
            if winner.is_none_or(|(_,current)|score>current+1e-12){
                winner=Some((i,score));
            }
        }
        let Some((index,_))=winner else{break;};
        *origins.entry(rows[index].domain.clone()).or_insert(0)+=1;
        chosen.push(index);
    }
    chosen.into_iter().map(|i|rows[i].fact.clone()).collect()
}
#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn fact(id:&str,domain:&str,claim:&str)->Value{
        json!({"id":id,"url":format!("https://{domain}/{id}"),
            "claim":claim,"verification":"observed_headline_not_verified_claim"})
    }
    #[test] fn complementary_index_results_get_fused_without_new_model_calls(){
        let a=fact("a","supply.example","Robot sensor fabrication expense reduction");
        let b=fact("b","buyer.example","Hospital equipment procurement cost increase");
        let selected=select(&[(3.0,vec![a.clone(),b.clone()]),(2.0,vec![b.clone()])],&[],2);
        assert_eq!(selected.len(),2);
        assert_eq!(selected[0]["id"],"b");
        assert_eq!(selected[1]["id"],"a");
    }
    #[test] fn cross_publisher_recall_displaces_same_publisher_monopoly(){
        let a=fact("a","one.example","Energy production input prices fall sharply");
        let b=fact("b","one.example","Construction steel prices rise in winter");
        let c=fact("c","two.example","Battery manufacturers reduce shipping expenses");
        let selected=select(&[(3.0,vec![a,b,c])],&[],3);
        assert_eq!(selected.len(),3);
        assert_eq!(selected[1]["id"],"c");
    }
    #[test] fn repeats_do_not_masquerade_as_independent_sources(){
        let a=fact("a","one.example","The exact same headline and article copy");
        let b=fact("b","two.example","The exact same headline and article copy");
        let c=fact("c","three.example","A distinct logistics supply problem");
        let result=select(&[(3.0,vec![a,b,c])],&[],22);
        assert_eq!(result.len(),2);
        assert!(result.iter().any(|r|r["id"]=="c"));
    }
    #[test] fn recent_fallback_preserves_labels_and_output_limit(){
        let rows=(0..40).map(|i|fact(&format!("r{i}"),"feed.example",
            &format!("Industry {i} has a different procurement bottleneck")))
            .collect::<Vec<_>>();
        let out=select(&[],&rows,22);
        assert!(!out.is_empty());
        assert!(out.len()<=22);
        assert_eq!(out[0]["verification"],"observed_headline_not_verified_claim");
    }
    #[test] fn empty_context_and_model_claims_are_not_promoted_to_verified(){
        assert!(select(&[],&[],22).is_empty());
        let row=json!({"id":"hypothesis","url":"https://example.net/test",
            "claim":"Estimated future sales","verification":"model_claim_unverified"});
        let out=select(&[(1.0,vec![row])],&[],22);
        assert_eq!(out[0]["verification"],"model_claim_unverified");
    }
}
