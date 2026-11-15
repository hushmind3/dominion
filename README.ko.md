# DOMINION

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="DOMINION icon" width="88" />
</p>

<p align="center"><strong>Bring your own model. Run your own agents. Own your workflow.</strong></p>
<p align="center"><a href="README.md">English</a> · <strong>한국어</strong></p>
<p align="center">내 모델로 실제 작업을 수행하는 오픈소스 AI 에이전트 워크스페이스.</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache 2.0" src="https://img.shields.io/badge/license-Apache%202.0-3B82F6" /></a>
  <img alt="Rust backend" src="https://img.shields.io/badge/engine-Rust-222222" />
  <img alt="React interface" src="https://img.shields.io/badge/UI-React-149ECA" />
  <img alt="Models" src="https://img.shields.io/badge/models-local%20%2B%20API-4F46E5" />
</p>

<p align="center">
  <a href="#-빠른-시작">빠른 시작</a> ·
  <a href="#-주요-기능">주요 기능</a> ·
  <a href="#-어떤-프로젝트인가요">프로젝트 소개</a> ·
  <a href="#english">English</a> ·
  <a href="docs/GETTING-STARTED.md">설치 문서</a> ·
  <a href="ENGINE.md">엔진 상세</a>
</p>

---

## 📸 DOMINION 미리보기

**그룹 대화 · 회의 — 여러 에이전트가 한 공간에서 대화하고 협업하는 화면**

<p align="center">
  <a href="docs/01-multi-agent-room.webp">
    <img src="docs/01-multi-agent-room.webp" alt="DOMINION multi-agent room with participants, shared conversation and room controls" width="100%" />
  </a>
</p>

<table>
  <tr>
    <td width="50%" align="center"><strong>💬 채팅 · 모델 선택</strong></td>
    <td width="50%" align="center"><strong>🤖 에이전트 스튜디오</strong></td>
  </tr>
  <tr>
    <td><a href="docs/02-chat-workspace.webp"><img src="docs/02-chat-workspace.webp" alt="DOMINION chat workspace and model controls" width="100%" /></a></td>
    <td><a href="docs/03-agent-studio.webp"><img src="docs/03-agent-studio.webp" alt="DOMINION agent studio with configurable agents and roles" width="100%" /></a></td>
  </tr>
  <tr>
    <td align="center">API/로컬 모델을 선택하고 작업을 지시하는 채팅 공간</td>
    <td align="center">에이전트별 역할·지침·모델을 관리하는 화면</td>
  </tr>
</table>

<sub>실제 개발 중인 DOMINION UI 캡처입니다. 스크린샷은 특정 기능의 실모델 검증 결과를 보증하지 않습니다.</sub>

---

## 🚀 어떤 프로젝트인가요?

**DOMINION은 LLM 모델이 아니라, 여러분이 선택한 모델로 일을 수행하는 에이전트 도구입니다.**

클라우드 API 키를 직접 연결하거나 로컬 모델을 사용하세요. 채팅으로 작업을 지시하면 Rust 실행 엔진이 도구 호출, 승인, 결과 기록, 기억 및 작업 흐름을 관리합니다. 여러 에이전트를 만들고 그룹 대화나 워크플로에 배치할 수도 있습니다.

**모델은 사용자가 선택하고, 실행 흐름은 DOMINION이 관리합니다.** 특정 모델 판매나 자체 API 크레딧 재판매를 전제로 하지 않습니다.

