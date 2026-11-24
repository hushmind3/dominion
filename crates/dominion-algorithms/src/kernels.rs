//! Storage-independent decision kernels; the product host persists evidence.
use serde_json::Value;

pub const KINDS: &[&str] = &[
    "learning.applicability",
    "strategy.selection",
    "program.repair",
    "context.selection",
    "verification.predicate",
    "tool.selection",
    "tool.discovery",
    "tool.combination",
    "tool.arguments",
    "task.decomposition",
    "execution.budget",
    "memory.retrieval",
    "memory.rerank",
    "memory.importance",
    "learning.counterexample",
    "recovery.strategy",
    "multiagent.sharing",
    "multiagent.roles",
    "multiagent.conflict",
    "execution.ordering",
    "execution.parallelism",
];

/// Concrete kernels the Rust harness can execute. Registering a name that is
/// not here is rejected: an algorithm record must point at real behavior.
pub fn executable_kernels(kind: &str) -> &'static [&'static str] {
    match kind {
        "learning.applicability" => &["conservative", "probe-unknown"],
        "strategy.selection" => &["estimated-softmax", "exploit-only", "expression"],
        "program.repair" => &["completed-preserving", "append-only"],
        "context.selection" => &["failure-first"],
        "verification.predicate" => &["three-valued", "strict"],
        "tool.selection" => &["read-before-write", "block-write-without-read", "none"],
        "tool.discovery" => &["all", "core", "query-relevant"],
        "tool.combination" => &["sequential", "parallel-reads"],
        "tool.arguments" => &["none", "normalize-paths"],
        "task.decomposition" => &["acceptance-lineage-dag-2026"],
        "execution.budget" => &["standard", "extended", "lean"],
        "memory.retrieval" => &["lexical", "failure-first", "balanced"],
        "memory.rerank" => &["evidence-lineage-2026"],
        "memory.importance" => &["recency-uses", "fixed", "evidence-weighted"],
        "learning.counterexample" => &["narrow", "hold", "disable"],
        "recovery.strategy" => &["fingerprint", "block-identical", "allow-all"],
        "multiagent.sharing" => &["two-agents", "three-agents", "never"],
        "multiagent.roles" => &["discussion", "round-robin", "mentioned"],
        "multiagent.conflict" => &["evidence-first", "owner-authority", "latest-speaker"],
        "execution.ordering" => &["as-written", "read-first"],
        "execution.parallelism" => &["concurrent", "sequential"],
        _ => &[],
    }
}

pub fn kernel_exists(kind: &str, implementation: &str) -> bool {
    executable_kernels(kind)
        .iter()
        .any(|kernel| *kernel == implementation)
}

pub fn expression_sample(_kind: &str) -> std::collections::BTreeMap<String, f64> {
    // The expression interpreter is restricted to registered executable kernels.
    let names = [
        "discrimination", "progress", "cost", "uncertainty", "historical",
        "unresolved", "precondition", "benefit", "similarity", "risk",
    ];
    names.iter().map(|name| (name.to_string(),1.0)).collect()
}

pub fn kind_label(kind: &str) -> &str {
    match kind {
        "learning.applicability" => "경험 적용 조건",
        "strategy.selection" => "전략 선택",
        "program.repair" => "프로그램 수리",
        "context.selection" => "컨텍스트 선택",
        "verification.predicate" => "검증 predicate",
        "tool.selection" => "도구 선택",
        "tool.discovery" => "도구 탐색",
        "tool.combination" => "도구 조합",
        "tool.arguments" => "도구 인자 정규화",
        "task.decomposition" => "작업 분해",
        "execution.budget" => "실행 예산",
        "memory.retrieval" => "기억 검색",
        "memory.rerank" => "기억 재랭킹",
        "memory.importance" => "기억 중요도 평가",
        "learning.counterexample" => "반례 처리",
        "recovery.strategy" => "복구 전략",
        "multiagent.sharing" => "에이전트 지식 공유",
        "multiagent.roles" => "에이전트 역할·발언 방식",
        "multiagent.conflict" => "멀티에이전트 충돌 해결",
        "execution.ordering" => "도구 실행 순서",
        "execution.parallelism" => "병렬 실행",
        _ => "기타",
    }
}

