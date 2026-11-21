//! Image attachments: the declared-vision route receives the real bytes, and
//! an endpoint that never declared vision is refused before any model call.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture-image";

fn upload(app: &TestApp, session_id: &Value) -> Value {
    app.post(
        "/api/attachments",
        &json!({
            "sessionId": session_id,
            "name": "pixel.png",
            "mimeType": "image/png",
            "data": STANDARD.encode(PNG),
        }),
    )
}

#[test]
fn declared_vision_sends_the_image_to_the_provider() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({
            "choices":[{"delta":{"content":"이미지를 확인했습니다."},"finish_reason":"stop"}],
            "usage":{"total_tokens":20}
        })]
    });
    let app = TestApp::new(port, "native");
    let provider = app
        .get("/api/providers")
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .expect("the fixture registers one provider");
    app.post(
        "/api/providers",
        &json!({
            "id":provider["id"],"name":provider["name"],"baseUrl":provider["baseUrl"],
            "kind":"openai","model":"mock","enabled":true,
            "capabilities":{"vision":true},
        }),
    );
    let session = app.post("/api/sessions", &json!({"title":"vision"}));
    let attachment = upload(&app, &session["id"]);
    assert_eq!(attachment["isImage"], true, "{attachment}");

    let stream = app.chat(&json!({
        "sessionId":session["id"],"input":"이미지를 설명해줘",
        "attachmentIds":[attachment["id"]],"autoLearning":false,
    }));
    assert!(stream.contains("이미지를 확인했습니다."), "{stream}");
    let captured = requests.lock().unwrap();
    assert!(
        captured.iter().any(|body| body.to_string().contains("data:image/png;base64,")),
        "the provider never received the image bytes"
    );
}

#[test]
fn undeclared_vision_refuses_the_image_before_calling_the_provider() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({
            "choices":[{"delta":{"content":"호출되면 안 됩니다."},"finish_reason":"stop"}]
        })]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"no vision"}));
    let attachment = upload(&app, &session["id"]);

    let stream = app.chat(&json!({
        "sessionId":session["id"],"input":"이미지를 설명해줘",
        "attachmentIds":[attachment["id"]],"autoLearning":false,
    }));
    assert!(
        stream.contains("이미지"),
        "the refusal must explain the missing declaration: {stream}"
    );
    assert!(
        requests.lock().unwrap().is_empty(),
        "a text-only endpoint must not receive the image"
    );
}
