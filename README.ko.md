# DOMINION — 도미니온 서브2

<p align="center"><img src="src-tauri/icons/icon.png" alt="DOMINION" width="90" /></p>
<p align="center"><strong>내 모델, 내 작업 공간, 독립 Rust 에이전트 실행기</strong></p>
<p align="center"><strong>한국어</strong> · <a href="README.md">English</a> · <a href="ENGINE.md">엔진 구조</a> · <a href="LICENSE">Apache-2.0</a></p>

**2026년 11월 20일 기준 `dominion-sub2` 실험 브랜치** 설명입니다. React/Vite 프론트는 최신 main 화면(`f86a36e`)과 동기화하고, 백엔드는 서브2 고유의 Rust 하네스·대화 연속성·실행/검증 구조를 유지했습니다. main과 **백엔드가 동일하다는 뜻은 아닙니다.**

<p align="center">
  <img src="docs/01-multi-agent-room.webp" alt="다중 에이전트 방" width="48%" />
  <img src="docs/02-chat-workspace.webp" alt="에이전트 채팅" width="48%" />
</p>

## 주요 기능

- **Rust 단일 실행기:** 도구 호출, 권한·승인, 실행 영수증, 취소, 복구 및 결과 검증.
- **멀티 에이전트 대화방:** SQLite 원문 보존, 참가자별 독립 기억, 확정된 사용자 상태, 동료 발언 공유, 호출별 토큰 사용량.
- **최신 main 프론트:** 모델 관리·허깅페이스 탐색, 제공자 선택, 연결 계정, 대화방/채팅, 범위별 새로고침.
- **API 키 없는 공개 모델 탐색:** `/api/providers/catalog/:id/models`, `/api/providers/openrouter/free-models`에서 읽기 전용 목록 조회. 실제 추론 권한과 분리.
- **GitHub CLI 계정 연결:** 화면의 로그인 흐름을 백엔드에 연결하고 사용자 승인 후 계정 검증·암호화 저장.
- **DOMINION Cloud:** 구글·카카오·네이버 인증, 무료 모델 조회, 사용자별 구독·본인확인·결제 연동. 로그인 성공을 모델/요금 조회 장애와 분리하고, 결제 승인 중 통신이 끊기면 주문번호 대조를 통해 복구합니다. 실결제·상용 정산 검증은 별개입니다.
- **도구/SDK:** 파일·셸·브라우저·MCP, `sdk/typescript/` TypeScript 클라이언트와 `crates/dominion-agent-kernel/` Rust 커널.
- **단계적 도구 탐색(2026-11-20):** Rust 단일 실행 루프에서 필요한 MCP·네이티브 도구만 노출합니다. 검색 결과는 이름·짧은 설명만 보내고, 실제 입력 스키마는 다음 모델 라운드에 원본 그대로 제공해 중복 JSON 전송을 줄입니다. 회귀 테스트를 CI에 등록했으며 실제 모델 비용 절감률은 아직 측정하지 않았습니다.

## 개발 및 테스트

필요 환경: Rust stable, Node.js 24.12 이상.

```bash
npm ci --prefix web
npm --prefix web run typecheck
node --experimental-strip-types --test web/tests/*.test.mjs
node scripts/check-sub2-contract.mjs
node scripts/sub2-source-audit.mjs
cargo test --lib
cargo run -- --home .
```

UI 개발 서버: `npm --prefix web run dev`. 데스크톱 실행/설치, 실제 계정 인증, 실모델 9명 대화 비용은 각각 실환경 검증이 필요한 항목입니다.

## 핵심 코드

| 기능 | 실제 경로 |
| --- | --- |
| 최신 프론트와 페이지 | `web/src/App.tsx`, `web/src/pages/`, `web/src/components/` |
| API·공개 모델 목록·GitHub 연결 | `src/api/mod.rs`, `src/api/services/providers/providers.rs`, `src/api/services/providers/cli_oauth.rs` |
| 에이전트 실행·도구 경계 | `src/engine/services/conversation/chat/runtime.rs`, `src/engine/services/conversation/chat/runtime/turn_body.rs` |
| 대화 비용·기억 연속성 | `src/engine/services/conversation/chat_room_context.rs`, `src/engine/services/conversation/chat_continuity.rs` |
| 방 상태와 발언자 선택 | `src/engine/services/conversation/room_state.rs`, `src/engine/services/conversation/rooms.rs` |
| 실행 작업·지속 저장 | `src/engine/services/automation/jobs.rs`, `src/db.rs` |
| SDK/커널 | `sdk/typescript/src/index.ts`, `crates/dominion-agent-kernel/src/lib.rs` |

[엔진 구조](ENGINE.md) · [문서 목차](docs/INDEX.md) · [서브2 CI](.github/workflows/ci.yml)

## 검증 범위

