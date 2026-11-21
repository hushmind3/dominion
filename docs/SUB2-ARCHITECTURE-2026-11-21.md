# DOMINION sub2 — 실행 아키텍처 정본 (2026-11-21)

> 범위: `dominion-sub2` 브랜치에서 **실제 소스와 저장소 계약**에 맞게 정리한 구조입니다. 다른 브랜치를 변경하거나 새 UI 메뉴·별도 런타임을 만드는 설계가 아닙니다. 본 문서가 기능의 실기기·실모델 운영 검증까지 보증하지는 않습니다.

## 1. 결정 — 1개 실행 경로, 3개 책임 계층

```mermaid
flowchart TD
    CLIENT[React/Vite · Tauri 2 · TypeScript SDK] --> API[Axum API / SSE]
    API --> ORCH[Chat · Room · Job · Task Team 조정기]
    ORCH --> HOST[Product Host Adapters]
    HOST --> HARNESS[dominion-turn-core: 단일 Harness + Driver + RunState]
    POLICY[dominion-agent-loop / dominion-agent-kernel: 교체 가능한 전략] --> HARNESS
    HOST --> MODELS[Model Gateway: 로컬/API 모델]
    HOST --> TOOLS[Tool Gateway: 승인·실행·효과 영수증]
    HOST --> BRIDGE[External Agent Bridge: Codex/Claude/Qwen]
    TOOLS --> ADDONS[MCP / 브라우저 / Skills / 채널]
    ORCH --> SQLITE[(SQLite 실행 기록·이벤트·체크포인트)]
    TOOLS --> SQLITE
    HARNESS --> HOST
    SQLITE --> API
```

**Core와 Strategy를 물리적으로 합치지 않습니다.** `dominion-turn-core`는 요청/라운드/도구 배치 승인/공통 상태 의미를 소유하고, 전략은 원하는 행동을 제안합니다. 실제 파일 변경·토큰 비용·상호작용 권한·저장소 접근은 제품 Host가 소유합니다. 같은 Rust 제품 바이너리 안에서 조합하며 별도 마이크로서비스는 필수가 아닙니다.

| 책임 | 실제 소스 | 교체 여부 |
|---|---|---|
| 순수 실행 Core / Harness | `crates/dominion-turn-core/src/{agent_loop,driver,run_state}.rs` | 필수 |
| 선택적 실행 전략 | `crates/dominion-agent-loop/src/lib.rs`, `crates/dominion-agent-kernel/src/decision_policy.rs` | 교체 가능 |
| 전략 설정·활성 정책 | `src/engine/services/decision/turn_policy.rs` | 설정 가능 |
| 제품 조립 / 실행 포트 | `src/engine/mod.rs`, `src/engine/services/conversation/chat/` | Rust 호스트 |
| 모델 게이트웨이 | `src/engine/services/providers/gateway.rs` | 모델/엔드포인트 어댑터 |
| 실제 도구 게이트웨이 | `src/engine/services/tools/tools.rs`, `src/engine/services/operations/effect.rs` | 정책·실행 책임 필수 |
| 선택적 외부 연동 | `src/engine/addons/`, `src/api/addons.rs` | 선택적 |
| 영속 상태/이벤트 | `src/db.rs`, `src/events.rs` | 단일 사실 원천 |
| 단말 및 SDK | `web/`, `src-tauri/`, `sdk/typescript/` | 동일 Axum HTTP/SSE 사용 |

## 2. 단일 실행 계약

1. **Ingress**: 사용자/SDK/채널 요청은 제품 Host에서 run/session/owner identity를 검증합니다.
2. **Strategy**: 계획·컨텍스트·모델 선택은 정책 계층에서 결정하며 도구 권한을 직접 발급하지 않습니다.
3. **Model**: `Harness::request_model`이 Driver의 라운드 제한을 집행합니다. Gateway는 로컬/API 모델별 기능 불지원과 대체 경로를 별도 판단합니다.
4. **Proposal**: `Harness::prepare_response`는 모델이 실제 응답한 도구 ID와 호출 수를 동결합니다. 빈 ID는 호스트에서 한 번만 부여하며 중복 ID는 거부합니다.
5. **Admission**: `Harness::admit_prepared`가 **현재 공개한 도구 목록**, 도구 활성화, 요청별 호출 한도를 배치 전체에 적용합니다.
6. **Effect**: 승인·workspace lock·SQLite Writer lease를 거친 도구 실행만 실제 파일/외부 효과를 수행합니다. 영수증은 `execution_operation`에 저장합니다.
7. **Observation**: 도구의 성공·실패·결과 불명을 구분합니다. 불명확한 변경 결과는 완료로 승격하거나 자동 중복 실행하지 않습니다.
8. **Finalization**: 검증된 실행 결과와 최종 답변은 Host의 저장 절차를 거쳐 기존 Run/Event 계약으로 투영합니다.

`dominion-agent-loop::run`은 독립 모델/도구 루프가 아니라 `dominion-turn-core::agent_loop::Harness::drive_ports`를 호출하는 SDK 호환 포트입니다. 제품 Chat은 같은 Harness의 라운드/승인 단계를 사용하되 스트리밍·DB/도구 스케줄링은 Host 어댑터에 둡니다. ProgramIR의 순수 도구 DAG/검증 절차는 별도 제품 서비스이며 **모델 제공자 실행 루프를 새로 만들면 안 됩니다.**

## 3. Room Chat과 Task Team은 한 가지 기능이 아닙니다

실제 참가자별 지속 읽기 커서의 소유 코드는 `src/engine/services/conversation/room_state.rs`이며, 방의 작업 관측판은 `src/engine/services/conversation/room_workboard.rs`입니다.

