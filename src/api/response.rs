use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub(super) fn prompt_error(
    value: crate::features::collaboration::prompt_settings::PromptError,
) -> Response {
    use crate::features::collaboration::prompt_settings::PromptError;
    let (status,message)=match value{
        PromptError::Invalid(message)=>(StatusCode::BAD_REQUEST,message),
        PromptError::Missing=>(StatusCode::NOT_FOUND,"수정할 AI 또는 그룹을 찾을 수 없습니다.".into()),
        PromptError::Changed=>(StatusCode::CONFLICT,"다른 곳에서 프롬프트나 참가자가 변경되었습니다. 작성한 내용은 유지하고 원본을 다시 확인하세요.".into()),
        PromptError::Storage(message)=>{
            eprintln!("Prompt settings storage: {message}");
            (StatusCode::INTERNAL_SERVER_ERROR,"프롬프트 변경을 저장할 수 없습니다.".into())
        },
    };
    error(status, &message)
}

pub(super) fn error(status: StatusCode, message: &str) -> Response {
    // Every API error body is read by the owner, so upstream text is turned
    // into a sentence here instead of at each handler.
    let code = match status {
        StatusCode::BAD_REQUEST => "invalid_request",
        StatusCode::UNAUTHORIZED => "authentication_required",
        StatusCode::FORBIDDEN => "permission_denied",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        StatusCode::SERVICE_UNAVAILABLE => "service_unavailable",
        _ => "request_failed",
    };
    let retryable = matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    );
    (
        status,
        Json(json!({"error":crate::plain::error(message),"code":code,"retryable":retryable})),
    )
        .into_response()
}
