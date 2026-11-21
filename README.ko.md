# 도미니언 러스트

[English](README.md) · [전체 아키텍처](ARCHITECTURE.md) · [하네스 계약](docs/HARNESS.md) · [Rust SDK](sdk/rust/README.md) · [TypeScript SDK](sdk/typescript/README.md) · [서브3 최신 통합 점검](docs/SUB3-INTEGRATION-2026-11-19.md)

**도미니언(DOMINION)**은 로컬·외부 AI 모델을 연결해 코드 수정, 파일·셸·웹/MCP 작업, 검증, 승인, 중단 및 복구를 수행하는 **사용자 통제형 Rust 실행 플랫폼**입니다. 단순한 채팅 UI가 아니라 실제 작업과 결과 증거를 SQLite에 남기는 실행 제품을 목표로 합니다.

> **이 문서는 `dominion-sub3` 브랜치 기준입니다.** `main`이나 배포판의 성능을 대표하지 않습니다. 구현된 기능과 테스트를 통과한 범위, 실제 모델로 검증한 성능은 구분합니다. 에이전트 실행에는 사용자가 구성한 모델·제공자가 필요합니다.

## 🚀 설치·실행

준비물: Rust 안정 버전 툴체인, **Node.js 24.12 이상**, npm. 데스크톱 패키징 대상은 **macOS·Windows**입니다.

```sh
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
git switch dominion-sub3
npm ci --prefix web
npm run build --prefix web
cargo run -- --open
```

Rust 서버가 기본적으로 **http://127.0.0.1:4317**에 React 화면을 제공합니다. 위 방식은 개발용 통합 실행이며 설치 프로그램을 만든 것은 아닙니다. 화면에서 모델·제공자를 연결하고 실행할 도구의 사용 권한을 설정해야 실제 에이전트 작업이 진행됩니다.

환경 설정: `DOMINION_PORT`(기본 4317), `DOMINION_DATA`(기본 `runtime`), `DOMINION_WEB`(기본 `web/dist`), `DOMINION_WORKSPACE`(기본 현재 디렉터리). DB는 기본적으로 `runtime/dominion.sqlite`에 생성됩니다. 기존 사용자는 마이그레이션·실험 전에 **runtime 데이터 폴더를 백업**해 두십시오.

화면만 개발할 때는 Rust 서버를 실행한 상태에서 `npm run dev --prefix web`을 사용할 수 있습니다. 데스크톱 패키지 빌드 코드는 `src-tauri/`와 `scripts/build-desktop.mjs`에 있습니다.

## 최신 메인 프론트 및 백엔드 호환성

2026-11-19 메인 `f86a36e`의 모델 탐색·공개 모델 목록·Google/카카오/네이버 로그인·계정 연결·채팅·그룹방 React 소스를 유지했습니다. 서브3에는 스테이징 설정 예시 `web/.env.cloud-sub3.example`이 추가돼 `web/` 전체 트리 해시는 메인과 달라졌지만, 실제 화면 구성요소 소스는 동일합니다. 서브3 Rust 하네스·그룹 컨텍스트 최적화, Codex WebSocket 계약별 캐시 무효화와 병렬 체크포인트 개선도 유지합니다.

**도미니언 회원제 클라우드 서버 이식:** 메인 `cloud/` Worker, D1 회원·사용량·결제 스키마, Google/카카오/네이버 로그인, 무료 모델 중계·스트리밍, 유료 모델 크레딧·TossPayments 결제 경로, `services/dominion-cloud/` 계약 코드를 서브3에 반영했습니다. 메인 운영 인프라와 분리한 `dominion-cloud-sub3` Worker 및 신규 D1 설정을 사용합니다. 실제 신규 D1과 OAuth/API 키를 아직 연결하지 않아 상용 로그인·결제가 완료된 상태는 아닙니다.