| 시스템 | 소유 상태 | 동작 |
|---|---|---|
| Room Chat | `rooms.rs`, `room_state.rs`, `room_workboard.rs` | 공통 원문·참가자별 읽기 커서·발언자 라우팅·협업 관측 |
| 병렬 Room 조사 | `room_parallel.rs` | 읽기 전용으로 제한한 독립 참가자 세션, 최대 4개 동시 모델 호출 |
| Task Team | `task_team.rs`, `runtime_program.rs`, `task_team_work.rs` | `dependsOn` DAG, 준비된 읽기 파동 병렬화, Writer/Delegate 독점 실행, 작업 증거 병합 |

그룹 메시지와 Task Team 작업 모두 같은 Chat/Harness와 영속 이벤트를 사용하지만, 발언권/읽기 커서와 작업 DAG/파일 변경 임대는 별개의 조정기에서 소유합니다.

**Task Team 불변조건 (2026-11-21 강화):** Worker는 원본 `executionId`, `sessionId`, `programRevision`, `teamNodeId`, `nodeId`, `agentId`에 귀속됩니다. SQLite IMMEDIATE 트랜잭션에서 **부모 실행이 살아 있는지 + 자식 상태가 기대값인지** 함께 확인한 후에만 재배정·선점·결과 확정이 가능합니다. 취소/중단된 부모에 대한 늦은 성공 영수증으로 자식을 완료 처리하지 않습니다. `onFailure: continue`로 실패가 관측된 경우는 계속할 수 있으나 그 실패를 성공 검증으로 기록하지 않습니다.

## 4. 도구와 Workspace Writer 단일 경계

- `workspace_write`, `workspace_replace`, `workspace_replace_range`, `workspace_apply_patch`, `shell_exec`, `terminal_exec`, `external_harness_run`은 동일 작업 공간의 변경 가능 자원으로 취급합니다.
- `file_write`의 **절대 경로가 해당 작업 공간 안으로 정규화되면** 똑같은 workspace Writer 잠금/SQLite 임대를 받습니다. 외부 절대 경로는 별도 외부 Writer 범위에 귀속합니다. 해석 불가 경로도 락 없이 진행시키지 않습니다.
- 모델의 `readOnly` 선언은 권한 증거가 아니며, Task Team 병렬화는 네이티브 `safe_to_overlap` 목록에만 의존합니다.
- 외부 Codex/Claude/Qwen 실행기는 Model Gateway의 `ModelPort`가 아니라 별도 Worker/Bridge입니다. 외부 런타임의 내부 도구 권한을 DOMINION이 모두 소유한다고 가정하지 않습니다.

## 5. Run/Event/SDK 공통 의미

- Run 상태: `queued`, `running`, `completed`, `failed`, `cancelled`, `interrupted`, `needs_review`, `skipped`.
- SQLite `event_log`가 단조 증가 `seq`를 관리합니다. 실제 Run 기록에서 만든 `run_state` 버전 1 이벤트는 `eventSeq == seq`이며 `runId`, `runKind`, nullable `sourceMessageId`, nullable `agentId`를 포함합니다.
- Task Team Worker 이벤트에는 `parentRunId`·`teamNodeId`·`nodeId`·`dependsOn`·`evidenceStatus`를 추가합니다. TypeScript SDK `runEvents()`는 동일 SSE 스트림을 읽고 계약 버전/커서를 검사합니다.
- 실행 `phase`는 Core에서 정의한 상태 전이만 허용합니다. 알 수 없는 phase 문자열을 임의의 실패/성공 영수증으로 덮어쓰지 않습니다.

## 6. 2026년 공개 시스템과 설계 연결

- [OpenAI Codex App Server](https://openai.com/index/unlocking-the-codex-harness/): 실행 주체와 UI 프로토콜·thread 상태를 분리. DOMINION은 Axum/SSE와 Rust Core를 유지합니다.
- [Qwen Code 2026-10-05 multi-agent](https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-05-session-multi-agent.md): 별도 에이전트 세션·영속 메시지·실행 소유권을 명시. DOMINION은 Room Cursor와 Task Team 결과를 분리합니다.
- [Hermes Bot Mode](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/bot-mode.md): 대화방/게이트웨이/개별 Bot 영속 세션. DOMINION은 그룹 발언과 실제 도구 변경의 소유권을 구별합니다.
- [AgentScope v2 release notes](https://github.com/agentscope-ai-java/agentscope-java/blob/master/docs/v2/en/docs/others/release-notes.md): HarnessAgent의 middleware/인프라 계층 분리. DOMINION은 필수 Core와 선택적 서비스/전략 경계를 고정합니다.

## 7. 구현 검증 대 검증 예정

**소스와 CI 검증 범위:** `cargo test --locked -p dominion-turn-core --lib`, `cargo test --locked -p dominion-agent-loop --lib`, `cargo test --lib engine::task_team_work::tests`, `cargo test --lib engine::resource_lock::tests`, `node --test scripts/sub2-architecture-contract.test.mjs`, `node --test scripts/sub2-core-boundary.test.mjs`, TypeScript 검사. CI는 전체 릴리스/설치 프로그램 빌드를 대신하지 않습니다.

**실환경에서 별도 검증할 항목:** 강제 종료 후 실제 외부 변경의 중복 방지, 100턴 이상 실모델 Room 품질·원가, Codex/Claude/Qwen 실연결·승인 전달, Windows/macOS Tauri 설치, 실제 사용자 계정·결제, 다중 프로세스 환경에서 외부 파일/셸의 모든 잠재적 변경 자원. 이 목록은 소스 미구현을 뜻하는 것이 아니라 **통합/운영 완성의 검증 경계**입니다.

원칙: 새 메뉴·새 HTTP 서버·새 병렬 모델 루프를 만들지 말고, 먼저 기존 실행 계약을 재사용하십시오.
