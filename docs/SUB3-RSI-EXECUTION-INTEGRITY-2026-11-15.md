# Dominion Sub3 — 실제 RSI 실행 경로와 평가·잔재 정합성 (2026-11-15)

## 목표
새 기능 숫자를 늘리기보다 이미 존재하는 RSI 코드가 **실제로** 소스를 개선·검증·계승할 때 거짓 성공이나 오염된 증거가 통과하지 못하도록 한다. 모델 실행기는 Rust Dominion 자체 코드가 담당하고, Node는 별도의 동결 A/B 검증 도구로만 사용한다.

## 현재 소스 분석과 실행 경로

`POST /api/rsi/start` → `src/api/rsi.rs::start` (소스 브랜치·커밋/모델/동결 평가 계획 검증) → `jobs::enqueue("rsi_evolve")` → `rsi_evolve::run` (실제 Git worktree 생성·소스 스냅샷) → `cargo build --release --locked` (후손 Rust 바이너리 생성) → `--rsi-worker-identity` (실제 세대 커밋) → `--rsi-worker-job` (별도 Rust 프로세스) → `jobs::run_prompt` (도구를 써서 격리된 소스 편집) → `rsi_worktree::finish` (실제 Git diff/컴파일 진단/후보 등록) → `rsi_worktree::compare_at` (동일 기준 A/B Git worktrees, 원본 동결 사례, 실제 Rust 빌드 및 테스트, 메타 평가) → `rsi::replayed_pair_metrics` (원시 영수증과 계산 재검산) → `rsi_evolve::adopt` (후속 생성기 Git 트리 채택) → 다음 후손 → `rsi_worktree::apply_campaign_to_sub` (명시적 브랜치에 fast-forward).

이 구조는 실제 코드상 존재하지만, 유료 모델 API를 이용한 **연속 RSI 세대 자동 완주·성능 향상은 이번 검증의 범위가 아니다**. 단위 테스트 통과로 프런티어 성능을 주장하지 않는다.

## 이번에 제거한 결함

1. **Git 트리 지문 누락**: 기존에는 모델 패치 SHA와 평가 JSON을 확인했지만 실행된 코드의 전체 Git tree SHA를 독립 재구성하지 않았다. `scripts/rsi-lab.mjs`가 동결 oracle 주입 전 baseline 및 candidate Git 트리를 기록하고 `scripts/rsi-measure.mjs`가 그 정체성을 v3 평가 보고서에 서명된 증거 범위로 포함한다. `rsi_worktree::reconstructed_patch_tree`가 별도의 `GIT_INDEX_FILE`을 사용해 기준 커밋+패치로 동일한 후보 트리를 재구성한다. 비교 결과의 트리와 어긋나면 유료/고비용 A/B 빌드 전 거부하며, `rsi::replayed_pair_metrics`, `rsi_evolve::adopt`, `apply_campaign_to_sub`에서도 같은 정체성을 확인한다. 기존 v2 영수증은 읽을 수 있지만, v3 트리 지문이 없는 결과는 신규 자동 발행을 통과하지 못한다.
2. **후손 작업 거짓 완료**: `rsi_evolve::worker`는 `run_prompt`가 `Ok(value)`를 반환한다는 이유만으로 `status=completed`를 썼다. 실제 `needs_review`, `failed`, `cancelled` 판정은 그대로 보존하고 불명/진행 중 상태는 검토 대기로 변환한다. 부모의 세대 기록에는 실제 컴파일/편집 결과가 남는다.
3. **초기 Git worktree 잔재**: `rsi_evolve::run`에서 원본 스냅샷 생성이 실패하면 정리 없이 이탈했다. `cleanup_failed_lab`는 Git 등록 정보를 확인한 뒤 Git의 `worktree remove --force`를 수행하며 디스크 폴더를 정리한다. 실패 시 감사 이벤트와 `needs_review` 원인을 남기고 무턱대고 다른 세대를 시작하지 않는다.
4. **A/B 실행기 잔재 묵살**: Node A/B 실행기의 `finally`는 실패한 worktree 제거를 무시하고 디렉터리를 지워 Git metadata만 남길 수 있었다. 이제 등록된 worktree들을 확인한 후 모두 제거하고, 삭제 실패는 **실패로 보고하며 현장 데이터를 보존**한다.

## 검증 계획