[클라우드 구조·스테이징 설정](docs/SUB3-CLOUD-GATEWAY-2026-11-19.md) · [클라우드 CI 8개 테스트 통과](https://github.com/hushmind3/dominion-rust/actions/runs/44503096540). 로컬 모델·사용자 개인 API 키 기능은 클라우드 계정 없이 계속 사용할 수 있습니다.

## 🧠 현재 구현 범위

| 영역 | 실제 코드 연결 |
| --- | --- |
| 자율 작업 | Rust 커널의 계획·도구 실행·검증, SQLite 영수증·체크포인트, 승인·취소·복구 |
| 일반 채팅·그룹방 | 스트리밍 대화, 모델 전환, 대화 기억, 그룹 참여자 상태와 컨텍스트 관리 |
| 모델·비용 | 로컬/API 제공자, 가능한 모델의 네이티브 도구 호출, 사용량 기록, 프롬프트 캐시 힌트와 Codex 증분 WebSocket |
| 도구 확장 | 파일·셸·브라우저·MCP·스킬·OAuth 연결, 승인된 도구만 실행하는 권한 경계 |
| 장기 기억 | SQLite 원문 보존, 교정·실패 기록, 작업 영수증 검색과 부분 복원 |
| SDK | 독립 Rust 실행 커널과 Rust/TypeScript API 클라이언트 |

**중요한 구조적 경계:** 일반 채팅 실행 경로와 자율 작업 Rust SDK 커널은 아직 **상위 실행 루프가 완전히 통합되지 않았습니다.** 겉으로 같은 화면과 작업 기록을 제공하더라도 하나의 실행기라고 주장하지 않습니다.

### 2026.11.21 — 유료 모델 호출 중단·복구 강화

네이티브 Rust 하네스는 모델 호출의 사용량과 검증된 판단 결과(또는 파싱 실패)를 **하나의 영속 체크포인트로 확정할 때까지** `model_in_flight` 재실행 방지 표시를 유지합니다. 모델 응답을 받은 뒤 DB 저장이 실패한 상황에서도 다음 시작에서 같은 호출을 무작정 반복하지 않고 재검토 상태로 처리합니다. 별도 UI·메뉴 변경은 없습니다. 저장 실패 주입 회귀 테스트를 서브3 SDK CI에 추가했습니다. [소스와 검증 계약](docs/SUB3-PAID-MODEL-SETTLEMENT-2026-11-21.md).

### 실행 경로

```text
React 자율 작업 패널 / TypeScript SDK / Rust HTTP 클라이언트
                         │
                /api/jobs · /api/sdk/runs
                         │
                 SQLite 작업 큐
                         │
              native_agent.rs (호스트)
                         │
         crates/dominion-agent-sdk::Kernel
           ├ 모델·Provider 어댑터
           ├ 승인된 파일/셸/MCP 도구
           ├ 검증·영수증·예산·복구
           └ 중단·승인·체크포인트

일반 채팅·그룹방 → /api/chat · /api/rooms
                 → src/conversation/chat/runtime/
                 → 별도 스트리밍 턴 실행 경로
```

기존 `/api/harness/runs`는 **신규 작업에 한해** 같은 Rust 작업 커널을 호출하는 호환 API입니다. 과거 독립 하네스 `sdk_run` 기록은 열람할 수 있지만, 미확인된 외부 작업을 임의로 재실행하지 않습니다. 호환 API의 도구 권한은 기존 작업 공간·셸 범위로 좁혀집니다. 자세한 내용은 [HARNESS.md](docs/HARNESS.md)에 있습니다.

## 🔧 API·SDK 사용

- 작업: `GET/POST /api/jobs`, `GET /api/jobs/:id`, `POST /api/jobs/:id/cancel`, `POST /api/jobs/:id/directive`
- 자율 실행 패널: `GET/POST /api/sdk/runs`와 `:id/approve`, `:id/cancel`, `:id/resume`, `:id/reconcile`, `:id/steer`
- 실행 기록 재검토: `GET /api/sdk/runs/:id/replay`, `POST /api/sdk/runs/:id/compare`, `POST /api/sdk/runs/:id/adjudicate` — 저장된 증거의 **읽기 전용** 재구성·비교
- 기존 기능: `POST /api/chat`, `/api/sessions`, `/api/providers`, `/api/rooms`, `/api/memory/overview`, `/api/mcp/catalog`, `/api/skill-catalog`

### TypeScript 클라이언트

```ts
// sdk/typescript/src/index.ts 의 Dominion 클래스 사용
import { Dominion } from './sdk/typescript/src/index'

const api = new Dominion({ baseUrl: 'http://127.0.0.1:4317' })
const run = await api.nativeRuns.start({
  instruction: '현재 작업 공간의 테스트 실패를 조사하세요',
  verifyCommand: 'cargo test',
})
const current = await api.nativeRuns.get(run.id)
// 승인 대기·중단·실패 상태를 확인한 뒤 계속 진행합니다.
```

`sdk/typescript`가 유일한 TypeScript REST/SSE SDK입니다. 기존 `/api/harness/runs` 호출은 Rust의 호환 API에서 계속 받아 동일한 실행 커널로 연결하며 중복 클라이언트 패키지는 제거했습니다. Rust 개발자는 `sdk/rust`에서 재노출하는 `dominion_sdk::harness::{Kernel, RunRequest, Model, Tools, Evidence, Journal}`를 사용합니다.

## 💾 컨텍스트·스킬·토큰 최적화

**2026년 로컬 KV 캐시 엔진 보강:** 내장 `llama.cpp` 모델은 `cache_prompt: true`를 사용하고, 지원하지 않는 `prompt_cache_key`를 전송·재시도하지 않습니다. 외부 llama.cpp 모델은 제공자 설정에 `"cacheBackend":"llama.cpp","kvCacheEnabled":true`, vLLM 모델은 `"cacheBackend":"vllm","kvCacheEnabled":true`를 지정하면 해당 서버의 KV 캐시 경로를 사용합니다. vLLM은 9인 그룹의 **공통 지침 캐시를 방 안에서 공유**하고 다른 방과는 격리합니다. 해당 로컬 모델은 기억을 보존하면서 누적 문맥을 1회 검색 예산보다 오래 유지합니다. **일반 HTTP 요청은 원본 프롬프트를 여전히 전송합니다. 서버가 앞부분의 재계산을 생략하는 방식이며 Codex WebSocket의 증분 전송과 다릅니다.** 서버 슬롯/모델 용량과 실제 캐시 적중률은 따로 검증해야 합니다. [그룹대화 구조 및 검증](docs/SUB3-ROOM-CONTEXT-DELTA-2026-11-18.md).


**고정 프롬프트·페르소나 최초 1회 전송(2026-11-18, Codex WebSocket 경로):** 첫 요청의 `developer` 입력에 그룹 공통 지침과 해당 에이전트 페르소나를 넣습니다. 연결된 후속 호출은 `previous_response_id`와 새 대화·도구 결과만 전송하므로 동일한 4,000토큰 지침을 네트워크 요청에 다시 싣지 않습니다. 지침 변경·세션 만료·연결 종료 시 원본으로 재초기화하며, 별도 에이전트의 페르소나와 기억이 섞이지 않도록 각각의 체인을 유지합니다. **일반 Chat Completions API와 세션 저장을 지원하지 않는 로컬 모델까지 동일하게 적용됐다는 뜻은 아닙니다.** 서버의 실제 KV 캐시·연결 상태 및 제공자 과금 토큰은 별개로 측정해야 합니다.


**9인 그룹대화 신규 토큰 최적화(2026-11-18):** 이전에는 주인 메시지 ID가 바뀔 때마다 공유 상태를 새로운 스냅샷으로 취급했습니다. 이제 **응답 인원·미응답자·확인된 작업 상태가 동일하면 공유 상태를 그대로 재사용**하고, 실제 내용이 달라지면 이전 스냅샷을 갱신합니다. 주인의 결정·정정, 이전 자기 답변, 필요한 동료 발언과 SQLite 원문은 유지합니다. 이 조치는 불필요한 프롬프트 캐시 초기화를 줄이기 위한 것이며, **확정 사실은 새 내용만 추가하고, 정정·삭제 시 기존 캐시를 갱신합니다.** 9인 그룹의 누적 캐시 예산과 회차별 검색 예산을 분리하고, 도구 스키마 순서를 고정했습니다. 캐시 성능이 확인되지 않은 로컬 모델은 누적 범위를 작게 잡으며, 뒤늦게 실행된 과거 주인 요청에 미래 정보가 섞이지 않도록 보호합니다. 호출별 실제 캐시 적중·신규 입력 토큰 계측도 추가했습니다. 9명 × 100턴 Rust 회귀 테스트는 작성했으나 아직 실행하지 않았습니다. **실제 유료 모델의 신규 토큰 절감 수치는 아직 측정 전**입니다. [그룹 컨텍스트 설계·회귀 테스트](docs/SUB3-ROOM-CONTEXT-DELTA-2026-11-18.md).

- Rust 자율 작업과 일반 채팅은 관련도·권한에 따라 스킬을 선택합니다. 관계없는 전체 스킬 설명을 무조건 모델에 넣지 않습니다.
- 장기 실행 기록 원본은 저장하고, 모델 입력에는 필요한 영수증만 선별해 제공합니다. 더 오래된 근거는 `dominion_receipt_search/read`로 조회합니다.
- 모델 제공자의 **실제 캐시 적중 토큰**과 도미니언이 추정한 **이력 재사용량**을 분리해 기록합니다. WebSocket 증분 전송만으로 API 과금 절감이 입증되는 것은 아닙니다.
- **2026-11-09** 과거 9인 그룹 100턴 측정의 캐시 적중률은 **18.15%**, 이력 예산 재구성은 **621회**였습니다. 현재 서브3 HEAD의 결과가 아니므로 재측정이 필요합니다. [계측 기록](docs/SUB3-CACHE-CONTEXT-2026-11-18.md)

## 🧹 제거된 기능과 보존되는 자산

**제거:** 내장 영상·음성 제작 파이프라인, 영상 모델 자동 설치·렌더·업로드, 독립 RSI 세대 반복·실험·자동 코어 수정 및 자동 채택. 미디어가 필요하면 사용자가 승인하는 **외부 MCP/플러그인 도구**를 연결하는 방향입니다.

**보존:** 기존 대화, 기억, 파일, 작업 이력, 사용자 원문 기반 수동 학습·교정 기록, 비활성 Skill 초안, 에이전트 성장 통계 화면, 검증된 절차와 이전 실행 증거. 수동 기억 추출에서 구형 자동 전략·도구정책·하네스 변경 제안 코드는 삭제했습니다. 오래된 기록과 사용자 파일은 임의로 삭제하거나 부작용을 재실행하지 않습니다.

GitHub·Google 등의 연결은 구성된 OAuth 앱 또는 인증된 공식 CLI를 사용할 수 있습니다. 제공자 동의 및 필요한 설정을 우회하지 않습니다. [연결 계정 설명](docs/CONNECTED-ACCOUNTS-2026-11-17.md)

## 🛑 실행 직전 취소·지시 수정 경계 (2026-11-20)

Rust 에이전트 실행 루프는 모델 요청·외부 도구·사용자가 지정한 필수 검증 명령을 호출하기 직전, **실행 의도 체크포인트 저장 후 취소 및 사용자 지시 변경을 다시 확인**합니다. 저장 중 취소가 발생하면 시작되지 않은 호출은 실행하지 않고, 지시가 변경되면 기존 도구 계획을 폐기한 뒤 수정된 지시를 반영해 다시 계획합니다. 이미 시작한 외부 작업의 결과가 불확실하면 기존 영속 영수증·수동 검토 절차를 유지합니다. `crates/dominion-agent-sdk/tests/kernel_replanning.rs`에 모델·도구·검증 취소 및 도구 실행 직전 지시 변경 회귀 테스트를 추가했습니다. 진행 중인 제공자 호출을 즉시 중단한다는 의미는 아닙니다.

## 🧩 그룹 대화 읽음 처리·증거 연속성 (서브3, 2026-11-21)

그룹에 새로 참여한 에이전트의 최초 읽기 커서는 **현재 주인 메시지**에서 시작합니다. 이전 방식처럼 최초 컨텍스트 작성 시점의 마지막 동료 메시지까지 한꺼번에 읽음 처리하지 않으므로, 필수 미확인 입력으로 실제 전달되지 않은 동료 발언을 다음 턴에서 누락하지 않습니다. 후속 실행은 SQLite 읽기 커서에 따라 미확인 메시지의 연속 구간만 선택하고, 최종 답변과 읽음 상태를 같은 트랜잭션으로 확정합니다. 기존 검색 가능한 과거 원문과 인덱스는 유지합니다. 그룹 작업판에서는 최신 에이전트 답변에 연결된 실제 작업 영수증이 없더라도 이전의 유효한 증거를 보존하고, 다른 에이전트에게 귀속된 작업 영수증을 제외하도록 보강했습니다. 관련 회귀 테스트는 `room_cursor::tests`, `workboard::tests`이며 메인·UI 메뉴는 변경하지 않았습니다.

## 🧪 개발 테스트와 릴리즈 검증

평상시에는 변경한 범위의 **Rust 개발 프로파일 테스트**만 실행합니다.

```sh
cargo test -p dominion-agent-sdk --lib
cargo test -p dominion-sdk --lib
cargo test --bin dominion-rust engine::rooms::tests
cargo test --bin dominion-rust api::providers::tests
cargo test --bin dominion-rust api::cli_oauth::tests
cargo test --bin dominion-rust engine::chat::context::tests
cargo test --bin dominion-rust engine::skill_discovery::tests
cargo test --bin dominion-rust api::sdk::tests
```

프론트 타입 검사·전체 프론트 테스트·Vite 빌드는 서브3 제품 CI에서 실행합니다. Windows/macOS 설치 패키지 릴리즈 빌드는 별도 릴리즈 게이트입니다.

```sh
npm run typecheck --prefix web
node --experimental-strip-types --test web/tests/*.test.mjs
npm run build --prefix web
node scripts/check-lean-product.mjs
```

**최신 문서 정리 이전 검증 코드:** `4e9f5bdf`(2026-11-20). [제품 Rust·프론트·TypeScript SDK CI](https://github.com/hushmind3/dominion-rust/actions/runs/44665179631) 및 [Rust SDK CI](https://github.com/hushmind3/dominion-rust/actions/runs/44665179763)가 통과했습니다. 폐기된 내장 영상·RSI, YouTube 업로드 인증, 구형 정책 자동 승격 및 중복 SDK를 제거하되 대화·사용자 파일·검증 기록은 보존합니다. 이후 커밋은 해당 GitHub Actions 결과를 따로 확인해야 합니다. 전체 릴리즈 게이트, 실제 27B 모델과 유료 제공자 과금 검증은 실행하지 않았습니다. [현재 문서 지도](docs/INDEX.md)를 참고하십시오.

**미완성·미측정:** 채팅/자율 실행 루프 완전 통합, 실제 모델의 장기 작업 완수율, 최신 브랜치의 Windows/macOS 릴리즈 검증, 실과금 기준의 캐시 절감률과 상용 운영 검증.

자세한 내용: [서브3 코드 감사](docs/SUB3-SOURCE-AUDIT-2026-11-18.md) · [경량화 구조](docs/SUB3-LEAN-COMPLEXITY-2026-11-16.md) · [독립 증거·복구](docs/SUB3-JUDGE-READY-2026-11-16.md).

라이선스: [Apache-2.0](LICENSE).
