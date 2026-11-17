//! room context benchmark integration scenarios, using the shared real-binary fixture.
use super::*;

/// Research measurement, not a behavioural guarantee: shows exactly what the
/// harness sends per participant across three owner turns in a nine-person room.
#[test]
fn measure_room_input_growth_across_three_turns() {
    let _serial = serial();
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reply_counter = counter.clone();
    let (port, requests) = scripted_provider(move |_| {
        let n = reply_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        vec![json!({
            "choices":[{"delta":{"content":format!("응답 {n}")},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":100,"completion_tokens":5,"total_tokens":105}
        })]
    });
    let root = temp_root();
    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let persona = "지침 문장입니다. ".repeat(220);
    let mut ids = Vec::new();
    for index in 1..=9 {
        let agent = app.post("/api/agents", &json!({
            "id":format!("m{index}"),
            "name":format!("참가자{index}"),
            "systemPrompt":format!("{persona}나는 참가자{index}입니다.")
        }));
        ids.push(agent["id"].as_str().unwrap().to_string());
    }
    app.post("/api/rooms", &json!({
        "id":"measure-room","name":"측정 방","agentIds":ids,
        "routingMode":"discussion","rounds":1,"maxTurns":10,
        "toolsEnabled":false,"autoLearning":false,"contextBudgetTokens":2000
    }));
    let mut seen = 0usize;
    for (round, text) in ["안녕", "오늘 기분은 어때", "점심은 뭐 먹을까"].iter().enumerate() {
        let sent = app.post("/api/rooms/measure-room/messages", &json!({"input":text}));
        let job_id = sent["jobId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            let job = app.get(&format!("/api/jobs/{job_id}"));
            if job["status"] == "completed" { break; }
            assert_ne!(job["status"], "failed", "{job}");
            assert!(Instant::now() < deadline, "timeout: {job}");
            std::thread::sleep(Duration::from_millis(30));
        }
        let bodies = requests.lock().unwrap();
        let fresh: Vec<Value> = bodies[seen..].to_vec();
        seen = bodies.len();
        drop(bodies);
        let mut total = 0usize;
        for (index, body) in fresh.iter().enumerate() {
            let tokens = measure_estimate(&body["messages"].to_string());
            total += tokens;
            let prompt = body["messages"][0]["content"].as_str().unwrap_or("");
            let name = prompt.split("활성 에이전트: ").nth(1)
                .and_then(|rest| rest.split(" (id:").next()).unwrap_or("?");
            println!("round {} call {} agent={} estimate={} items={}", round + 1, index + 1, name, tokens, body["messages"].as_array().map(Vec::len).unwrap_or(0));
        }
        println!("round {} summary calls={} total_estimate={}", round + 1, fresh.len(), total);
    }
    let db = rusqlite::Connection::open_with_flags(
        root.join("runtime/dominion.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ).unwrap();
    let mut statement = db.prepare("select data from records where kind='participant_conversation'").unwrap();
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
    for row in rows {
        let data: Value = serde_json::from_str(&row.unwrap()).unwrap();
        let view = &data["workingView"];
        let frames = view["frames"].as_array().cloned().unwrap_or_default();
        let mut contents = std::collections::HashMap::new();
        for frame in &frames {
            let text = frame["message"]["content"].as_str().unwrap_or("").to_string();
            if !text.is_empty() { *contents.entry(text).or_insert(0usize) += 1; }
        }
        let duplicates = contents.values().filter(|count| **count > 1).count();
        println!(
            "view agent={} epoch={} frames={} duplicate_contents={} estimate={}",
            data["agentId"].as_str().unwrap_or("?"),
            view["epoch"], frames.len(), duplicates,
            measure_estimate(&json!(frames).to_string())
        );
    }
}


