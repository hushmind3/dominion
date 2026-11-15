//! DOMINION-owned on-demand discovery over the *permission-scoped* catalog.
//! Search reveals a schema for a later provider round. It cannot execute
//! a tool, install an MCP server, or bypass round-scoped authorization.
use super::*;
use std::collections::{BTreeSet,HashSet};

pub(super) const TOOL_SEARCH_NAME:&str="dominion_tool_search";

/// Collapse equivalent model search prompts before another tool catalog scan.
/// This is a pure deterministic key; never used for granting permissions.
pub(super) fn canonical_query(query:&str)->String {
    query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}
const MAX_REVEALED:usize=20;
const MAX_MATCHES:usize=6;

fn schema_bytes(v:&Value)->usize {
    serde_json::to_string(v).map(|raw|raw.len()).unwrap_or(usize::MAX)
}
fn search_definition()->Value {
    json!({"type":"function","function":{
        "name":TOOL_SEARCH_NAME,
        "description":"Search currently authorized DOMINION native and MCP tools by capability. Results make matching tools callable in your NEXT model turn; search never performs a tool operation or grants permissions. Do not repeat a query with no results.",
        "parameters":{"type":"object","properties":{
            "query":{"type":"string","description":"Action or service to discover, e.g. github pull request, browser click, read file, edit code."},
            "limit":{"type":"integer","minimum":1,"maximum":6}},
            "required":["query"],"additionalProperties":false}
    }})
}

/// Recompute from today's authorized catalog. A persisted name never grants
/// a tool that has since been disabled or removed by an owner.
pub(super) fn restore_revealed(state:&AppState,execution_id:&str,
    catalog:&[Value])->BTreeSet<String>{
    let granted=catalog.iter().map(tool_name).collect::<HashSet<_>>();
    let Some(execution)=state.store.get("execution",execution_id)else{
        return BTreeSet::new()
    };
    execution["revealedToolNames"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).filter(|name|granted.contains(name))
        .take(MAX_REVEALED).map(str::to_string).collect()
}

/// A newly discovered tool can be shown only after a search call completes.
/// Current-model-turn tools remain the definitive execution allowlist.
pub(super) fn menu(catalog:&[Value],initial:&[Value],budget:usize,
    query:&str,recent:&HashSet<String>,revealed:&BTreeSet<String>,
    search_enabled:bool)->Vec<Value>{
    if catalog.is_empty(){return Vec::new()}
    let initial_names=initial.iter().map(tool_name).collect::<HashSet<_>>();
    let hidden=catalog.iter().any(|definition|
        !initial_names.contains(tool_name(definition)));
    let all_bytes=initial.iter().map(schema_bytes)
        .try_fold(0usize,|sum,size|sum.checked_add(size));
    if !hidden && all_bytes.is_some_and(|n|n<=budget){
        return initial.to_vec();
    }
    let search=search_definition();
    let search_bytes=if search_enabled {schema_bytes(&search)}else{0};
    if search_enabled&&search_bytes>=budget{
        return budgeted_tool_definitions(initial.to_vec(),budget,query,recent);
    }
    // Reserve bytes for explicitly discovered schemas first. Sorting the
    // candidates is insufficient because a secondary ranker may evict them.
    // Never advertise a tool absent from today's permission-scoped catalog.
    let mut capacity=budget-search_bytes;
    let mut selected=Vec::<Value>::new();
    let mut ordinary=Vec::<Value>::new();
    for definition in catalog {
        let name=tool_name(definition);
        if name.is_empty(){continue;}
        if revealed.contains(name) {
            let size=schema_bytes(definition);
            if size<=capacity {
                capacity-=size;
                selected.push(definition.clone());
            }
        }else if initial_names.contains(name) {
            ordinary.push(definition.clone());
        }
    }
    selected.extend(budgeted_tool_definitions(ordinary,capacity,query,recent));
    if search_enabled {selected.push(search);}
    selected
}

fn words(input:&str)->BTreeSet<String>{
    input.to_lowercase().split(|ch:char|!ch.is_alphanumeric())
        .filter(|word|word.chars().count()>=2).take(80)
        .map(str::to_string).collect()
}

fn aliases(query:&str)->Vec<&'static str>{
    let q=query.to_lowercase();
    let mut out=Vec::new();
    const GROUPS:&[(&[&str],&[&str])]=&[
        (&["깃허브","커밋","리포","풀리퀘"],&["github","git","repo","pull"]),
        (&["파일","문서","폴더","소스"],&["workspace","file","read"]),
        (&["수정","편집","패치","고쳐"],&["replace","write","edit"]),
        (&["검색","찾아","조회"],&["search","fetch","list"]),
        (&["브라우저","클릭"],&["browser","control"]),
        (&["영상","장면","렌더"],&["video","render"]),
        (&["빌드","테스트","실행"],&["shell","terminal","exec"]),
        (&["위임","협업","에이전트"],&["delegate","task"]),
    ];
    for (terms,words) in GROUPS{
        if terms.iter().any(|term|q.contains(term)){out.extend_from_slice(words);}
    }
    out
}

