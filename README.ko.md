# DOMINION · 도미니온

<p align="center">
  <img src="src-tauri/icons/icon.png" width="88" alt="DOMINION 아이콘" />
</p>
<p align="center"><strong>모델은 직접 선택하고, 에이전트 실행과 데이터는 직접 소유합니다.</strong></p>
<p align="center">
  <a href="README.md">English</a> · <a href="README.ko.md">한국어</a> ·
  <a href="ENGINE.md">엔진 구조</a> · <a href="sdk/typescript/README.md">TypeScript SDK</a>
</p>
<p align="center">
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue" /></a>
  <img alt="Rust 엔진" src="https://img.shields.io/badge/engine-Rust-262626" />
  <img alt="React UI" src="https://img.shields.io/badge/UI-React-149ECA" />
  <a href="https://github.com/hushmind3/dominion-rust/actions/workflows/ci.yml?query=branch%3Adominion-sub2"><img alt="서브2 개발 테스트" src="https://img.shields.io/github/actions/workflow/status/hushmind3/dominion-rust/ci.yml?branch=dominion-sub2&label=sub2%20dev%20tests" /></a>
</p>

**DOMINION은 로컬 우선·오픈소스 AI 에이전트 작업 공간입니다.** Rust 엔진 1개가 모델 호출, 도구 실행, 승인, 취소, 영속 작업, 검증과 복구를 담당합니다. React 화면, HTTP API, Rust SDK, TypeScript SDK는 **같은 실행기를 사용하는 인터페이스**입니다.

**이 문서는 2026년 11월 19일 기준 실험 브랜치 `dominion-sub2`의 소스 설명입니다.** 기본 `main` 브랜치 설명이나 상용 배포 완성 보증이 아닙니다. 별도 토큰 재판매 서비스 없이 사용자가 보유한 모델·API·로컬 추론 환경을 연결합니다.

