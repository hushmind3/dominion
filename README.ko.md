# DOMINION

**내 모델로. 내 컴퓨터에서. 내 방식대로.**

<p>
  <img src="src-tauri/icons/icon.png" alt="DOMINION" width="80" />
</p>

Rust 실행 엔진과 Tauri 데스크톱 앱으로 이루어진 오픈소스·로컬 우선 AI 에이전트 워크스페이스입니다.
직접 가진 API 키나 로컬 모델을 연결하면, 도구를 쓰는 에이전트가 코드를 읽고 고치고, 명령을 실행하고,
웹을 검색하고, MCP 서버를 호출하고, 여러 에이전트가 한 대화방에서 함께 일합니다.
파일 쓰기와 셸 실행은 전부 승인을 거치고, 모든 상태는 내 컴퓨터의 SQLite에 남습니다.

[English](README.md) · [한국어](README.ko.md)

[![License: Apache 2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
![Engine: Rust](https://img.shields.io/badge/engine-Rust-262626)
![Desktop: Tauri 2](https://img.shields.io/badge/desktop-Tauri%202-24C8DB)
![UI: React](https://img.shields.io/badge/UI-React-149ECA)
![Models: BYOK + local](https://img.shields.io/badge/models-BYOK%20%2B%20local-6855D8)

[스크린샷](#스크린샷) · [주요 기능](#주요-기능) · [모델](#모델) · [설치와 빌드](#빠른-시작) ·
[구조](#구조) · [현재 상태](#현재-상태) · [토론](https://github.com/hushmind3/dominion-rust/discussions)

---

## 스크린샷

<!-- 게시 전 TODO: 현재 빌드로 다시 캡처하세요. 첫 이미지에는 이미 없어진 "영상 제작" 메뉴가 남아 있고
     Money 메뉴가 없어서, 지금 내비게이션보다 오래된 화면입니다. -->

**멀티 에이전트 대화방 — 여러 에이전트가 한 대화를 공유하고, 발화자별 토큰 사용량이 표시됩니다**

<p>
  <a href="docs/01-multi-agent-room.webp">
    <img src="docs/01-multi-agent-room.webp" alt="DOMINION 멀티 에이전트 대화방: 참가자, 공유 대화, 대화방 설정" width="100%" />
  </a>
</p>

| 채팅 워크스페이스 | 에이전트 스튜디오 |
| --- | --- |
| [<img src="docs/02-chat-workspace.webp" alt="DOMINION 채팅 워크스페이스와 모델 선택" width="100%" />](docs/02-chat-workspace.webp) | [<img src="docs/03-agent-studio.webp" alt="DOMINION 에이전트 스튜디오: 에이전트 카드, 역할, 모델 설정" width="100%" />](docs/03-agent-studio.webp) |
| 모델을 고르고 작업을 맡깁니다. | 에이전트마다 역할·지시·모델을 따로 줍니다. |

<sub>한국어 UI의 개발 빌드에서 캡처했습니다. 화면을 보여주는 것이며, 실모델 안정성을 보증하는 자료는 아닙니다.</sub>

## 주요 기능

| 기능 | 실제로 하는 일 |
| --- | --- |
| **내 모델, 내 키** | OpenAI·Anthropic·Gemini·DeepSeek·Mistral·xAI·OpenRouter·Groq·Cerebras·Together·Perplexity·GitHub Models 등 30개가 넘는 프리셋. DOMINION 계정도, 토큰 재판매도 없습니다. |
| **로컬 모델** | Ollama, LM Studio, llama.cpp(`llama-server`, 분할 GGUF 포함), vLLM, LiteLLM, macOS의 MLX. 모델 다운로드·설치도 **모델 관리** 안에서 끝납니다. |
| **구독 로그인 활용** | 이미 쓰고 있는 ChatGPT·CLI 요금제를 그대로 쓸 수 있습니다. Codex ChatGPT OAuth, `claude`·`gemini`·`opencode` CLI 로그인을 추론 경로로 연결합니다. 도구·승인·기록은 여전히 DOMINION이 소유하고, CLI는 글 생성만 담당합니다. |
| **도구를 쓰는 에이전트** | 워크스페이스·절대경로 파일 도구, SHA-256로 검증하는 줄 단위 편집, 셸·터미널 실행, 웹 검색, HTTP 가져오기, 헤드리스 Chrome 제어, GitHub 읽기/커밋, MCP 도구, 스킬. |
| **승인과 실행 증거** | 쓰기·셸·브라우저·GitHub 커밋은 승인 요청을 띄웁니다. 취소된 배치는 호출을 조용히 버리지 않고 중단 영수증을 남기고, 결과가 불확실한 외부 변경은 검토 대기로 멈춥니다. |
| **durable job** | 한 번의 턴이 하나의 작업이며, 체크포인트·취소·복구가 SQLite에 남습니다. 저장된 도구 영수증이 완료 조건을 모두 채우면 모델을 한 번 더 부르지 않고 끝낼 수 있습니다. `POST /api/sdk/runs`에 `idempotencyKey`를 주면 재시도해도 두 번 실행되지 않고 원래 작업이 반환되며(인수가 바뀌면 409), 실행도 두 번 되지 않습니다. |
| **멀티 에이전트 대화방** | 참가자 최대 32명, 발언 순서, 멘션, 공유 지식, 발화자별 프롬프트·캐시 토큰 집계. 에이전트끼리 격리된 Git 워크트리와 역할 프로필(`general`·`inspection`·`implementation`·`verification`)로 작업을 넘길 수도 있습니다. |
| **기억** | 로컬 SQLite 기억. 임베딩 서비스도 외부 메모리 서버도 쓰지 않습니다. 기본 재정렬기 `evidence-lineage-2026`은 검증 가능한 출처를 기준으로 후보를 정렬하고, 사실을 저장할 때는 사용자의 실제 문장 인용(`evidenceQuote`)을 요구하므로 지어낸 기억은 거부됩니다. |
| **첨부** | 이미지(PNG/JPEG/GIF/WebP)나 UTF-8 텍스트 파일(최대 8MiB)을 채팅에 붙일 수 있습니다. 로컬에 보관되고 엔진이 다시 내려줍니다. |
| **메신저 연결** | Telegram 봇 폴링과 Discord 어댑터. 둘 다 내 봇 토큰과 대상 ID를 직접 설정해야 합니다. |
| **Money 조사 도구(기본 꺼짐)** | 공개 RSS·GitHub·Hugging Face 신호를 모델 토큰 없이 로컬에서 수집하고, 출처가 붙은 사업 가설로 정리합니다. **설정 → Money 도구**에서 켭니다. |

## 모델

OpenAI 채팅 완성 API를 말하는 것이면 무엇이든 됩니다. 프리셋을 고르거나 base URL을 직접 넣으면 되고,
Anthropic은 자체 경로로 지원합니다. 로컬 추론은 런타임을 한 번 설치하면 디스크의 모델을 찾아 씁니다.

- **Ollama** — `ollama serve` 실행 후 로컬 프리셋 추가
- **LM Studio / vLLM / llama.cpp / LiteLLM** — 각 로컬 서버 포트에 연결(LM Studio 1234, llama.cpp 8080, vLLM 8000, LiteLLM 4000)
- **MLX** — macOS 전용, 모델은 앱 데이터 폴더로 내려받습니다

클라우드 모델 요금은 선택한 제공자에게 청구되고, 로컬 모델은 내 하드웨어를 씁니다.
키는 로컬 키 보관소에 ChaCha20-Poly1305로 봉인되며, 저장 후에는 앱이 다시 보여주지 않습니다.

## 빠른 시작

**macOS 13.3+** 와 **Windows**(WebView2)를 지원합니다. Linux는 아직 지원 대상이 아닙니다.

**1. 배포된 데스크톱 앱 설치.** [Releases](https://github.com/hushmind3/dominion-rust/releases/latest)에서 최신 설치 파일을
받습니다. 현재는 **v0.4.3**입니다.

| 플랫폼 | 파일 |
| --- | --- |
| macOS, Apple Silicon | `DOMINION_0.4.3_aarch64.dmg` |
| macOS, Intel | `DOMINION_0.4.3_x64.dmg` |
| Windows, 설치 프로그램 | `DOMINION_0.4.3_x64-setup.exe` |
| Windows, MSI | `DOMINION_0.4.3_x64_en-US.msi` · `DOMINION_0.4.3_x64_ko-KR.msi` |

서명하지 않은 빌드라서 첫 실행 때 macOS Gatekeeper와 Windows SmartScreen이 한 번 확인을 요청합니다.
앱의 SQLite DB·키·로그·작업 워크스페이스는 OS의 앱 데이터 폴더에 저장되며, 설치된 앱 번들 안에는 아무것도 쓰지 않습니다.

**2. 데스크톱 앱 직접 빌드(Tauri).** 실제 제품입니다. 창이 Rust 엔진을 사이드카로 함께 띄웁니다.

```
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
npm ci --prefix web
node scripts/build-desktop.mjs
```

준비물: Git, Rust stable과 Cargo, Node.js **24.12 이상**, OS별 C/C++ 빌드 도구
(Windows는 MSVC의 C++ 데스크톱 워크로드와 Windows SDK). 결과물은 `src-tauri/target/release/bundle/`에
생깁니다. macOS는 `macos/DOMINION.app`과 DMG, Windows는 MSI와 NSIS 설치 파일입니다.
서명·데이터 위치·사이드카 동작은 [docs/DESKTOP.md](docs/DESKTOP.md)에 정리되어 있습니다.

**3. 브라우저 모드(엔진 + Vite 번들).** 개발 중 빠르게 돌려볼 때:

```
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home . --open
```

그다음 <http://127.0.0.1:4317> 을 엽니다.

### 처음 5분

1. **모델 관리** → 클라우드 제공자에 내 키를 넣거나 로컬 런타임을 추가합니다.
2. **채팅** → 모델을 고르고 작업을 맡기면, 도구 호출과 승인이 대화 안에 표시됩니다.
3. **에이전트** → 역할과 지시를 주면 그 에이전트만의 기억과 모델을 갖습니다.
4. **그룹 대화** → 여러 에이전트를 한 방에 넣고 같은 과제를 토론시킵니다.
5. **연결 · 채널** → Telegram·Discord 봇을 붙이면 휴대폰에서도 에이전트와 대화할 수 있습니다.

자세한 안내: [설치와 첫 사용](docs/GETTING-STARTED.md) · [데스크톱 빌드](docs/DESKTOP.md) · [계정 연결](docs/ACCOUNT-CONNECTIONS.md).

## 구조

```
React + Vite UI  ──  Tauri 2 데스크톱 셸 (macOS / Windows)
        │                        │
        ▼                        ▼
Rust HTTP API (Axum, 127.0.0.1)  ──►  SQLite (상태·기억·작업·영수증·로그)
        │
        ▼
네이티브 Rust 에이전트 엔진 (crates/dominion-core + 제품 어댑터)
        ├── 채팅, 스트리밍, 도구 호출, 승인, 취소, 복구
        ├── 기억, 경험, 대화방, 위임 작업, 검증
        ├── 스킬, MCP(stdio + Streamable HTTP), Telegram, Discord
        └── 모델 어댑터: 클라우드 API, 로컬 런타임, CLI 로그인
```

에이전트 런타임은 Rust이고, Node는 화면을 빌드할 때만 씁니다. 채팅·대화방·예약 작업·HTTP SDK가 모두
같은 엔진과 같은 SQLite 저널을 지나므로, 따로 생각해야 할 두 번째 런타임이 없습니다.

### 실행 루프를 돌리는 것

에이전트는 검색 점수를 감싼 프롬프트가 아닙니다. `crates/dominion-core`가
**초기화 → 필요 시 계획 → 모델 호출 → 도구 승인·실행 → 검증 → 종료**의 제한된 루프를 소유하고,
[`src/engine/chat/product.rs`](src/engine/chat/product.rs)가 그것을 모델 제공자·작업공간·승인·SQLite 영수증에 연결합니다.

| 계층 | 저장소에서 확인할 수 있는 것 |
| --- | --- |
| **커널과 안전** | 단일 Rust 실행 루프, 도구 의도·결과 기록, 공유 토큰·비용 예산. 취소·미승인·미확정 외부 효과가 남아 있으면 완료 처리되지 않습니다. |
| **계획** | 실제 실행 가능한 알고리즘을 버전별로 등록하고, 작업 분해에는 `acceptance-lineage-dag-2026`을 씁니다. 과거 실험 기록은 이력일 뿐, 스스로 코드를 고치는 엔진이 아닙니다. |
| **검증과 복구** | 동결된 완료 조건 검사, 자원별 의무 계보, 영수증 기반 복구, 제한된 인과 진단. 모델이 "끝났다"고 말하는 것만으로는 작업이 완료되지 않습니다. |
| **기억과 문맥** | 로컬 SQLite 코퍼스 위의 `evidence-lineage-2026` 재정렬, 사용자 확정 대화방 상태, 출처 범위 그룹 문맥. BM25는 후보를 고르는 보조 커널로만 남아 있고 하네스 정책이 아닙니다. |
| **토큰 재사용** | 제공자별 요청 조립, Codex 증분 WebSocket, 검증된 접두부를 유지하고 새로 선택된 사용자 사실만 보내는 누적형 대화방 투영. 정정·폐기·지시 변경이 있으면 다시 구성합니다. |

여기서 알아 둘 두 가지:

- **검증 후 종료.** 실제 도구 영수증이 완료 조건을 이미 채웠다면, 모델을 한 번 더 부르지 않고 끝낼 수 있습니다.
  다만 귀속 가능한 영수증, 미확정 효과 없음, 최신 사용자 지시가 모두 확인돼야 합니다.
- **접두부 재사용은 캐시 적중이 아닙니다.** 프롬프트·캐시된 프롬프트·신규 프롬프트 토큰은 실행별로, 그것도 제공자가 보고한 값만 기록합니다.

**TypeScript 클라이언트:** [`packages/sdk-typescript`](packages/sdk-typescript/README.md)는 그 엔진을 부르는 얇은 HTTP
클라이언트이며 별도 하네스가 아닙니다. `waitForRun`은 완료·승인·수동 복구 경계에서 멈추고,
`waitForVerifiedRun`은 독립 검증 증거가 붙은 `taskOutcome: "succeeded"`만 성공으로 돌려줍니다.

**더 깊은 문서:** [엔진](ENGINE.md) · [하네스 계약](docs/HARNESS.md) ·
[이 README에서 옮긴 구현 메모](docs/HARNESS-DETAIL-2026-11-18.md) · [문서 목차](docs/INDEX.md).

## 에이전트가 할 수 있는 일, 없는 일

읽기·검색·상태 확인은 바로 실행되고, 무언가를 바꾸는 일은 먼저 물어봅니다.

| 승인이 필요 | 묻지 않고 실행 |
| --- | --- |
| `shell_exec`, `terminal_exec` | `workspace_list`, `workspace_read`, `workspace_read_range`, `code_search` |
| `workspace_write`, `workspace_replace`, `file_write` | `web_search`, `http_fetch` |
| `browser_control`(열기·클릭·입력·스크린샷) | `memory_search`, `skill_view`, `task_status`, `task_result` |
| GitHub 커밋과 `github_api` 쓰기 메서드 | `workspace_git_status`, `workspace_git_diff`, `github_file_read` |

위임받은 하위 작업은 부모의 권한을 물려받을 뿐 넓히지 못하고, 토큰·비용 예산을 함께 씁니다.
검증 프로필의 하위 작업은 격리된 워크트리에서 승인된 Cargo 테스트만 돌릴 수 있습니다.

## 측정한 것과 측정하지 않은 것

이 저장소에 있는 측정은 두 개이고, 어느 것도 제품 전체의 벤치마크가 아닙니다.

**기억 전달 여부(2026-11-06).** 합성 기억 21개를 넣고 무료 API 모델과 로컬 3B 모델에 각각 3문항을 물었을 때,
재정렬 커널이 없으면 정답 기억이 모델에 **0/6회** 전달됐고 당시 기본이던 BM25 커널을 쓰면 **6/6회** 전달됐습니다.
답 정확도는 여전히 들쭉날쭉했고 문서에도 그렇게 적혀 있습니다. 이 실험은 실험 브랜치
(`codex/native-memory-harness`)에서 BM25 커널로 측정한 것이며, 지금 기본 재정렬기는 `evidence-lineage-2026`이고
그 재정렬기에 대한 같은 측정은 아직 없습니다.
[docs/REAL-MODEL-EXPERIMENT-2026-11-06.md](docs/REAL-MODEL-EXPERIMENT-2026-11-06.md) 참고.

**그룹 대화의 비용과 회상(2026-11-09).** 9인·100턴 실행
[`ROOM-100-TURN-LUNA`](benchmark/ROOM-100-TURN-LUNA-2026-11-09.json)는 제공자 캐시 재사용 **18.15%** 와
장기 기억 회상 실패를 실험 브랜치에서 기록했습니다. 그 뒤의 재사용 작업(검증된 대화방 상태의 누적 투영,
Codex 증분 전송)은 회귀 테스트만 있고, 새 캐시 적중률·회상 향상·비용 절감을 측정한 결과는 없습니다.

그 밖의 숫자는 자랑거리가 아닙니다. 공개한 토큰 절감률도, 자기개선 주장도, 이 컴퓨터에서 측정하지 않은
품질 수치도 없습니다.

## 선택 기능: Money 조사 도구

새로 설치하면 꺼져 있습니다(**설정 → Money 도구**). 공개 RSS·GitHub·Hugging Face 신호를 모델 토큰 없이
인덱싱된 SQLite에 모으고, 출처 페이지 버전을 내용 해시로 고정 보관하며, 모델이 제안한 인용문을 보관된
본문과 대조합니다. 조사는 외부 연락·지출·구현을 승인하지 않으며, 그 영역은 이 도구의 범위 밖입니다.
자세한 내용은 [docs/MONEY.md](docs/MONEY.md).

## 현재 상태

DOMINION은 **활발히 개발 중인 프리뷰**이며, 관리형 서비스도 검증된 상용 제품도 아닙니다.

- **설치 파일은 서명돼 있지 않습니다.** v0.4.3부터 macOS DMG와 Windows MSI/NSIS 설치 파일을 배포하지만,
  Apple·Windows 서명 인증서가 없어 첫 실행 때 직접 확인해야 합니다. 그 이전에는 공개된 릴리스가 없었습니다.
- **macOS와 Windows만 지원합니다.** 엔진 자체는 이식 가능하지만 CI와 설치 파일은 두 OS를 대상으로 합니다.
- **CI는 전체 검증이 아닙니다.** 일반 푸시에서는 엔진·크레이트나 지정된 웹 파일이 바뀔 때만 macOS에서 일부 Rust
  회귀 테스트가 돕니다. 프런트 빌드·릴리스 프로파일 테스트·두 OS 설치 파일은 태그나 수동 실행 때 `release.yml`에서만
  검사합니다. 초록 체크가 "배포 가능한 빌드가 검증됐다"는 뜻은 아닙니다.
  [Actions](https://github.com/hushmind3/dominion-rust/actions) 참고.
- **실모델 장시간 안정성은 미검증입니다.** 대화방·위임 경로는 모의 모델 테스트로 덮여 있지만, 몇 시간짜리 실모델 운용은 검증되지 않았습니다.
- **채널은 수동 설정입니다.** Telegram·Discord는 내 봇 자격증명이 필요하고, 원클릭 봇 생성은 없습니다.
- **은퇴한 기능.** 내장 영상 생성과 자율 RSI 캠페인은 종료됐습니다. 보관된 기록은 읽기 전용이며
  (`GET /api/rsi/*`는 `retired: true` 반환), 미디어 생성이 필요하면 MCP 도구를 쓰세요.
- **승인은 설계입니다.** 모든 변경은 먼저 물어봅니다. 사람 없이 몇 시간씩 도는 자율 운용은 목표가 아닙니다.

## 참여하기

가장 도움이 되는 것은 재현 가능한 버그 리포트, 테스트 케이스, 모델·제공자 호환성 수정, Rust 엔진 개선,
UI 작업, 문서입니다.

- [토론](https://github.com/hushmind3/dominion-rust/discussions) — 사용 환경, 질문, 워크플로 아이디어
- [이슈](https://github.com/hushmind3/dominion-rust/issues) — OS, 모델·제공자, 재현 절차, 기대 결과와 실제 결과
- [풀 리퀘스트](https://github.com/hushmind3/dominion-rust/pulls) — 변경 이유와 필요한 테스트를 함께
- API 키·봇 토큰·개인 대화를 이슈에 붙이지 마세요.

## 라이선스

[Apache 2.0](LICENSE) · [English README](README.md)