/// Only the already authorized catalog is searched. Search results are
/// candidate names/descriptions, not execution receipts.
pub(super) fn search(catalog:&[Value],query:&str,limit:usize,
    revealed:&BTreeSet<String>)->(Vec<Value>,Vec<String>){
    let q=query.trim();
    if q.is_empty()||q.chars().count()>240{return (vec![],vec![])}
    let tokens=words(q);
    let aliases=aliases(q);
    let mut matched=catalog.iter().enumerate()
        .filter(|(_,definition)|tool_name(definition)!=TOOL_SEARCH_NAME)
        .filter_map(|(i,definition)|{
            let name=tool_name(definition).to_lowercase();
            if name.is_empty(){return None}
            let description=definition["function"]["description"].as_str()
                .or_else(||definition["description"].as_str()).unwrap_or("");
            let title=words(&name.replace('_'," "));
            let all=words(&format!("{name} {description}"));
            let mut score=0i32;
            if q.eq_ignore_ascii_case(&name){score+=200}
            if name.contains(&q.to_lowercase()){score+=80}
            for term in &tokens{
                if title.contains(term){score+=15}
                else if all.contains(term){score+=3}
            }
            for term in &aliases{
                if title.contains(*term){score+=12}
                else if all.contains(*term){score+=2}
            }
            (score>0).then_some((i,score))
        }).collect::<Vec<_>>();
    matched.sort_by(|(i,a),(j,b)|
        revealed.contains(tool_name(&catalog[*i]))
            .cmp(&revealed.contains(tool_name(&catalog[*j])))
            .then_with(||b.cmp(a)).then_with(||i.cmp(j)));
    let selected=matched.into_iter().take(limit.clamp(1,MAX_MATCHES))
        .map(|(i,_)|&catalog[i]).collect::<Vec<_>>();
    let names=selected.iter().map(|item|tool_name(item).to_owned())
        .collect::<Vec<_>>();
    let results=selected.into_iter().map(|item|{
        let description=item["function"]["description"].as_str()
            .or_else(||item["description"].as_str()).unwrap_or("");
        json!({"name":tool_name(item),
            "description":description.chars().take(140).collect::<String>(),
            "alreadyRevealed":revealed.contains(tool_name(item))})
    }).collect();
    (results,names)
}

#[cfg(test)]
mod tests{
    use super::*;
    fn def(name:&str,pad:usize)->Value{
        json!({"type":"function","function":{"name":name,
            "description":"x".repeat(pad),"parameters":{"type":"object","properties":{}}}})
    }
    #[test]
    fn missing_mcp_tool_requires_search_before_next_turn(){
        let catalog=vec![def("workspace_read",10),def("mcp__github__create_pr",50)];
        let initial=vec![catalog[0].clone()];
        let first=menu(&catalog,&initial,2048,"read file",
            &HashSet::new(),&BTreeSet::new(),true);
        assert!(first.iter().any(|row|tool_name(row)==TOOL_SEARCH_NAME));
        assert!(!first.iter().any(|row|tool_name(row)=="mcp__github__create_pr"));
        let (results,names)=search(&catalog,"깃허브 github pull",3,&BTreeSet::new());
        assert!(!results.is_empty());
        assert!(names.contains(&"mcp__github__create_pr".to_string()));
        let revealed=BTreeSet::from(["mcp__github__create_pr".to_string()]);
        let next=menu(&catalog,&initial,2048,"read file",
            &HashSet::new(),&revealed,true);
        assert!(next.iter().any(|row|tool_name(row)=="mcp__github__create_pr"));
    }
    #[test]
    fn search_cannot_grant_tools_absent_from_permission_catalog(){
        let allowed=vec![def("workspace_read",10)];
        let (results,names)=search(&allowed,"github delete repository",6,&BTreeSet::new());
        assert!(results.is_empty());
        assert!(names.is_empty());
    }
    #[test]
    fn tiny_provider_budget_never_overflows_with_discovery(){
        let catalog=vec![def("workspace_read",50),def("hidden_mcp",1000)];
        let offered=menu(&catalog,&catalog[..1],128,"read file",
            &HashSet::new(),&BTreeSet::new(),true);
        assert!(offered.iter().map(schema_bytes).sum::<usize>()<=128);
    }
    #[test]
    fn revealed_tool_survives_ranking_and_tight_schema_budget(){
        let expensive=def("workspace_read",1200);
        let discovered=def("mcp__github__create_pull_request",40);
        let catalog=vec![expensive.clone(),discovered.clone()];
        let budget=schema_bytes(&discovered)+schema_bytes(&search_definition())+4;
        let revealed=BTreeSet::from(["mcp__github__create_pull_request".to_owned()]);
        let next=menu(&catalog,&[expensive],budget,"read files",
            &HashSet::new(),&revealed,true);
        assert!(next.iter().any(|tool|tool_name(tool)==
            "mcp__github__create_pull_request"));
        assert!(next.iter().map(schema_bytes).sum::<usize>()<=budget);
        let exhausted=menu(&catalog,&[],budget,"read files",
            &HashSet::new(),&revealed,false);
        assert!(exhausted.iter().any(|tool|tool_name(tool)==
            "mcp__github__create_pull_request"));
        assert!(!exhausted.iter().any(|tool|tool_name(tool)==TOOL_SEARCH_NAME));
    }
    #[test]
    fn equivalent_discovery_queries_share_one_cache_key(){
        assert_eq!(canonical_query("  GitHub   Pull   Request "),
            canonical_query("github pull request"));
        assert_eq!(canonical_query(" 파일   수정 "),
            canonical_query("파일 수정"));
        assert_ne!(canonical_query("web search"),canonical_query("file search"));
    }
    #[test]
    fn full_small_catalog_does_not_waste_search_schema(){
        let catalog=vec![def("workspace_read",10),def("workspace_diff",10)];
        let selected=menu(&catalog,&catalog,2048,"inspect code",
            &HashSet::new(),&BTreeSet::new(),true);
        assert_eq!(selected,catalog);
    }
}
