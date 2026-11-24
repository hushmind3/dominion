//! Default planning and budget selection without any host dependency.
use serde_json::Value;
use dominion_core::strategy::TurnLimits;

/// Legacy default planning heuristic, now owned by the replaceable
/// strategy layer rather than the native execution/finalization code.
pub fn needs_plan(input:&str,acceptance:&Option<(String,Value)>)->bool{
    let text=input.trim();
    if text.chars().count()>240 || text.contains('\n'){return true;}
    if ["그리고","그 다음","다음으로","한 뒤","후에","then","after",";"]
        .iter().any(|marker|text.contains(marker)){return true;}
    match acceptance {
        Some((_,checks))=>checks.as_array()
            .map(|items|items.is_empty()).unwrap_or(true),
        None=>true,
    }
}


pub fn execution_limits(chosen:Option<&Value>,default_rounds:usize,default_tools:usize)->TurnLimits{
    TurnLimits{
        max_rounds:chosen.and_then(|a|a["parameters"]["maxRounds"].as_u64())
            .unwrap_or(default_rounds as u64).clamp(2,24) as usize,
        max_tools:chosen.and_then(|a|a["parameters"]["maxTools"].as_u64())
            .unwrap_or(default_tools as u64).clamp(4,120) as usize,
    }
}
#[cfg(test)]
mod tests{
 use super::*;
 use serde_json::json;
 #[test]
 fn budget_preferences_are_bounded(){
  assert_eq!(execution_limits(Some(&json!({"parameters":{"maxRounds":999,"maxTools":0}})),8,40),TurnLimits{max_rounds:24,max_tools:4});
  assert_eq!(execution_limits(None,8,40),TurnLimits{max_rounds:8,max_tools:40});
 }
}