[빠른 설치](#-빠른-시작) · [기능](#-주요-기능) · [구조](#-실행-구조와-소스-경로) · [SDK/API](#-sdk와-api) · [검증과 한계](#-검증-상태와-알려진-한계)

## 📸 화면 미리보기

**여러 에이전트가 참가하는 그룹 대화**

![멀티 에이전트 방](docs/01-multi-agent-room.webp)

| 일반 채팅 | 에이전트 설정 |
| --- | --- |
| ![채팅 작업 공간](docs/02-chat-workspace.webp) | ![에이전트 스튜디오](docs/03-agent-studio.webp) |

*개발 빌드의 실제 화면입니다. 화면이 존재한다는 사실은 실모델 작업 완수율을 입증하지 않습니다.*

## ✨ 주요 기능

| 기능 | 실제 구현 범위 | 핵심 소스 |
| --- | --- | --- |
| **BYOK·로컬 모델** | 사용자가 등록한 API 모델과 GGUF/llama.cpp·Ollama 등 호환 로컬 추론 연결 | `src/engine/models.rs`, `chat/provider.rs` |
| **일반 채팅** | 실시간 스트리밍, 도구 호출, 권한 승인, 중단 버튼, SQLite 이력 | `web/src/pages/Chat.tsx`, `src/api/chat.rs`, `src/engine/chat/` |
| **자율 작업** | 사용자 목표, 실행·검증·재시도, 체크포인트, `needs_review`, 목표 변경·취소·복구 | `src/engine/jobs.rs`, `autonomy.rs`, `verification.rs` |
| **멀티에이전트 대화** | 개별 페르소나와 공동 사용자 확정 상태, 선별된 동료 발언, 참가자별 연속성 | `src/engine/rooms.rs`, `chat/room_context.rs` |
| **컨텍스트 비용 절감** | 공통 고정 프리픽스, 최신 파일 읽기의 예산 내 1회 원문 전달과 이후 압축, 맥락 선별, 캐시 계측, Codex WebSocket 증분 전송 | `chat/context.rs`, `trace_budget.rs`, `continuity.rs`, `codex_ws.rs` |
| **도구·MCP·스킬** | Rust 도구 권한 검사, 필요한 도구 정의 발견, 스킬 목록 선제시 및 `skill_read` 지연 로딩 | `src/engine/tools.rs`, `chat/capability.rs` |
| **머니 조사** | 원문·출처·가격/수요 신호·가설·보류 후보 보존, 사용자 검토·선택 | `src/engine/money/`, `src/api/money/` |
| **외부 연결** | Telegram·Discord·GitHub·MCP 등 사용자가 설정한 인증/권한으로 연결 | `src/engine/telegram.rs`, `discord.rs`, `mcp.rs` |
| **SDK** | Rust에 직접 내장하는 실행 인터페이스 및 TypeScript HTTP 클라이언트 | `src/sdk.rs`, `sdk/typescript/` |

**대화 연속성 원자적 갱신(서브2):** 모델 제공자 경로와 참가자별 최종 응답 포인터를 SQLite 트랜잭션 1개에서 조건부로 동시 변경합니다. 늦게 도착한 동시 실행 응답이나 변경 전 페르소나의 응답은 연속성 포인터를 부분적으로 덮어쓰지 못합니다. 동일 경로·다른 경로·프리픽스 변경에 대한 회귀 테스트를 추가했습니다. 소스: `src/engine/chat/continuity.rs`, `src/db.rs`. 실모델 성능 향상을 주장하는 변경은 아닙니다.

**2026-11-19 서브2 방 캐시 개선:** 사용자 메시지 ID 대신 내용 기반 상태 식별자로 동일한 방 상태를 재사용하고, 응답 현황이 변경되면 이전 상태를 교체합니다. 원본 발언과 출처 ID는 SQLite에 보존합니다. 예시 문장 속 참가자 이름은 호출로 해석하지 않으며, 직접 호출은 `@이름` 또는 명시적인 호칭·대상 조사를 사용합니다. 이번 변경의 실모델 토큰 절감량은 별도 재측정 대상입니다.

**현재 제품 방향:** 서브2에서는 과거 내장 영상·음성 제작 파이프라인과 독립 RSI 세대 반복·자동 코어 변경 기능을 퇴역시켰습니다. 필요한 생성 기능은 외부 MCP/Connections로 호출합니다. 기존 대화·작업·설정·기억·이미 생성한 미디어 기록은 보존합니다.

## ⚡ 빠른 시작

**macOS·Windows 소스 설치**를 기준으로 합니다. Git, Rust stable/Cargo, Node.js **24.12 이상**, OS별 C/C++ 개발 도구가 필요합니다. Windows에서는 MSVC Build Tools와 Windows SDK가 필요합니다.

```bash
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
git switch dominion-sub2
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home .
```

실행 후 **http://127.0.0.1:4317** 접속 → **Models**에서 모델/제공자 등록 → **Chat** 또는 **Home → Autonomous work**에서 작업을 시작합니다.

- 개발용 Rust 테스트 예시: `cargo test --lib engine::chat::continuity::tests`, `cargo test --lib engine::tools::tests`, `cargo test --lib sdk::tests`, `cargo test -p dominion-agent-kernel --lib`
- 프론트엔드 타입 검사는 `npm --prefix web run typecheck`
- Windows/macOS 설치 패키지 검증은 별도의 릴리즈 작업입니다. 개발 CI 통과가 설치 파일의 정상 작동을 보장하지는 않습니다.

Rust가 실제 서버와 실행기를 구동하며, Node.js는 React 화면의 빌드에 사용합니다. 기본 서버는 로컬 주소에 바인딩합니다. `DOMINION_DATA`(기본 `runtime/`), `DOMINION_WORKSPACE`, `DOMINION_HOME`, `DOMINION_WEB`, `DOMINION_PORT`로 저장 위치·작업 폴더·포트를 지정할 수 있습니다.

[설치 상세](docs/GETTING-STARTED.md) · [데스크톱 패키징](docs/DESKTOP.md)

## 🧱 실행 구조와 소스 경로

```text
 React / Vite 화면 ────────────┐
 TypeScript @dominion/sdk ────┼──► Axum /api/* ────────────┐
 외부 API 클라이언트 ────────────┘                          │
 Rust 애플리케이션 ───────────────► src/sdk.rs ─────────────┤
                                                            ▼
                                          Rust 채팅/잡/그룹 실행기
                                          ├─ 판단·예산 커널
                                          ├─ 도구 권한 → 승인 → 실행
                                          ├─ 검증 → 영수증 → 작업 상태
                                          ├─ 기억·컨텍스트·스킬·MCP
                                          └─ 취소·복구·이벤트 버스
                                             │           │
                                             ▼           ▼
                                           SQLite      모델 API /
                                                       로컬 추론
```

| 담당 | 코드 위치 | 책임 |
| --- | --- | --- |
| API | `src/api/mod.rs` | HTTP 엔드포인트 조합, 같은 Rust 작업 엔진에 전달 |
| Rust SDK·시작 | `src/sdk.rs`, `src/bootstrap.rs` | `Dominion::open/start/submit/steer/wait`; 시작에는 Tokio 런타임 필요 |
| 채팅·모델 | `src/engine/chat/runtime.rs`, `runtime/turn_body.rs`, `provider.rs` | 스트리밍, 도구 결과, 사용량 저장 |
| 자율 작업 | `src/engine/jobs.rs`, `autonomy.rs`, `crates/dominion-agent-kernel/` | 잡 소유권, 실행 판단, 검증·재시도 |
| 상태·복구 | `src/db.rs`, `src/state.rs`, `src/engine/effect.rs` | SQLite, 실행 시도/세대 보호, 외부 효과 검토 |
| 그룹 맥락 | `src/engine/rooms.rs`, `chat/room_context.rs`, `room_state.rs` | 공동 결정사항, 개별 기억과 메시지 선별 |
| 사용자 화면 | `web/src/pages/`, `web/src/components/NativeAgentPanel.tsx` | 대화·모델·작업·머니·연결 설정 |

예전 실험 자료는 `docs/`와 `benchmark/`에 남겨두되, **현재 작동하는 모듈과 과거 연구 기록은 구분**합니다.

## 🔌 SDK와 API

**Rust SDK는 별도 하네스 없이 엔진을 직접 사용하는 인터페이스**입니다.

```rust
use dominion_rust::sdk::{Dominion, EngineOptions, Prompt};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), String> {
    let sdk = Dominion::open(EngineOptions::new(".", "./runtime"))?;
    sdk.start()?;
    let job = sdk.submit(Prompt::new("소스 코드를 확인하고 수정 후 검증해"))?;
    let result = sdk.wait(job["id"].as_str().unwrap(), Duration::from_secs(120)).await?;
    println!("{}", result["status"]);
    Ok(())
}
```

[TypeScript SDK](sdk/typescript/README.md)는 같은 Rust 서버를 호출하는 선택형 클라이언트입니다. 패키지 이름은 `@dominion/sdk`이며, 자체 모델 실행기나 별도 에이전트를 설치하지 않습니다.

| 기능 | HTTP API |
| --- | --- |
| 채팅 스트리밍 | `POST /api/chat` |
| 현재 채팅 중단 | `POST /api/sessions/:id/cancel` + `{"requestId":"UUID"}` |
| 중단 완료 확인 | `GET /api/sessions/:id/cancel-status` |
| 작업 만들기·조회 | `POST /api/jobs`, `GET /api/jobs/:id` |
| 작업 취소·추가 지시 | `POST /api/jobs/:id/cancel`, `POST /api/jobs/:id/steer` |
| Home 자율 작업 | `/api/sdk/runs` 및 ID별 승인·취소·재개·외부 효과 확인 경로 |
| 도구 실행 증거 | `/api/executions/:id/operations` |

`POST /api/jobs`의 `input`은 선택적으로 `verifyCommand`, `successCriteria`, `tokenBudget`을 받습니다. 토큰 예산은 **1,024~2,000,000토큰의 모델 호출 사전 판단 제한**이며, 실제 과금의 엄격한 보증이 아닙니다. Rust SDK의 `Prompt` 구조체는 별도 명시 필드만 제공하므로 모든 HTTP 옵션을 동일한 Rust 필드로 오해하면 안 됩니다.

**중단 버튼**은 브라우저 연결만 닫지 않고, 서버에 요청별 취소를 기록하고 Rust 실행 상태가 종료될 때까지 기다린 뒤 같은 세션의 보내기 버튼을 다시 활성화하도록 구성했습니다. 원격 추론 서버가 연결 중단을 감지하기 전에 처리한 토큰이나 외부 시스템에 이미 적용된 변경 작업은 별도 확인이 필요합니다. [중단·캐시 설계](docs/SUB2-CHAT-STOP-CACHE-2026-11-18.md)

## 🧠 컨텍스트·기억·토큰 절감

모델에서 보고하는 사용량을 다음처럼 구분합니다.

1. **전체 입력 토큰**: 캐시된 부분까지 포함할 수 있는 Provider 사용량
2. **캐시 적중 토큰**: Provider가 실제로 보고한 값만 사용하며, 미제공은 0이 아닌 *미확인*
3. **비캐시 입력 토큰**: 정확한 전체·캐시 입력이 있을 때만 계산

**2026-11-18 캐시 접두부 구조 보강(서브2):** 이제 `세션 ID + 참가자 + 모델 제공자`별로 별도 캐시 키를 생성합니다. 서로 다른 방에서 같은 페르소나를 사용해도 캐시 경로가 충돌하지 않습니다. MCP 등록 순서가 달라져도 읽기·편집·테스트 도구의 스키마 배열을 같은 순서로 전송합니다. OpenCode Go의 `/zen/go`와 `/inference/go` 공식 주소를 모두 인식하고, 플래너·검토기·대화 압축에 사용되는 별도 모델 호출에도 일관된 `x-opencode-session` 헤더를 부여합니다. 실행 기록에는 **어떤 접두부가 변경됐는지**와 제공자가 보고한 실제 캐시 적중률을 구분해 남깁니다. HTTP 모델이 페르소나 4,000토큰을 전송·계산·과금 없이 생략한다고 주장하는 기능은 아닙니다. [구현·회귀 테스트](docs/SUB2-PREFIX-REUSE-HARNESS-2026-11-18.md)

Codex WebSocket은 서버가 이전 응답 이력을 보유한 경우에 증분 문맥을 활용합니다. **직전 요청에서 WebSocket 전송이 확인된 경우에만** 별도로 제한된 누적 이력 한도를 적용합니다. 소스 수정·권한 변경·컨텍스트 교체·연결 종료·HTTP 폴백에서는 전체 이력 재구성이 필요할 수 있습니다. **Cloudflare HTTP 및 로컬 추론에는 WebSocket 전용 이력 확대를 적용하지 않습니다.**

**9인 그룹대화 컨텍스트:** 최근 동료 발언은 원문 그대로 유지하고, 먼저 발언한 참가자의 최신 내용은 **메시지 ID가 있는 짧은 출처 색인**으로 전달합니다. 원본은 같은 방의 SQLite 대화 기록에 남아 있으며 모델이 새로운 사실로 요약해 만들어내지 않습니다. 인용·회상·원문 비교 요청에서는 도구 권한이 있을 때 `room_message_read({messageId,offset?})`로 원문을 6,000자씩 읽습니다. 다른 방의 메시지나 비공개 추론은 조회하지 못합니다. 색인 1개가 추가되더라도 전체 색인을 매번 다시 만들지 않고, 기존 원문이 모델의 유지된 문맥에 있으면 같은 내용의 짧은 색인도 중복 추가하지 않습니다. **원문 보존과 신규 토큰 절감은 별도로 측정**하며 실모델 절감률은 아직 새로 검증하지 않았습니다. [그룹대화 개선 근거](docs/SUB2-ROOM-DELTA-MEMORY-2026-11-18.md).

스킬은 이름·ID·설명을 먼저 보여주고, 필요할 때 `skill_read({skillId,offset?})`로 본문을 읽습니다. 긴 스킬은 `nextOffset`으로 끝까지 조회하며 승인되지 않은 스킬이나 다른 참가자의 비공개 스킬은 차단합니다. 도구가 꺼져 있는 턴에는 읽기 도구를 강요하지 않고 제한된 본문을 유지합니다.

**이전 실험과 현재 코드는 구분합니다.** [2026년 11월 9일 9인 그룹 100턴](benchmark/ROOM-100-TURN-LUNA-2026-11-09.json)은 입력 2,630,782토큰, 캐시 적중률 18.15%, 이력 한도 재구성 621회, 후반 회상 정확도 0/9를 기록했습니다. [11월 8일 증분 전송 실험](benchmark/CODEX-WEBSOCKET-INCREMENTAL-2026-11-08.json)은 3턴 전송 이력을 76.9% 줄였지만 실제 과금 절감은 안정적으로 입증하지 못했습니다. **이번 변경 이후의 100턴 재측정 결과는 아직 없습니다.**

## 🧪 검증 상태와 알려진 한계

- **개발 CI:** `dominion-sub2`의 [Rust 회귀 테스트 워크플로](.github/workflows/ci.yml)와 프론트엔드 TypeScript 타입 검사. 전체 릴리즈·운영체제별 패키징·실모델 완수율 검증과 구분합니다.
- **작업 판정:** 산출물 파일이 맞더라도 검증 근거나 외부 효과 상태가 미해결이면 `needs_review`가 남을 수 있습니다. 실제 `completed`와 파일 정답은 서로 다른 관측값입니다.
- **비용·제공자:** 모델별 캐시 지원, 실제 요금·속도·도구 호환성이 다릅니다. 자체 추정값을 공급자의 청구서라고 표시하지 않습니다.
- **실행 환경:** 사전 검증된 설치 패키지를 제공한다는 주장은 하지 않습니다. Telegram·Discord·MCP·로컬 모델은 개별 설정·권한·서버가 필요합니다.

관련 문서: [2026-11-18 소스 감사](docs/SUB2-SOURCE-AUDIT-2026-11-18.md) · [하네스 검증 결정](docs/SUB2-HARNESS-DECISION-REPAIR-2026-11-18.md) · [컨텍스트·캐시](docs/SUB2-HARNESS-CONTEXT-CACHE-2026-11-18.md) · [머니 원문 출처](docs/SUB2-MONEY-SOURCE-LINEAGE-2026-11-18.md)

## 🤝 개발 참여

[엔진 설명](ENGINE.md) · [전체 기술 문서 색인](docs/INDEX.md) · [영문 README](README.md)

버그 제보에는 **브랜치·커밋 SHA·모델/Provider·실제 작업 상태·재현 입력·가려진 실행 로그·검증 증거**를 포함하면 분석에 도움이 됩니다. API 키·OAuth 인증값·개인 대화 원문은 공개하지 마세요.

라이선스: [Apache-2.0](LICENSE). 