> **개발자 프리뷰** · 소스코드와 실행 방법을 공개하는 단계입니다. 모든 OS·모델·채널 조합의 실사용 안정성을 보장하지 않으며, 자기개선(RSI)은 별도의 실험 기능입니다. 현재 [GitHub Releases](https://github.com/hushmind3/dominion-rust/releases)에서 사전 빌드된 배포 파일을 제공하지 않으므로 아래의 소스 실행 안내를 먼저 확인해 주세요.

## ✨ 주요 기능

| 영역 | DOMINION에서 할 수 있는 일 |
| --- | --- |
| **🔌 모델 선택 / BYOK** | 자신이 보유한 모델 API 키를 등록하고 제공자·모델을 선택합니다. API 사용료는 해당 제공자 정책에 따릅니다. |
| **🖥️ 로컬 모델** | GGUF/llama.cpp, Ollama 등 로컬 추론 경로를 연결합니다. macOS에는 MLX 경로도 있습니다. |
| **🤖 에이전트** | 에이전트별 지시사항, 모델, 기억을 분리하고 작업을 수행합니다. |
| **🧠 기억과 경험** | 대화·도구 결과·사용자 정정을 로컬에 기록하고 관련 정보를 다음 작업에 재사용합니다. 모델 가중치를 재훈련하는 기능은 아닙니다. |
| **🛠️ 실제 도구 실행** | 파일·셸·HTTP·검색·MCP 도구를 호출하며, 승인/취소 및 실행 증거를 관리합니다. |
| **👥 멀티 에이전트** | 공용 기록을 읽는 그룹 대화, 순번 발언, 멘션, 다단계 워크플로를 구성합니다. |
| **💬 외부 채널** | Telegram·Discord용 Rust 연결 드라이버가 있습니다. 현재 봇 토큰·채널 정보 등의 **수동 설정이 필요**합니다. |
| **🧪 실험적 자기개선** | 소스 변경·평가·채택을 다루는 연구 경로를 제공합니다. 반복적인 지능 향상은 실증된 완성 기능이 아닙니다. |

### 왜 별도의 에이전트 도구인가요?

- **모델 종속을 줄입니다.** 한 모델·한 공급자만 사용하도록 강제하지 않습니다.
- **실행 기록이 남습니다.** 답변뿐 아니라 도구 호출과 오류, 승인, 작업 상태도 추적합니다.
- **로컬에서 운영할 수 있습니다.** SQLite 데이터와 기억은 기본적으로 로컬에 저장되며, 클라우드 모델 연결은 사용자가 선택합니다.
- **직접 확장할 수 있습니다.** Apache-2.0 공개 소스와 Rust/React 구조를 제공합니다.

## ⚡ 빠른 시작

macOS 또는 Windows에서 **소스코드로 실행**하는 방법입니다. Git, Rust stable/Cargo, **Node.js 24.12+**, OS별 C/C++ 빌드 도구가 필요합니다. Windows는 MSVC 빌드 도구 및 Windows SDK를 준비하세요.

~~~bash
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home .
~~~

실행 후 브라우저에서 **http://127.0.0.1:4317** 을 엽니다.

1. **모델 관리**에서 본인의 API 제공자 또는 로컬 모델을 연결합니다.
2. **채팅**에서 모델을 선택하고 간단한 작업을 맡깁니다.
3. 필요한 경우 **에이전트·기억·워크플로·그룹 대화**를 설정합니다.
4. Telegram/Discord는 연결 설정에서 본인의 봇 토큰 및 대상 정보를 등록합니다.

상세 설치·OS별 준비 사항은 **[설치와 첫 사용](docs/GETTING-STARTED.md)** 을 참고해 주세요. 외부 API 이용 시 사용료가 발생할 수 있으며, 로컬 모델은 사용자의 하드웨어 자원을 사용합니다.

## 🧩 구조

~~~text
React UI (web/)
       │
       ▼
Rust API / agent harness (Axum)
       ├── Agent chat · tools · approval · cancellation
       ├── Memory · experience · jobs · multi-agent rooms
       ├── Skills · MCP · Telegram · Discord
       ├── SQLite (local state and execution history)
       ├── User-selected model APIs
       └── Local model runtimes (e.g. GGUF / llama.cpp)
~~~

Node.js는 프론트엔드 **빌드 도구**로 사용하며, 에이전트 실행 엔진은 Rust입니다. 실제 공급자 연결·로컬 실행 환경에 따라 추가 프로세스가 필요할 수 있습니다.

## 🧪 현재 개발 상태와 한계

DOMINION은 기능 수보다 **실제 작업 성공률과 안정적인 실행 경험**을 개선해 가는 프로젝트입니다.

- 채팅·도구·기억·그룹 대화·작업 큐 등의 구현이 있으며, 실제 모델/운영 환경에 따라 동작 차이가 있습니다.
- 그룹 대화의 일부 검증은 모의 모델 기반입니다. 실모델 장시간 동작을 모두 검증했다는 뜻은 아닙니다.
- Telegram/Discord는 현재 **원클릭 봇 생성이 아닌 수동 연동**입니다.
- 공급자별 캐시·컨텍스트 최적화 경로가 존재하지만 **일정한 토큰 절감률을 보장하지 않습니다**.
- RSI는 소스 수정과 검증 절차를 연구하는 기능이며, **범용 재귀적 지능 향상을 입증한 제품은 아닙니다**.
- 빌드/테스트 결과는 [GitHub Actions](https://github.com/hushmind3/dominion-rust/actions)에서 커밋별로 확인해 주세요.

## 🤝 참여하기

실제 사용 중 발견한 문제나 개선 제안이 가장 유용합니다.

- **[Issues](https://github.com/hushmind3/dominion-rust/issues)** — OS, 모델/제공자, 재현 절차, 기대 결과를 포함해 문제를 제보해 주세요. API 키·개인 대화·토큰은 올리지 마세요.
- **[Pull Requests](https://github.com/hushmind3/dominion-rust/pulls)** — Rust 하네스, UI, 모델 연결, 채널 드라이버, 문서 등 개선을 환영합니다.
- 실제 화면을 보여주는 스크린샷과 재현 가능한 테스트도 프로젝트를 개선하는 데 도움이 됩니다.

더 깊은 구현 자료: [엔진 설명](ENGINE.md) · [문서 목차](docs/INDEX.md) · [RSI 실행 계약](docs/RSI-MAIN-INTEGRATION-2026-11-12.md) · [이전 상세 README 보관본](docs/README-TECHNICAL-SNAPSHOT-2026-11-14.md)

## English

**DOMINION is an open-source, local-first AI agent workspace — not an LLM model or a hosted API resale service.**

Bring your own API credentials or connect a local model, then use a Rust-native execution harness to run tool-using agents with persistent memory, approvals, logs, workflows, and multi-agent rooms. Telegram and Discord adapters are implemented but currently require manual bot configuration. Experimental self-improvement research is available separately; autonomous recursive intelligence gains are **not** claimed.

**Getting started:** [Installation guide](docs/GETTING-STARTED.md) · [Engine internals](ENGINE.md) · [Issues](https://github.com/hushmind3/dominion-rust/issues) · [Pull requests](https://github.com/hushmind3/dominion-rust/pulls)

**License:** [Apache License 2.0](LICENSE).


## 🧩 Rust / TypeScript SDK (서브3)

도미니언 자체 Rust 엔진과 동일한 API·작업 상태를 사용하는 별도 SDK를 추가했습니다.

- [Rust SDK](sdk/rust/README.md): `dominion-sdk` — 독립 Rust 실행 커널(자율 판단·실제 도구 실행·사용자 중간 지시·검증·복구)과 선택형 HTTP 클라이언트.
- [TypeScript SDK](sdk/typescript/README.md): `@dominion-ai/sdk` — 타입 기반 API, 스트림 재접속, 작업 상태/취소·완료 대기.
- [구조·2026 프런티어 비교](docs/SDK-ARCHITECTURE-2026-11-15.md).

CLI 하네스나 타사 에이전트 실행기를 SDK에 넣지 않습니다. 실제 판단·도구·메모리·체크포인트는 `src/engine/`이 담당하고 SDK와 GUI는 `/api/*`에 연결됩니다. 두 패키지는 아직 패키지 레지스트리에 게시하지 않았으며 Rust SDK 컴파일과 실모델 통합 검증은 별도 환경에서 진행해야 합니다.


### Codex WebSocket 세션·장기 문맥 보호 (서브3, 2026-11-15)

Rust 대화 전송 계층은 `previous_response_not_found`처럼 명시적으로 거부된 체인의 안전한 전체 재전송과, 이미 모델에 전달됐을 수 있으나 완료를 확인하지 못한 요청을 구분합니다. 후자는 WebSocket/HTTP/다른 모델로 자동 중복 생성하지 않고 비용 미확정 감사 기록을 남깁니다. 이미지·음성 등 구조화 입력은 순서와 내용을 해시로 비교하며 원본 미디어를 연결 레지스트리에 중복 저장하지 않습니다. 독립 Rust SDK는 최신 대형 도구 결과가 LowCost 문맥 한도를 넘더라도 잘라서 표시하고 원본 영수증은 영구 검색·조회 대상으로 유지합니다. 상세: [Codex 전송](docs/SUB3-NATIVE-STREAM-2026-11-15.md), [장기 문맥](docs/SUB3-DURABLE-CONTEXT-2026-11-15.md). 전송량 추정은 실제 청구 토큰 절감 수치가 아닙니다. 신규 집중 검증 [GitHub Actions #43688832106](https://github.com/hushmind3/dominion-rust/actions/runs/43688832106) 성공: Rust SDK 45개, 네이티브 모델/호스트 41개, Codex WebSocket 15개, 워크플로 복구 1개로 **102/102** 통과, TypeScript 타입 검사 및 Vite 배포 빌드도 통과했습니다. 실제 유료 제공자 장기 완수율·청구 절감률은 별도 측정 대상입니다.


### 자율 작업의 제공자별 모델 문맥 상한 (서브3)

`native_model::fit_native_context`는 모델 호출 직전에 제공자의 설정된 문맥 용량과 출력 예약량을 확인하고, 오래된 도구 호출·결과를 **쌍 단위**로 덜어냅니다. 사용자의 현재 목표·시스템 지시·도구 정의는 잘라내지 않고, 전체 도구 영수증은 Rust SDK에 보존해 재검색할 수 있습니다. 필수 입력만으로 문맥을 초과하면 유료 모델 요청 전에 중단하며, 생략 건수와 추정 입력량을 기록합니다. 최신 [GitHub Actions #43689095869](https://github.com/hushmind3/dominion-rust/actions/runs/43689095869)에서 Rust SDK 45개, 네이티브 호스트 43개, Codex WebSocket 15개, 복구 1개 등 **104/104** 통과하고 프런트 타입 검사·빌드도 성공했습니다. 상세 내용은 [장기 문맥 설계](docs/SUB3-DURABLE-CONTEXT-2026-11-15.md)를 참조하십시오.


### 자율 에이전트 Codex WebSocket 세션 연속성 (서브3, 2026-11-15)

`/api/jobs`의 실제 Rust 실행 커널에 `src/engine/native_codex_ws.rs`를 연결했습니다. 모델이 완료한 응답 ID를 기준으로 **직접 실행해 영수증이 남은 도구 결과**와 최신 작업 상태만 다음 WebSocket 요청에 보냅니다. 세션·문맥이 달라지거나 이전 응답이 만료되면 안전하게 전체 문맥을 다시 구성합니다. 요청의 성공 여부가 불명확하면 HTTP·다른 모델로 자동 중복 생성하지 않고 검토 상태를 보존합니다. 설정된 모델 문맥 한도에 가까워지면 제공자가 보고한 토큰을 바탕으로 응답 체인을 새로 시작합니다. Rust의 권한·승인·검증·저장·복구 구조와 TypeScript SDK/API·GUI는 기존 경로를 그대로 사용합니다. 상세 구현과 검증 범위: [서브3 Codex 자율 작업 WebSocket](docs/SUB3-NATIVE-CODEX-WS-2026-11-15.md).

**자율 작업 WebSocket 집중 검증:** [GitHub Actions #43690699582](https://github.com/hushmind3/dominion-rust/actions/runs/43690699582)에서 Rust SDK·네이티브 호스트·대화용 WebSocket·워크플로 복구 **112/112 통과**, TypeScript 타입 검사와 Vite 빌드도 성공했습니다. 실제 유료 Codex 계정 호출, 청구 토큰 절감률, 장기 작업 벤치마크는 별도로 측정해야 합니다.

### 🔧 자율 작업 토큰·캐시 사용량 실측 표시 (서브3, 2026-11-15)

Rust 엔진이 모델 제공자의 실제 응답에서 입력·출력·캐시 읽기·캐시 쓰기 토큰을 수집하고, 작업별 누적 기록인 `nativeModelUsage`를 저장합니다. 제공자가 사용량을 보내지 않은 호출은 **미보고 횟수로 분리**해 0원·0토큰으로 위장하지 않습니다. Anthropic의 별도 캐시 읽기·생성 토큰도 Rust 실행 예산의 입력 토큰 합계에 포함합니다. 이제 자율 작업 GUI에서 실제 보고 사용량과 미보고 경고를 확인할 수 있고, TypeScript SDK에서 같은 데이터 타입을 제공합니다. [세부 설계](docs/SUB3-NATIVE-METERING-2026-11-15.md). [GitHub Actions #43691823640](https://github.com/hushmind3/dominion-rust/actions/runs/43691823640)에서 **Rust 115/115**, 독립 **TypeScript SDK 7/7**, React 타입 검사·Vite 빌드 모두 통과했습니다. 실제 API 청구원가와 절감률은 제공자별 계정 요금 검증 전까지 금액으로 단정하지 않습니다.

자율 작업 화면의 고급 설정에서 **실행 토큰 예산**도 직접 입력할 수 있습니다. 값은 기존 `/api/jobs.input.tokenBudget`를 통해 Rust SDK의 `maxEstimatedTokens`로 전달되며, 진행 중에는 커널 예산 사용량과 제공자 실측 사용량을 구분해 보여줍니다. 이전 작업을 다시 시작할 때도 기존 예산을 보존합니다.

최종 GUI 예산 연동까지 반영한 [집중 CI #43692151683](https://github.com/hushmind3/dominion-rust/actions/runs/43692151683)도 **Rust 115/115 · TypeScript SDK 7/7 · 프런트 타입 검사·빌드 전부 통과**했습니다.


### 🔧 자율 에이전트 프롬프트 캐시·중복 청구 방지 (서브3, 2026-11-15)

Rust 자율 작업의 일반 OpenAI 호환 모델 호출을 **1회 전송** 경로로 전환했습니다. 네트워크가 끊겼다고 이미 모델이 수신했을 수 있는 유료 요청을 임의로 재전송하지 않습니다. Anthropic 모델에서는 반복되는 **도구 정의·시스템 지시**에 5분짜리 명시적 캐시 경계를 설정하고, 매 턴 바뀌는 현재 작업·관찰 내용은 캐시 쓰기 대상으로 지정하지 않습니다. 캐시를 지원하지 않는 제공자가 `cache_control`을 명시적으로 거부한 경우에만 원본 요청을 한 번 재시도하며, 해당 호환성 결과를 제공자·모델별로 기억합니다. 로컬 Anthropic 호환 서버는 캐시 지원을 명시적으로 선언한 경우에만 사용합니다. 캐시 쓰기/읽기 실측과 작업 예산은 기존 Rust 엔진·SDK·GUI 경로를 그대로 사용합니다. [구현 구조 및 테스트](docs/SUB3-NATIVE-PROMPT-CACHE-2026-11-15.md).

실제 TCP·HTTP 회귀 테스트를 포함한 [서브3 집중 CI #43693114690](https://github.com/hushmind3/dominion-rust/actions/runs/43693114690)에서 **Rust 121/121**, **TypeScript SDK 7/7**, 프런트엔드 타입 검사·빌드가 전부 통과했습니다. 연결이 끊긴 모델 호출 재실행 금지, 캐시 힌트 명시적 거부 후 안전한 원본 재요청, 캐시 읽기·쓰기 토큰 누적을 검증했습니다. 유료 제공자의 실청구 절감률은 아직 별도 측정 대상입니다.

### 🧠 Rust 품질 보존형 모델·비용 자동 선택 (서브3, 2026-11-15)

모델 설정 화면에 **모델별 품질 점수(0~1)와 입력·출력·캐시 읽기·쓰기 단가(원/100만 토큰)**를 직접 등록할 수 있게 했습니다. 자율 작업 화면에서 **품질 하한을 유지한 비용 최적화**를 명시적으로 켜면 Rust 엔진이 후보 모델 중 품질 기준을 충족하는 모델만 비교해 비용순으로 선택합니다. 사용자가 모델이나 에이전트 제공자를 직접 지정하면 **자동으로 바꾸지 않습니다**. 적격 후보가 없으면 알리지 않고 저품질 모델로 전환하는 대신 유료 호출 전 중단합니다.

선택된 제공자·모델·엔드포인트·인증 계약은 작업 기록에 고정됩니다. 작업을 재개하거나 진행 중 제공자 설정이 바뀌어도 임의로 다른 모델에 요청하지 않습니다. 캐시 사용량 기록이 3개 이상의 완료 작업에서 충분히 쌓인 경우에만 **보수적으로** 예상 캐시 절감을 계산에 반영합니다. 완료된 호출의 실측 토큰에 등록 단가를 곱한 **원화 비용 추산**은 작업 화면과 TypeScript SDK에서 확인할 수 있으며, 단가 또는 사용량이 없는 호출은 비용 미산정으로 구분합니다. 실청구서 확인 없이 확정 지출이라고 표시하지 않습니다. [설계·API·검증](docs/SUB3-NATIVE-QUALITY-COST-ROUTING-2026-11-15.md).

품질 보존형 라우팅과 캐시 이력까지 적용한 [집중 CI #43695469189](https://github.com/hushmind3/dominion-rust/actions/runs/43695469189)에서 **Rust 126/126 · TypeScript SDK 7/7 · 프런트 타입 검사·빌드 모두 통과**했습니다. 실제 로컬 HTTP 호출까지 이어진 Rust 작업 통합 테스트에서 모델 선택·고정·원화 비용 추산·엔드포인트 변경 시 재개 차단을 확인했습니다. 유료 제공자의 실청구서와 작업 성공률 개선 효과는 별도 실측 대상으로 남깁니다.

### 🛠️ 서브3 RSI 실동작·독립 평가·실행 잔재 강화 (2026-11-15)

기존 Rust RSI의 실제 경로 **소스 지정 → Git 분리 실험 → 개선기 바이너리 재빌드 → 후손 작업으로 실제 소스 편집 → 동일 기준 원본/후보 A/B 컴파일·검증 → 승격·다음 세대**를 따라 실행 정합성을 수정했습니다. 이제 평가 보고서의 원본·후보 Git 트리를 별도의 Rust 임시 Git 인덱스로 재구성해 대조하고, **채택·브랜치 적용 시에도 정확히 같은 코드 트리**인지 검증합니다. 후손 작업이 검증 실패를 반환했는데 성공 완료로 기록하던 문제를 고쳤으며, 실험 초기화·A/B 실행 실패 후 Git worktree 등록 정보가 남는 잔재 경로도 정리했습니다. 단순 성공 플래그가 아닌 **실제로 컴파일되는 소형 Rust 프로젝트의 원본/개선본 A/B 검사**를 추가했습니다. [실제 실행 흐름과 검증 문서](docs/SUB3-RSI-EXECUTION-INTEGRITY-2026-11-15.md). 유료 모델을 이용한 여러 세대 RSI 완주·성공률 향상 검증은 별도의 남은 작업입니다.

**2026-11-15 RSI 집중 검증:** [GitHub Actions #43697949313](https://github.com/hushmind3/dominion-rust/actions/runs/43697949313)에서 **Rust 223/223**, 실제 Git·Cargo A/B **2/2**, TypeScript SDK **7/7**, React 타입 검사·Vite 빌드 전부 통과했습니다. 특히 진짜 Rust 개선기 바이너리 재빌드 → 새로운 세대 실행 → Git 실험 계보 채택 → 소스 브랜치 반영을 실제 프로세스 통합 테스트에서 확인했습니다. 상용 모델을 이용한 자율 다세대 RSI 완주와 프런티어 벤치마크는 현재 검증 결과에 포함되지 않습니다.
