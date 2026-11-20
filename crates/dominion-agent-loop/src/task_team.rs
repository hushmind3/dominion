//! Pure bounded task dependency and exclusive-write planner.
//! No provider, tool, approval, database or runtime authority lives here.
use serde_json::Value;
use std::collections::{BTreeSet,HashMap};

#[derive(Debug,Clone,PartialEq,Eq)]
pub struct Schedule {
    pub waves:Vec<Vec<usize>>,
    pub has_dependencies:bool,
    pub has_writers:bool,
}
impl Schedule {
    pub fn ordered(&self)->Vec<usize>{
        self.waves.iter().flatten().copied().collect()
    }
    pub fn requires_serial(&self)->bool{
        self.has_dependencies || self.has_writers
    }
}
fn writes(node:&Value)->bool{
    match node["type"].as_str().unwrap_or(""){
        "delegate"|"model_decision"|"loop"=>true,
        "call_tool"=>!matches!(node["tool"].as_str(),
            Some("file_read"|"workspace_read"|"workspace_list"|
                "workspace_search"|"workspace_stat"|"code_search"|
                "code_index_search")),
        "observe"|"verify"=>false,
        "sequence"|"parallel"=>node["nodes"].as_array()
            .is_none_or(|xs|xs.iter().any(writes)),
        "branch"=>node["then"].as_array()
            .is_none_or(|xs|xs.iter().any(writes))
            ||node["else"].as_array().is_none_or(|xs|xs.iter().any(writes)),
        _=>true,
    }
}
/// Dependency IDs are resolved within one team wave. Unknown or cyclic edges
/// fail before any tool is admitted. A writer/delegate receives an exclusive
/// wave; a model-supplied readOnly label never grants concurrency.
pub fn schedule(nodes:&[Value])->Result<Schedule,String>{
    if nodes.is_empty()||nodes.len()>64{return Err("team requires 1..64 nodes".into());}
    let mut ids=HashMap::<&str,usize>::new();
    for (i,n) in nodes.iter().enumerate(){
        if let Some(id)=n["id"].as_str().filter(|s|!s.is_empty()){
            if ids.insert(id,i).is_some(){return Err(format!("duplicate node: {id}"));}
        }
    }
    let mut deps=vec![BTreeSet::<usize>::new();nodes.len()];
    let mut has_dependencies=false;
    for (i,n) in nodes.iter().enumerate(){
        if n.get("dependsOn").is_some() && !n["dependsOn"].is_array(){
            return Err(format!("node {i}: dependsOn must be an array"));
        }
        if let Some(raw)=n["dependsOn"].as_array(){
            if raw.len()>64{return Err("too many dependencies".into());}
            for item in raw{
                let id=item.as_str().filter(|s|!s.is_empty())
                    .ok_or("team dependency must be a node ID")?;
                let d=*ids.get(id).ok_or_else(||format!("unknown dependency: {id}"))?;
                if d==i{return Err("self dependency".into());}
                deps[i].insert(d);
                has_dependencies=true;
            }
        }
    }
    let has_writers=nodes.iter().any(writes);
    let mut done=BTreeSet::new();
    let mut waves=Vec::new();
    while done.len()<nodes.len(){
        let ready=(0..nodes.len()).filter(|i|!done.contains(i)
            &&deps[*i].iter().all(|d|done.contains(d))).collect::<Vec<_>>();
        if ready.is_empty(){return Err("cyclic team dependencies".into());}
        let wave=if writes(&nodes[ready[0]]){vec![ready[0]]}
            else{ready.into_iter().filter(|i|!writes(&nodes[*i])).collect()};
        for i in &wave{done.insert(*i);}
        waves.push(wave);
    }
    Ok(Schedule{waves,has_dependencies,has_writers})
}
#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn independent_reads_are_concurrent_and_writer_depends_on_all_previous_reads(){
        let plan=vec![
            json!({"id":"a","type":"call_tool","tool":"file_read"}),
            json!({"id":"b","type":"call_tool","tool":"workspace_read"}),
            json!({"id":"c","type":"delegate","dependsOn":["a","b"]}),
            json!({"id":"d","type":"call_tool","tool":"workspace_write","dependsOn":["c"]}),
        ];
        assert_eq!(schedule(&plan).unwrap().waves,vec![vec![0,1],vec![2],vec![3]]);
        assert!(schedule(&plan).unwrap().requires_serial());
    }
    #[test]
    fn cycle_unknown_and_custom_mcp_mutations_fail_closed(){
        assert!(schedule(&[json!({"id":"a","dependsOn":["missing"]})]).is_err());
        assert!(schedule(&[
            json!({"id":"a","dependsOn":["b"]}),
            json!({"id":"b","dependsOn":["a"]}),
        ]).unwrap_err().contains("cyclic"));
        assert_eq!(schedule(&[
            json!({"id":"a","type":"call_tool","tool":"mcp__unknown__read"}),
            json!({"id":"b","type":"delegate"}),
        ]).unwrap().waves,vec![vec![0],vec![1]]);
    }
}
