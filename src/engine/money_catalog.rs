//! Public GitHub / Hugging Face technology catalog snapshots.
//! This is zero-LLM-token collection. Stars/downloads are technology attention
//! indicators, not verified business demand, customers or revenues.
use crate::{engine::jobs::JobContext,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

fn digest(value:&str)->String{format!("{:x}",Sha256::digest(value.as_bytes()))}
fn text(v:&Value,key:&str,max:usize)->String{
    v[key].as_str().unwrap_or("").chars().take(max).collect()
}
fn metric(entity:&str,metric:&str,value:u64,url:&str,time:&str)->Value{
    let day=time.chars().take(10).collect::<String>();
    json!({"id":format!("metric:{}",digest(&format!("{entity}:{metric}:{day}"))),
        "entityId":entity,"metric":metric,"value":value,"url":url,
        "observedAt":time,"metricStatus":"platform_counter_not_customer_sales"})
}
fn github(input:&Value,time:&str)->(Vec<Value>,Vec<Value>){
    let mut signal=Vec::new();
    let mut metrics=Vec::new();
    for repo in input["items"].as_array().into_iter().flatten().take(25){
        let full=text(repo,"full_name",190);
        let url=text(repo,"html_url",1000);
        let description=text(repo,"description",500);
        if full.is_empty()||!url.starts_with("https://github.com/"){continue;}
        let published=text(repo,"created_at",80);
        let title=format!("New repository: {full}");
        let mut item=json!({"id":digest(&url),"title":title,
            "description":description,"sourceName":"GitHub emerging repositories",
            "url":url,"publishedAt":published,"capturedAt":time,
            "contentStatus":"platform_api_metadata","articleExcerpt":"",
            "verification":"platform_listing_not_commercial_demand"});
        item=crate::engine::money_intelligence::enrich_signal(item);
        if chrono::DateTime::parse_from_rfc3339(&published).ok().is_some_and(|d|
            d.with_timezone(&chrono::Utc)>=chrono::Utc::now()-chrono::Duration::days(30)){
            item["changeTags"]=json!(["launch"]);
        }
        if let Some(stars)=repo["stargazers_count"].as_u64(){
            metrics.push(metric(&format!("github:{full}"),"stars",stars,&url,time));
        }
        signal.push(item);
    }
    (signal,metrics)
}
fn huggingface(input:&Value,time:&str)->(Vec<Value>,Vec<Value>){
    let mut signal=Vec::new();
    let mut metrics=Vec::new();
    for model in input.as_array().into_iter().flatten().take(25){
        let name=text(model,"id",190);
        if name.is_empty()||name.contains(".."){continue;}
        let url=format!("https://huggingface.co/{name}");
        let task=text(model,"pipeline_tag",120);
        let title=format!("Open model: {name}");
        let desc=format!("Hugging Face model listing; task={task}");
        let mut item=json!({"id":digest(&url),"title":title,"description":desc,
            "sourceName":"Hugging Face trending models","url":url,
            "publishedAt":text(model,"createdAt",80),"capturedAt":time,
            "contentStatus":"platform_api_metadata","articleExcerpt":"",
            "verification":"platform_listing_not_commercial_demand"});
        item=crate::engine::money_intelligence::enrich_signal(item);
        if let Some(downloads)=model["downloads"].as_u64(){
            metrics.push(metric(&format!("hf:{name}"),"rolling_downloads",downloads,&url,time));
        }
        if let Some(likes)=model["likes"].as_u64(){
            metrics.push(metric(&format!("hf:{name}"),"likes",likes,&url,time));
        }
        signal.push(item);
    }
    (signal,metrics)
}
pub async fn run(context:&JobContext,_job:&Value)->Result<Value,String>{
    crate::engine::money_tool::ensure_live(context)?;
    let at=chrono::Utc::now();
    let start=(at-chrono::Duration::days(10)).format("%Y-%m-%d").to_string();
    let urls=[
        ("github",format!("https://api.github.com/search/repositories?q=created:%3E%3D{start}+stars:%3E%3D20&sort=stars&order=desc&per_page=25")),
        ("huggingface","https://huggingface.co/api/models?sort=trendingScore&limit=25".into()),
    ];
    let mut signals=Vec::new();
    let mut metrics=Vec::new();
    let mut errors=Vec::new();
    for (kind,url) in urls {
        let body=match crate::engine::tools::http_fetch(&url).await{
            Ok(value) if value["status"].as_u64()==Some(200)=>value,
            Ok(value)=>{errors.push(format!("{kind}: HTTP {}",value["status"]));continue;},
            Err(error)=>{errors.push(format!("{kind}: {}",error.chars().take(140).collect::<String>()));continue;}
        };
        let parsed=match serde_json::from_str::<Value>(body["text"].as_str().unwrap_or("")){
            Ok(parsed)=>parsed,
            Err(err)=>{errors.push(format!("{kind}: non-JSON response: {err}"));continue;}
        };
        let (mut items,mut counters)=if kind=="github"{github(&parsed,&at.to_rfc3339())}
            else{huggingface(&parsed,&at.to_rfc3339())};
        signals.append(&mut items);
        metrics.append(&mut counters);
    }
    let imported=context.state.store.money_ingest_signals(&signals)?;
    let new_metrics=context.state.store.money_store_market_metrics(&metrics)?;
    let recent=context.state.store.money_recent_signals(120)?;
    let new_tensions=context.state.store.money_store_tensions(
        &crate::engine::money_mechanism::discover(&recent,30))?;
    let result=json!({"id":format!("catalog:{}",context.job_id),
        "jobId":context.job_id,"importedSignals":imported,
        "newMetricSnapshots":new_metrics,"crossMarketHypotheses":new_tensions,
        "catalogItemCount":signals.len(),"counters":metrics.len(),
        "errors":errors,"capturedAt":at.to_rfc3339()});
    context.state.store.put("money_catalog_report",&result)
        .ok_or("tech catalog report save failed")?;
    Ok(result)
}
pub fn schedule(state:&AppState){
    let bucket=chrono::Utc::now().timestamp().div_euclid(21600);
    let id=format!("money:catalog:{bucket}");
    if state.store.get("job",&id).is_none(){
        let _=crate::engine::jobs::enqueue_once(state,&id,"money_catalog",
            json!({"bucket":bucket,"source":"public_github_huggingface"}));
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn github_repository_is_observation_not_a_buyer(){
        let input=json!({"items":[{"full_name":"org/tool","html_url":"https://github.com/org/tool",
            "description":"Cheaper sensor","stargazers_count":125,"created_at":"2026-11-16T00:00:00Z"}]});
        let (signals,metrics)=github(&input,"2026-11-16T12:00:00Z");
        assert_eq!(signals.len(),1);
        assert_eq!(metrics.len(),1);
        assert_eq!(metrics[0]["value"],125);
        assert_eq!(signals[0]["verification"],"platform_listing_not_commercial_demand");
    }
    #[test] fn trending_downloads_are_not_revenue(){
        let (signals,metrics)=huggingface(&json!([{"id":"org/model","downloads":1234,"likes":12}]),"2026-11-16T12:00:00Z");
        assert_eq!(signals.len(),1);
        assert_eq!(metrics[0]["metricStatus"],"platform_counter_not_customer_sales");
    }
}