/// Safe arithmetic expression over named candidate features. This lets a new
/// selection rule be added as an executable algorithm record without a Rust
/// rebuild, while the harness still owns the candidate features.
pub fn eval_expression(
    expression: &str,
    variables: &std::collections::BTreeMap<String, f64>,
) -> Result<f64, String> {
    struct Parser<'a> {
        chars: Vec<char>,
        index: usize,
        variables: &'a std::collections::BTreeMap<String, f64>,
    }
    impl<'a> Parser<'a> {
        fn skip(&mut self) {
            while self.chars.get(self.index).is_some_and(|ch| ch.is_whitespace()) {
                self.index += 1;
            }
        }
        fn peek(&mut self) -> Option<char> {
            self.skip();
            self.chars.get(self.index).copied()
        }
        fn next(&mut self) -> Option<char> {
            self.skip();
            let value = self.chars.get(self.index).copied();
            if value.is_some() {
                self.index += 1;
            }
            value
        }
        fn expr(&mut self) -> Result<f64, String> {
            let mut value = self.term()?;
            while let Some(op @ ('+' | '-')) = self.peek() {
                self.next();
                let rhs = self.term()?;
                value = if op == '+' { value + rhs } else { value - rhs };
            }
            Ok(value)
        }
        fn term(&mut self) -> Result<f64, String> {
            let mut value = self.factor()?;
            while let Some(op @ ('*' | '/')) = self.peek() {
                self.next();
                let rhs = self.factor()?;
                if op == '*' {
                    value *= rhs;
                } else {
                    if rhs.abs() < f64::EPSILON {
                        return Err("division by zero".into());
                    }
                    value /= rhs;
                }
            }
            Ok(value)
        }
        fn factor(&mut self) -> Result<f64, String> {
            match self.next() {
                Some('+') => self.factor(),
                Some('-') => Ok(-self.factor()?),
                Some('(') => {
                    let value = self.expr()?;
                    if self.next() != Some(')') {
                        return Err("missing closing parenthesis".into());
                    }
                    Ok(value)
                }
                Some(ch) if ch.is_ascii_digit() || ch == '.' => {
                    let mut text = ch.to_string();
                    while let Some(next) = self.peek() {
                        if next.is_ascii_digit() || next == '.' {
                            text.push(next);
                            self.next();
                        } else {
                            break;
                        }
                    }
                    text.parse::<f64>().map_err(|_| "invalid number".into())
                }
                Some(ch) if ch.is_alphabetic() || ch == '_' => {
                    let mut name = ch.to_string();
                    while let Some(next) = self.peek() {
                        if next.is_alphanumeric() || next == '_' {
                            name.push(next);
                            self.next();
                        } else {
                            break;
                        }
                    }
                    self.variables
                        .get(&name)
                        .copied()
                        .ok_or_else(|| format!("unknown variable {name}"))
                }
                other => Err(format!("unexpected token {other:?}")),
            }
        }
    }
    if expression.trim().is_empty() {
        return Err("expression is empty".into());
    }
    // Generated arithmetic must be bounded, finite and free of stack-exhaustion
    // inputs. It cannot call arbitrary Rust functions or external tools.
    if expression.chars().count()>512 {
        return Err("expression exceeds 512 characters".into());
    }
    let mut nesting=0usize;
    let mut depth=0usize;
    for ch in expression.chars() {
        if ch=='(' {
            nesting+=1;
            depth=depth.max(nesting);
            if depth>32 {return Err("expression nesting exceeds 32".into());}
        } else if ch==')' {
            nesting=nesting.saturating_sub(1);
        }
    }
    let mut parser = Parser {
        chars: expression.chars().collect(),
        index: 0,
        variables,
    };
    let value = parser.expr()?;
    if parser.peek().is_some() {
        return Err("trailing tokens in expression".into());
    }
    if !value.is_finite() {
        return Err("expression produced a non-finite score".into());
    }
    Ok(value)
}


/// Preserve explicit owner activation over validated and builtin alternatives.
pub fn select_kernel(records:Vec<Value>,kind:&str)->Option<Value>{
    if !KINDS.contains(&kind){return None;}
    records.into_iter()
        .filter(|algorithm| {
            algorithm["kind"].as_str() == Some(kind)
                && (matches!(algorithm["status"].as_str(),
                    Some("active" | "validated"))
                    || (algorithm["status"] == "available"
                        && algorithm["source"] == "builtin"))
                && algorithm["implementation"].as_str()
                    .is_some_and(|implementation| kernel_exists(kind, implementation))
        })
        .max_by_key(|algorithm| match algorithm["status"].as_str() {
            Some("active") => 3_u8,
            Some("validated") => 2,
            _ => 1,
        })
}

pub fn usage_rating(usage: &Value, task_outcome: &str) -> Option<(bool, &'static str)> {
    // Some paths only observe exposure, not whether the decision helped. They
    // force `unrated` even when the surrounding task succeeded or failed.
    if usage["forceUnrated"].as_bool() == Some(true)
        || usage["kind"].as_str() == Some("memory.retrieval")
    {
        return None;
    }
    if usage["signal"].is_object() {
        let successes = usage["signal"]["successes"].as_u64().unwrap_or(0);
        let failures = usage["signal"]["failures"].as_u64().unwrap_or(0);
        if successes > 0 || failures > 0 {
            return Some((
                (successes > 0 && failures == 0) || successes > failures,
                "domain_signal",
            ));
        }
    }
    match task_outcome {
        "succeeded" => Some((true, "task_outcome")),
        "failed" => Some((false, "task_outcome")),
        _ => None,
    }
}


#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn active_owner_kernel_wins_over_candidate_statistics(){
        let records=vec![
            json!({"kind":"strategy.selection","status":"validated","implementation":"exploit-only","successRate":1.0}),
            json!({"kind":"strategy.selection","status":"active","implementation":"estimated-softmax","successRate":0.0}),
            json!({"kind":"strategy.selection","status":"active","implementation":"unknown"}),
        ];
        assert_eq!(select_kernel(records,"strategy.selection").unwrap()["implementation"],"estimated-softmax");
    }
    #[test]
    fn only_verified_outcomes_or_domain_signals_are_rated(){
        assert_eq!(usage_rating(&json!({}),"unverified"),None);
        assert_eq!(usage_rating(&json!({"kind":"memory.retrieval"}),"succeeded"),None);
        assert_eq!(usage_rating(&json!({"signal":{"successes":0,"failures":1}}),"succeeded"),Some((false,"domain_signal")));
    }
    #[test]
    fn bounded_expression_is_executable_without_product_host(){
        let variables=std::collections::BTreeMap::from([("cost".into(),2.0),("benefit".into(),5.0)]);
        assert_eq!(eval_expression("benefit * 2 - cost",&variables).unwrap(),8.0);
        assert!(eval_expression("benefit / 0",&variables).is_err());
    }
}
