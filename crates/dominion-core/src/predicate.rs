//! Shared evidence predicate contract. It evaluates supplied observations only;
//! it cannot observe tools, authorize an effect or certify a task by itself.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredicateVerdict {
    True,
    False,
    Unknown,
}

impl PredicateVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_true(self) -> bool {
        matches!(self, Self::True)
    }
}

fn predicate_observation(predicate: &Value, resolved: &Value) -> Result<Option<Value>, String> {
    let source = predicate["from"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("predicate.from is required")?;
    let Some(base) = resolved.get(source) else {
        return Ok(None);
    };
    if base.is_null() {
        return Ok(None);
    }
    match predicate["path"].as_str() {
        Some(path) => Ok(base.pointer(path).cloned()),
        None => Ok(Some(base.clone())),
    }
}

/// Three-valued predicate evaluation. Missing observation fields are Unknown,
/// not False, so they can never refute a hypothesis.
pub fn predicate_verdict(
    predicate: &Value,
    resolved: &Value,
) -> Result<PredicateVerdict, String> {
    match predicate["type"].as_str().unwrap_or("") {
        "exists" => {
            let source = predicate["from"].as_str().unwrap_or("");
            let Some(base) = resolved.get(source) else {
                // `exists` is a scope-complete check over the resolved map.
                return Ok(PredicateVerdict::False);
            };
            if base.is_null() {
                return Ok(PredicateVerdict::False);
            }
            match predicate["path"].as_str() {
                Some(path) => Ok(if base.pointer(path).is_some() {
                    PredicateVerdict::True
                } else {
                    PredicateVerdict::False
                }),
                None => Ok(PredicateVerdict::True),
            }
        }
        "equals" => {
            let Some(actual) = predicate_observation(predicate, resolved)? else {
                return Ok(PredicateVerdict::Unknown);
            };
            Ok(if actual == predicate["value"] {
                PredicateVerdict::True
            } else {
                PredicateVerdict::False
            })
        }
        "compare" => {
            let Some(actual) = predicate_observation(predicate, resolved)? else {
                return Ok(PredicateVerdict::Unknown);
            };
            let (Some(left), Some(right)) =
                (actual.as_f64(), predicate["value"].as_f64())
            else {
                return Ok(PredicateVerdict::Unknown);
            };
            let verdict = match predicate["op"].as_str().unwrap_or("") {
                "gt" => left > right,
                "gte" => left >= right,
                "lt" => left < right,
                "lte" => left <= right,
                other => return Err(format!("unsupported compare op: {other}")),
            };
            Ok(if verdict {
                PredicateVerdict::True
            } else {
                PredicateVerdict::False
            })
        }
        "not" => Ok(match predicate_verdict(&predicate["predicate"], resolved)? {
            PredicateVerdict::True => PredicateVerdict::False,
            PredicateVerdict::False => PredicateVerdict::True,
            PredicateVerdict::Unknown => PredicateVerdict::Unknown,
        }),
        "all" => {
            let mut unknown = false;
            for item in predicate["predicates"].as_array().into_iter().flatten() {
                match predicate_verdict(item, resolved)? {
                    PredicateVerdict::False => return Ok(PredicateVerdict::False),
                    PredicateVerdict::Unknown => unknown = true,
                    PredicateVerdict::True => {}
                }
            }
            Ok(if unknown {
                PredicateVerdict::Unknown
            } else {
                PredicateVerdict::True
            })
        }
        "any" => {
            let mut unknown = false;
            for item in predicate["predicates"].as_array().into_iter().flatten() {
                match predicate_verdict(item, resolved)? {
                    PredicateVerdict::True => return Ok(PredicateVerdict::True),
                    PredicateVerdict::Unknown => unknown = true,
                    PredicateVerdict::False => {}
                }
            }
            Ok(if unknown {
                PredicateVerdict::Unknown
            } else {
                PredicateVerdict::False
            })
        }
        other => Err(format!("unsupported predicate type: {other}")),
    }
}

/// Boolean convenience for callers that can treat Unknown as non-satisfied.
/// Branch/Verify must use `predicate_verdict` so Unknown is not silently False.
pub fn predicate_holds(predicate: &Value, resolved: &Value) -> Result<bool, String> {
    Ok(predicate_verdict(predicate, resolved)?.is_true())
}
#[cfg(test)]
mod tests{
 use super::*;
 use serde_json::json;
 #[test]
 fn missing_values_stay_unknown_in_composite_predicates(){
   let observation=json!({"result":{}});
   let unknown=json!({"type":"equals","from":"result","path":"/missing","value":true});
   assert_eq!(predicate_verdict(&unknown,&observation).unwrap(),PredicateVerdict::Unknown);
   assert_eq!(predicate_verdict(&json!({"type":"not","predicate":unknown}),&observation).unwrap(),PredicateVerdict::Unknown);
 }
}