개발용 CI는 Rust 회귀 테스트, TypeScript 검사, UI 단위 테스트, 백엔드 API/잔재·중복 점검을 수행하며 Vite 프로덕션 빌드는 별도 릴리스 과정으로 분리합니다. **실제 브라우저 클릭·사용자 계정 로그인·맥/윈도 설치 실행·9명×3턴 실모델 비용**은 별도의 통합 측정으로 평가합니다. 프롬프트 캐시의 재사용과 실제 과금 절감량은 구분합니다.

## 2026-11-20 서브2 중복·잔재 전수 점검

2026-11-20 코드 정리에 앞서 Git 트리의 파일과 SHA를 점검하고, 실행 코드·SDK·클라우드·테스트·워크플로의 활성 파일을 `scripts/sub2-source-audit.mjs`로 검사했습니다. 정적 API 경로 **219개(컬렉션 자동 생성 8개 포함)**의 충돌과 파일 전체 해시·서로 다른 파일의 긴 동일 코드 블록을 검사합니다. 현재 감사 기준에서는 실행 파일 전체의 완전 동일 복제와 긴 동일 블록 후보가 0건입니다. **의미만 같은 코드까지 자동으로 증명한다는 뜻은 아닙니다.**

- **API:** 더 이상 호출되지 않는 공통 컬렉션 `/:id/:action` 4개 라우트, 도달 불가능한 액션 핸들러·매핑 삭제. 활성 방 실행 API `/api/rooms/:id/run`과 스킬 `GET /api/skills/:id/export`는 유지합니다.
- **모델 파일:** GGUF·MLX의 중복 재귀 탐색을 하나로 통합. 관리형 GGUF를 외부 경로로 다시 등록하더라도 목록에는 1개만 표시하며 기존 외부 ID·DB 기록을 유지합니다. 기존 심볼릭 링크와 설치 파일 검색 경로를 보존합니다.
- **검증:** Rust 파일 탐색·모델 중복 등록 회귀 테스트와 프론트·계약·중복 감사를 CI에 편입했습니다. 샘플만 검사한 테스트와 실기기·실모델 실행 검증은 구분합니다.

### 구형 실행 경로 제거 (2026-11-20)

현재 사용하지 않는 영상 생성·업로드, 독립적 자기개선 캠페인, 과거 목표 제어용 Rust 모듈과 HTTP 경로를 삭제했습니다. 별도 실험 프로세스를 주기적으로 시작하던 환경변수 기반 스케줄러, 자동 실험 요청 예약 함수 및 스케줄러 상태 API도 제거했습니다. 네이티브 Rust SDK·TypeScript SDK·OAuth·도구 라우팅·번역·테스트와 구형 개발 문서의 참조도 함께 제거했습니다. 제품에서 계속 쓰는 일반 작업·코드 인덱스·검증·복구·MCP 도구는 유지합니다. 과거 사용자의 SQLite 기록과 파일은 자동 삭제하지 않습니다.

`scripts/check-sub2-contract.mjs`는 구형 실행 경로의 재유입을 막고, `scripts/sub2-source-audit.mjs`는 활성 파일과 API 경로의 중복을 확인합니다. GitHub Actions에서 점검하지만 실제 모델·외부 계정·결제 검증은 별도입니다.

## 자연어 실행 코어 최소화 (2026-11-20)

자연어→실행→결과의 흐름은 `src/engine/core/`에 두고, 모델 선택·메모리·멀티에이전트·자동화·판단·파일 도구 구현은 `src/engine/services/`로 분리했습니다. MCP·외부 채널은 `src/engine/addons/`에서 처리합니다. 기존 메뉴·Rust 모듈 이름·API 주소는 유지됩니다. [코어 분리 설계](docs/SUB2-MICROKERNEL-2026-11-20.md)를 참고하세요.

HTTP API 역시 실행·승인·이벤트 11개 경로만 코어에 두고 제품 기능 123개는 서비스 라우터 5개로, 외부 연동 27개는 부가기능 라우터로 분리했습니다. 기존 주소와 공통 인증·보안 미들웨어는 유지합니다.

최소 실행 코어를 독립 Rust 크레이트 `crates/dominion-turn-core/`로 분리했습니다. 코어는 모델·도구 실행 단계, 서비스 실행 포트, 실행 결과 영수증만 소유하며 모델 연결·메모리·계획·자동화·MCP·UI는 외부 서비스입니다. 기존 `src/engine/core/mod.rs`는 호환 재내보내기만 합니다.

2026-11-20: 실행 코어와 행동 알고리즘 분리를 강화했습니다. 순수 전략 코드는 `crates/dominion-agent-kernel/src/decision_policy.rs`, 활성 정책 버전 선택은 `src/engine/services/decision/turn_policy.rs`, 실행 제어는 `crates/dominion-turn-core/`가 담당합니다. 기존 메뉴와 API는 유지됩니다.

루프 알고리즘은 기존 `agent.loop` 정책 슬롯을 통해 `react`와 `plan-execute`를 코드 변경 없이 선택할 수 있습니다. 알 수 없는 정책 이름은 기존 자동 선택으로 복귀하며 코어 권한에 영향을 주지 않습니다. UI·새 메뉴 추가 없음.
