# 도미니언 — Rust 자율 에이전트 플랫폼

도미니언은 **사용자가 요청한 일을 끝내는 Rust 실행 플랫폼**입니다. 모델·공급자에 종속되지 않고 코드 수정, 파일 작업, 검색, 브라우저, 그룹 협업, 도구 위임, 테스트·검증, 승인·취소·복구를 실행합니다.

## 제품 범위

**유지:** 일반 채팅과 스트리밍, 기억, 파일, 검증된 절차와 실패 기록, Rust 하네스, TypeScript/Rust SDK, 장기 작업, MCP, 플러그인·스킬, 연결된 채널, OAuth, 브라우저, 로컬 모델, 외부 API.

**제거:** 내장 영상·오디오 제작 및 업로드 파이프라인, 영상 모델 설치/다운로드, 독립 RSI 연구 엔진, 세대 반복·자동 채점·자동 코어 수정·자동 채택, 실험 스케줄러 및 해당 UI/API/실행 스크립트. 영상·음성이 필요하면 향후 사용자가 승인한 **외부 MCP·플러그인 도구**로 연결합니다.

## 실행 구조

- `crates/dominion-agent-sdk`: 실행·계획·도구 발견·인수 검증·예산·체크포인트·검증·복구를 소유하는 독립 Rust 커널.
- `src/engine/native_agent.rs`: 실제 모델·MCP·파일·권한·영수증을 SDK에 연결하는 제품 어댑터.
- `src/engine/chat/runtime.rs`: 스트리밍 대화·세션·기억·도구 실행을 유지하면서 RSI 전용 판단 분기를 제거한 일반 채팅 경로.
- `src/engine/jobs.rs`, `src/db.rs`: SQLite 작업 큐, 인덱스 기반 대기/실행 기록 조회, 작업 도착 시 알림, 승인·취소·복구 및 기록 보존.
- `src/engine/mcp.rs`, `src/engine/skills.rs`: 외부 MCP 도구 및 플러그인·스킬 연결. 허용된 도구는 실행 직전 계약을 다시 검사합니다.
- `sdk/typescript`, `sdk/rust`, `packages/sdk-typescript`: 하나의 Rust 실행 제품을 사용하는 SDK 클라이언트.
- `web/src`: 채팅·기억·프로바이더·에이전트·그룹·채널·설정·개발 도구 등 기존 화면.

## 안전한 데이터 유지와 자원 절약

기존 SQLite의 메시지·기억·파일·작업 및 실패 기록은 삭제하지 않습니다. 이전 영상/RSI 작업은 **재실행을 금지**합니다. 대기 중인 작업은 취소 처리하고, 실행 중이었다면 확인 필요로 표시합니다. 과거에 완료된 기록과 결과는 그대로 열람할 수 있습니다.

일반 채팅의 `autoLearning`은 기본 해제, 5분마다 돌던 성장·증류 점검과 대화 종료 후 무인 정책 실험을 중단합니다. 작업 대기열은 과거 Job 전체를 400ms마다 재조회하는 대신 활성 상태만 SQL 인덱스로 읽고, 새 작업에는 `Notify`로 반응합니다. 정상적인 사용자 요청에 따른 편집·테스트·검증과 명시적 기억 저장은 유지합니다.

## 실제 API 및 검증

`POST /api/chat`, `/api/jobs`, `/api/sdk/runs`, `/api/harness/runs`에서 채팅·자율 실행·검증·승인·복구를 수행합니다. `/api/sdk/runs/:id/replay`, `/api/sdk/runs/:id/compare`는 저장된 증거를 읽어 **모델·외부 도구 재호출 없이** 실패 원인과 동일 조건 실행을 비교합니다. 외부 도구는 `/api/mcp/catalog`, `/api/mcp/discover`, `/api/skill-catalog`, `/api/channels`로 연결됩니다.

프런트 빌드: `npm ci --prefix web && npm run typecheck --prefix web && npm run build --prefix web`. Rust: `cargo test --release --locked`. 제품 잔재 검사: `node scripts/check-lean-product.mjs`. 최신 브랜치 HEAD에서의 CI를 최종 판정 근거로 삼습니다.

[경량화 구조·회귀 기준](docs/SUB3-LEAN-COMPLEXITY-2026-11-16.md) · [실행 기록·독립 검증](docs/SUB3-JUDGE-READY-2026-11-16.md).
