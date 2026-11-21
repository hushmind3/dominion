# 도미니언 러스트

<p align="center"><img src="src-tauri/icons/icon.png" alt="DOMINION" width="90" /></p>
<p align="center"><strong>내 모델, 내 작업 공간, 독립 Rust 에이전트 실행기.</strong></p>
<p align="center"><strong>한국어</strong> · <a href="README.md">English</a> · <a href="LICENSE">MIT</a> · <a href="https://github.com/hushmind3/dominion-rust/releases">설치 파일</a></p>

DOMINION은 **Rust 실행 엔진 + React/Vite 인터페이스 + Tauri 2 데스크톱**
구조의 로컬 우선 오픈소스 AI 에이전트 워크스페이스입니다.
**Windows와 macOS**에서 같은 코드의 네이티브 실행기와 UI를 사용합니다.

<p align="center">
  <img src="docs/01-multi-agent-room.webp" alt="다중 에이전트 방" width="48%" />
  <img src="docs/02-chat-workspace.webp" alt="채팅 작업 공간" width="48%" />
</p>

## 주요 기능

- **실제 작업 수행:** Rust 단일 실행 루프, 도구 실행 승인, 예산 제한,
  취소·재개, 효과 영수증, 결과 검증과 복구. 팀 패치는 모든 Worker의
  고정 증거를 확인하고 중단 신호를 재검사한 후 통합하며, 재시작 중단된
  외부 CLI 작업은 재실행 대신 검토 상태로 보관합니다.
- **복원·로컬 API 보호:** 유지보수 모드에서 백업을 복원할 때 기존 SQLite
  DB·vault 키를 함께 보존해 실패·중간 종료 뒤 재시작에서 이전 상태를 복구합니다.
  실행 중인 작업은 복원 전에 종료되어야 하며, owner GET은 로컬 Host를 검사합니다.
  인증된 원격 Worker는 별도 접근 제어를 유지합니다.
- **모델 선택:** OpenAI·Anthropic 등 API, Ollama·llama.cpp·LM Studio 같은
  로컬 추론 엔드포인트 연결. 엔드포인트별 텍스트·도구 기능 제한은
  자동 대체 모델을 선택할 때도 적용합니다.
- **일반 채팅 / 에이전트 선택:** 새 1:1 대화는 일반 채팅으로 시작합니다.
  메시지 입력창에서 에이전트를 선택하면 도구 실행·승인·작업 검증을 사용할 수 있습니다.
  일반 채팅에서는 에이전트별 지침·도구 호출·자동 학습을 끄고 모델에 직접
  질문합니다. 양쪽 모두 대화 내용과 사용량을 기존 Rust 저널에 보관하며,
  기존 에이전트 세션은 에이전트 모드를 유지합니다.
- **2026 일반 채팅 경량화:** 도구 사용을 켜 두어도 인사·설명 요청에는
  불필요한 네이티브·MCP 스키마를 붙이지 않으며, 파일·웹·작업 요청은 기존
  Rust 승인·실행 경로를 유지합니다. OpenAI 호환 모드의 최종 전송 요청을
  기준으로 컨텍스트 한도를 검사하고 공급자 실측치와 추정치를 구별해 기록합니다.
- **다중 에이전트 방:** 에이전트별 페르소나와 독립 문맥, 공통 방 상태,
  SQLite 대화 기록, 호출별 토큰 계측. 참가자별 읽기 위치는 실제 전달된
  메시지 ID를 기록하며, 늦게 도착한 답변이 새 주인 요청의 근거를
  이전 턴에서 읽었다고 잘못 기록하지 않도록 보호합니다. 100개 이상 메시지가
  누적된 방에서도 검색된 과거 주인 요청과 실제 에이전트 답변을 예산 안에서
  함께 선택하도록 보강했습니다.
- **도구:** 파일·셸·브라우저와 외부 MCP 확장, 동일한 승인·권한·감사 체계.
  외부 Codex·Claude·Qwen 조사 작업은 정상 완료된 이전 작업 ID를
  명시적으로 지정하면 같은 읽기 전용 격리 워크트리에서 이어갈 수 있습니다.
  세션 ID·소유권·원본 Git 커밋·미사용 상태를 검증하며 중단된 작업은 자동 재실행하지 않습니다.
- **지속 상태:** 대화·기억·작업·검증·승인 기록의 SQLite 저장
- **SDK:** Rust 실행 커널과 작업 생성·승인·취소·검증용 TypeScript 클라이언트
- **데스크톱:** 실행 시 별도 Node 서버가 필요 없는 Rust 사이드카 기반 구성

## 설치 파일

