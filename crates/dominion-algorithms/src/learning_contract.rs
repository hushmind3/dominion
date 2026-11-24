//! Decode bounded candidate proposals only. Host-side provenance checks and
//! persistence are required before any candidate can become learned knowledge.
use serde_json::Value;
pub struct LearnedMemory {
    pub text: String,
    pub kind: String,
    pub importance: f64,
    pub key: Option<String>,
    pub source_ids: Vec<String>,
    pub quote: String,
}
pub struct LearnedSkill {
    pub name: String,
    pub instructions: String,
    pub source_ids: Vec<String>,
}
pub struct LearnedStrategy {
    pub text: String,
    pub rationale: String,
    pub scope: String,
    pub source_ids: Vec<String>,
    pub verification: Value,
}

pub struct LearnedToolPolicy {
    pub tool: String,
    pub text: String,
    pub rationale: String,
    pub scope: String,
    pub source_ids: Vec<String>,
}

pub struct Learning {
    pub memories: Vec<LearnedMemory>,
    pub skill: Option<LearnedSkill>,
    pub strategies: Vec<LearnedStrategy>,
    pub tool_policies: Vec<LearnedToolPolicy>,
    pub harness: Option<Value>,
}

fn source_ids(value: &Value) -> Vec<String> {
    value["sourceMessageIds"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|id| id.as_str().map(str::to_string))
                .take(8)
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_learning(text: &str) -> Result<Learning, String> {
    let start = text
        .find('{')
        .ok_or("학습 모델이 JSON을 반환하지 않았습니다.")?;
    let end = text
        .rfind('}')
        .filter(|end| *end >= start)
        .ok_or("학습 JSON이 완전하지 않습니다.")?;
    let value: Value =
        serde_json::from_str(&text[start..=end]).map_err(|_| "학습 JSON을 해석할 수 없습니다.")?;
    let items = value["memories"]
        .as_array()
        .ok_or("학습 JSON에 memories 배열이 필요합니다.")?;
    let memories = items
        .iter()
        .filter_map(|item| {
            Some(LearnedMemory {
                text: item["text"].as_str()?.chars().take(1601).collect(),
                kind: item["type"].as_str().unwrap_or("fact").into(),
                importance: item["importance"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0),
                key: item["key"]
                    .as_str()
                    .map(|key| key.chars().take(120).collect()),
                source_ids: source_ids(item),
                quote: item["evidenceQuote"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(1600)
                    .collect(),
            })
        })
        .take(8)
        .collect();
    let skill = if value["skill"].is_object() {
        let item = &value["skill"];
        Some(LearnedSkill {
            name: item["name"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect(),
            instructions: item["instructions"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(12000)
                .collect(),
            source_ids: source_ids(item),
        })
    } else {
        None
    };
    let strategies = value["strategies"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(LearnedStrategy {
                        text: item["text"].as_str()?.chars().take(2000).collect(),
                        rationale: item["rationale"].as_str().unwrap_or("").chars().take(1000).collect(),
                        scope: if item["scope"].as_str() == Some("agent") { "agent".into() } else { "shared".into() },
                        source_ids: source_ids(item),
                        verification: item["verification"].clone(),
                    })
                })
                .take(4)
                .collect()
        })
        .unwrap_or_default();
    let tool_policies = value["toolPolicies"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(LearnedToolPolicy {
                        tool: item["tool"].as_str()?.chars().take(80).collect(),
                        text: item["text"].as_str()?.chars().take(2000).collect(),
                        rationale: item["rationale"].as_str().unwrap_or("").chars().take(1000).collect(),
                        scope: if item["scope"].as_str() == Some("agent") { "agent".into() } else { "shared".into() },
                        source_ids: source_ids(item),
                    })
                })
                .take(4)
                .collect()
        })
        .unwrap_or_default();
    let harness = value["harnessProposal"]
        .is_object()
        .then(|| value["harnessProposal"].clone());
    Ok(Learning { memories, skill, strategies, tool_policies, harness })
}


#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn parser_never_expands_candidate_limits(){
        let rows=vec![serde_json::json!({"text":"fact","importance":2.0,"sourceMessageIds":["a"]});12];
        let value=serde_json::json!({"memories":rows});
        let learned=parse_learning(&value.to_string()).unwrap();
        assert_eq!(learned.memories.len(),8);
        assert_eq!(learned.memories[0].importance,1.0);
        assert_eq!(learned.memories[0].source_ids,vec!["a".to_string()]);
        assert!(parse_learning("not json").is_err());
    }
}