- 실제 로컬 Git 저장소에서 Rust가 불변 원본·패치로 후보 트리를 독립 재구성하고 소스 index·HEAD를 건드리지 않는지 검사한다.
- 후손 작업의 `needs_review`, `failed`, `cancelled`가 `completed`로 둔갑하지 않는지 검사한다.
- 초기 생성 실패에 실제 분리 Git worktree 제거와 Git 등록 정보 정리가 뒤따르는지 검사한다.
- Node 실행기가 유니코드·공백이 든 디렉터리에서 변경본을 만들고, 잘못된 패치 실패 후 Git worktree를 남기지 않는지 검사한다.
- **실제 최소 Rust Cargo 프로젝트**의 고장 난 테스트와 올바른 수정 후보를 별도로 빌드하고, 동일 평가 기준에서 A/B가 서로 다른 실제 테스트 결과를 기록하는지 검사한다.
- 단위 테스트 결과와 실제 외부 모델로 진행한 긴 자율 캠페인 완료는 분리해 보고한다.

## 2026 원천 설계와 독립성

검토 대상: [OpenAI Codex Rust 실행기](https://github.com/openai/codex), [Qwen Code](https://github.com/QwenLM/qwen-code), [OpenClaw](https://github.com/openclaw/openclaw), [Hermes Agent](https://github.com/NousResearch/hermes-agent), [DeepSeek 오픈소스 모델·에이전트 연구](https://github.com/deepseek-ai), [Anthropic Claude Code](https://github.com/anthropics/claude-code). 이들은 세션/도구/계획/실행 환경/검증 설계를 검토하는 참고 시스템이며 Dominion의 Rust 계획·승인·RSI 의사결정 실행기를 외부 에이전트 SDK로 대체하지 않는다. 모델 가중치 재학습 없이 **소스 수정·독립 검증·차세대 실행**에 집중한다.

## 잔재 감사

작업 브랜치 Git 전체 트리(373개 추적 파일, recursive response 완전)를 조사했다. 추적된 `rsi-labs/`, `rsi-paired/`, `node_modules/`, `target/`, 임시 patch, `*.rej`, `*.orig`는 없었다. 런타임 파일의 누출 여부는 실패 경로 테스트와 실행 후 Git 등록 확인으로 검증한다. 실제 호스트의 과거 남은 비추적 디렉터리는 연결된 실행 컴퓨터를 직접 조사해야 판단 가능하며, 이 문서만으로 정리됐다고 주장하지 않는다.

**CI는 위 잔재 검사와 코드 정리 후에만 수행한다.** 작업 성공률·모델 청구 비용·SWE-bench는 이후 동일 조건으로 별도 측정한다.


## 검증 결과 — 2026-11-15

[집중 GitHub Actions #43697949313](https://github.com/hushmind3/dominion-rust/actions/runs/43697949313)에서 **Rust 총 223/223**, 실제 Node·Git·Cargo 기반 RSI A/B **2/2**, 독립 TypeScript SDK **7/7** 테스트가 통과했다. React TypeScript 타입 검사와 Vite 프로덕션 빌드도 통과했다.

Rust 집계: 독립 에이전트 SDK 45, 네이티브 호스트·전송 67, `engine::rsi::tests` 9, `engine::rsi_worktree::tests` 11, `engine::rsi_evolve::tests` 71, `api::rsi::tests` 4, Codex WebSocket 15, 워크플로 복구 1 = **223**. `rsi_evolve` 테스트에는 실제 Rust 후손 바이너리 재컴파일·실행·기존 소스 보호·검증된 Git 계보 채택 및 선택된 브랜치 반영이 포함된다.

Node A/B는 정상·실패 시 Git worktree 정리, 유니코드 경로, 실제 Cargo 프로젝트의 **원본 회귀 테스트 실패 / 수정본 통과 / 양쪽 공통 보호 테스트 통과**, v3 동결 기준 Git 트리 기록을 확인한다. 프로토콜·원화 청구·상용 모델로 자동 세대 반복까지 입증한 수치는 아니다.

기준 코드 커밋: `339bed1947e4c8ee6e67657f10c5dc0f3e760c1a`; 이후 문서 수정은 실행 소스/테스트를 변경하지 않는다. 전체 제품 릴리스 CI, 유료 모델 장기 RSI, SWE-bench는 아직 완주 증거가 없다.