[GitHub Releases](https://github.com/hushmind3/dominion-rust/releases)에
**Apple Silicon용 DMG, Intel Mac용 DMG, Windows용 EXE·MSI**를 제공합니다.
코드 서명 없는 빌드이므로 설치 시 운영체제의 보안 확인이 필요할 수 있습니다.

버전별 배포는 **동일한 소스 커밋에서 Windows·macOS 네이티브 검사와
빌드가 모두 성공한 후** 생성하도록 구성했습니다. 프런트 빌드만
성공했다고 데스크톱 릴리즈 완료로 취급하지 않습니다.

## 개발 환경

**Rust stable, Node.js 24.12 이상, npm**과
[Tauri 2 운영체제별 사전 준비](https://v2.tauri.app/start/prerequisites/)가 필요합니다.

```bash
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
npm ci --prefix web
npm run typecheck --prefix web
npm run build --prefix web
cargo test --locked --workspace
```

개발 서버는 터미널 2개에서 다음을 실행합니다.

```bash
npm run dev:server --prefix web
npm run dev --prefix web
```

운영체제별 데스크톱 빌드:

```bash
node scripts/check-release-contract.mjs
node scripts/build-desktop.mjs
```

코어와 알고리즘은 [분리 계약](docs/CORE-ALGORITHM-BOUNDARY-2026.md)에 따라
연결합니다. 코어에는 실행 인터페이스만, 계획·도구 선택·작업 분해 정책은 부가기능에 둡니다.

## 코드 구조

| 계층 | 실제 코드 |
| --- | --- |
| 실행 커널·효과 영수증 | [dominion-core 계약](crates/dominion-core/src/runtime_port.rs), [저널](crates/dominion-core/src/journal.rs) |
| 에이전트 실행 루프·오케스트레이션 | [dominion-algorithms](crates/dominion-algorithms/src/engine.rs), [루프 회귀 테스트](crates/dominion-algorithms/src/engine_tests.rs) |
| 코어/알고리즘 인터페이스 | [TurnStrategyPort](crates/dominion-core/src/strategy.rs), [호스트 조립](src/addons/runtime_hooks.rs), [교체형 알고리즘](src/addons/strategies/turn_policy.rs) |
| 실행·도구 승인 | [채팅 실행기](src/engine/chat/runtime.rs), [도구 게이트웨이](src/engine/chat/runtime/tool_dispatch.rs) |
| 개인 대화 문맥 | [Context Engine](src/engine/chat/context_engine.rs), [문맥 연속성](src/engine/chat/continuity.rs) |
| 방·기억·연속성 | [그룹 방](src/addons/integrations/rooms.rs), [문맥 구성](src/addons/integrations/chat_room_context.rs), [연속성](src/engine/chat/continuity.rs) |
| HTTP·영속 저장 | [API 라우터](src/api/mod.rs), [SQLite](src/db.rs) |
| 화면 | [React 앱](web/src/App.tsx), [메뉴](web/src/components/WorkspaceShell.tsx) |
| 설치 프로그램 | [Tauri 설정](src-tauri/tauri.conf.json), [빌드 스크립트](scripts/build-desktop.mjs) |
| TypeScript SDK | [클라이언트](packages/sdk-typescript/src/index.ts) |

### 대화와 작업 완료는 다릅니다

에이전트 작업은 Rust에서 권한·도구·모델·예산을 적용한 실행으로
변환됩니다. 도구 결과와 승인, 취소, 검증 결과는 SQLite에 남습니다.
**모델이 성공했다고 답한 것만으로 작업 완료를 판정하지 않습니다.**
일반 채팅은 외부 작업을 실행하지 않고 결과를 검증 완료로 표시하지 않습니다.
MCP 검색 후 도구를 호출하는 경우에도 현재 스키마와 실행·세션
권한을 재검증합니다.

### 대화 기억과 토큰 비용

기본 **1:1 채팅**은 별도의 [Context Engine](src/engine/chat/context_engine.rs)에서
기존 [원문·첨부파일·기억 저장소](src/db.rs)를 건드리지 않고 모델에 필요한 문맥만
구성합니다. 사용자 공통 지침은 유지하고 에이전트 페르소나는 에이전트 모드에서만
적용합니다. 기억·경험·도구 정보는 현재 요청에 필요한 항목만 선택합니다.

- **캐시 가능한 고정 접두사:** 같은 내용의 기억 스냅샷을 1회만 조립하고
  내용 수정·철회 시 명시적으로 작업 문맥을 갱신합니다. Codex WebSocket은
  이전 응답을 재사용할 수 있을 때만 `previous_response_id`를 전달합니다.
- **일반 대화 경량화:** 단순 인사에는 기존 작업 기록, MCP 전체 목록,
  무관한 스킬·성장 통계를 주입하지 않습니다. 최신 상태·날짜가 필요하지
  않은 요청에는 시계 정보도 추가하지 않습니다.
- **문맥 예산:** 모델별 최대 컨텍스트와 출력 예약량을 먼저 반영합니다.
  기본 1:1 채팅은 이전 기록 32KB, 작업 문맥 12,000토큰 상한을 사용하고
  중요한 최신 사용자 입력은 잘리지 않도록 보존합니다. 모델 공급자의
  실제 토큰 수와 내부 예측치는 구별합니다.
- **통합 사용량 기록:** 각 모델 호출과 보조 호출의 사용량을 세션별로
  집계하고, 공급자가 제공한 캐시 읽기·쓰기 토큰을 별도로 기록합니다.
  보고되지 않은 캐시 적중 수치는 0이 아닌 '알 수 없음'으로 취급합니다.
  별도 캐시 단가를 설정하면 예산 정산에 반영합니다.

실제 Rust 앱과 모의 모델을 연결한 [1:1 채팅 10/100턴 벤치마크](benchmark/PRIVATE-CHAT-WIRE-2026-11-20.md)에서
모델 API에 전송한 JSON 본문 누적량이 비제한 과거 입력 재전송 기준보다
10턴 **4.9%**, 100턴 **83.2%** 감소했습니다. 이 수치는 **전송 바이트 기준**이며
실제 상용 제공자의 과금 토큰·캐시 할인율을 대신하지 않습니다.

### 백엔드 부가기능 계층

도미니언의 **Rust 실행 커널**은 대화 원문·실행 저널·모델 호출·권한·검증을 담당합니다.
MCP 서버·장기기억·스킬·메신저/채널은
[별도 부가기능 호스트](src/addons/integrations/extensions/mod.rs)를 통해 필요할 때만 연결됩니다.
검색 가능한 도구 목록뿐 아니라 **실제 도구 실행 시점에도 부가기능 허용 여부를 다시 확인**합니다.
메모리 기능을 꺼도 대화 원문은 삭제되지 않으며, 메신저 기능을 꺼도 일반 채팅은 동작합니다.
기존 프론트 메뉴는 변경하지 않았습니다.

설정·검증 방법은 [2026년 백엔드 확장 아키텍처](docs/EXTENSION-HOST-2026-11-20.md)에 정리했습니다.
글로벌 `/api/extensions/runtime` 및 세션별 `/api/sessions/:id/extensions` API를 지원합니다.

### 실행기 개선 · 불필요한 모델 호출 절감

Codex WebSocket은 도구 목록·추론 설정 등 제공자 요청 계약이 바뀌면
이전 `previous_response_id`를 재사용하지 않고 새 연결에서 전체 문맥을
재구성합니다. **실제 도구 실행으로 완료 조건을 충족했다면 모델을
한 번 더 부르지 않고 검증 결과를 저장**합니다. 병렬 프로그램은
독립적인 읽기만 동시 실행하고, 셸 명령·파일 변경·의존성이 있는
분기는 순차 실행합니다. 체크포인트에도 실제 실행 모드를
처음부터 기록합니다. 기본 에이전트·그룹 문맥·실제 도구 실행을
GitHub CI에서 회귀 검사합니다.

9명 방의 대화 기록은 저장하되, 모델에 전송할 문맥은 각 참여자의
역할과 현재 질문·확정 상태를 기준으로 선택합니다. 제공자 캐시가
지원되면 고정 접두부의 재사용을 요청합니다. **실제 캐시 적중
토큰**과 하네스의 **재사용 추정치**는 분리해 기록하며,
독립 모델 호출이 동일한 내부 사고 상태를 유지한다고 가정하지 않습니다.

### 폐기 기능 정리

도미니언은 하나의 Rust 에이전트·SDK 실행 커널로 동작합니다.
영상 제작과 별도 조사 작업은 필요할 때 일반 MCP 도구로 연결합니다.
사용자의 로컬 DB와 파일은 코드 갱신 과정에서 자동 삭제하지 않습니다.

## 검증·배포

- [일반 CI](.github/workflows/ci.yml): Rust 회귀, 화면 계약,
  프런트 빌드 및 Windows 검사
- [네이티브 릴리즈](.github/workflows/release.yml):
  macOS ARM64·x64 및 Windows x64 테스트·빌드·배포
- [구조 검사](scripts/check-runtime-architecture.mjs),
  [버전·사이드카 검사](scripts/check-release-contract.mjs)

추가 문서: [시작 안내](docs/GETTING-STARTED.md),
[데스크톱](docs/DESKTOP.md), [엔진 구조](ENGINE.md),
[하네스](docs/HARNESS.md), [TypeScript SDK](packages/sdk-typescript/README.md).

**메인 브랜치**가 정식 개발 기준입니다. 라이선스는 MIT입니다.
