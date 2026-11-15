# 서브3: 현재 메인 기능 보존과 실행 오류 보강 (2026-11-15)

비교 기준: `main` (`a6c12ccf85c55ecf4ea46e33a7f4f9027bb36e9f`) 및 `dominion-sub3`의 실제 코드. SDK 기능 확대보다 메인 사용자 동작과 실행 정합성을 먼저 수리한다.

## 실사용 경로에서 반영한 변경

- `src/engine/chat/runtime.rs`: 모델이 선언한 도구 순서를 보존한다. 쓰기/셸/기타 부작용을 넘어서 읽기를 끌어올리던 전역 재정렬과 읽기 선행 실행을 제거한다. 연속된 읽기 도구 최대 4개만 병렬 처리하며, 호출별 권한/절차 사전 확인·예산 선차감·완료 결과 재사용을 적용해 중복 실행과 중복 청구를 방지한다.
- `src/engine/chat/context.rs`: 저장된 도구 관찰을 저장 순서가 아닌 `tool_call_id`로 복원한다. 병렬 완료 순서가 달라도 요청한 도구 결과를 다음 모델 호출에서 잃지 않는다.
- `src/engine/chat/mod.rs`, `context.rs`, `room_context.rs`, `web/src/pages/Chat.tsx`: 메인과 같은 자동 한국어 인식 및 사용자가 선택한 원어 옵션을 되살리고, 시스템 프롬프트의 장황한 언어 지시를 줄였다.
- `src/engine/verification.rs`: 메인의 한국어 파일명·바탕화면 대상 인식과 명시적 변경 결과/실제 효과 입증을 수용한다. 서브3의 `freeze_criteria`는 보존한다.
- `src/engine/code_index.rs`, `src/engine/rsi_system.rs`: `crates/` 실소스도 탐색하면서 `target`, `node_modules` 등 생성물은 제외한다.

## 회귀 확인

- `src/engine/chat/tests.rs`: 도구 ID별 복원, 읽기/쓰기/셸 순서 장벽, 한국어 자동·원어 우선, 실파일 생성/셸 증거 검사.
- `src/engine/verification.rs`: 한국어 파일명과 바탕화면 경로 검증.
- GitHub 최신 브랜치 파일을 다시 읽어 함수 호출 경계·조건·UI 원어 옵션 11개를 정적으로 확인했다.

## 실제 검증과 메인 동등성 (2026-11-15)

- 메인 Git 트리 `a6c12ccf85c55ecf4ea46e33a7f4f9027bb36e9f`의 정적 API route **210/210**이 서브3에도 존재한다. 서브3의 `/api/jobs/:id/directive` 1개가 추가로 존재한다.
- 메인/서브3 `web/`에서 빌드 캐시를 제외한 **56/56개 파일 Git blob SHA가 전부 동일**하다.
- [제품 집중 테스트 #43711789923](https://github.com/hushmind3/dominion-rust/actions/runs/43711789923): Rust 본체 컴파일, 채팅/도구 순서 회귀, 결과물 검증, 실제 네이티브 MP4 생성, 웹 타입 검사·프로덕션 빌드 **통과**.
- [Rust SDK·RSI 집중 테스트 #43711861791](https://github.com/hushmind3/dominion-rust/actions/runs/43711861791): Rust SDK·네이티브 호스트/RSI/복구·TypeScript UI 등을 검증하는 워크플로 **통과**.
- 메인의 공유 `dominion_agent_sdk::runtime::BudgetLedger`를 제품 채팅의 `budget`으로 직접 재사용한다. 분기별 독립 계수가 아닌 하나의 공유 회계 장부에서 동시 실행을 승인한다.
- `src/api/sdk_runs.rs::create`는 메인 실행 계약의 `model`, `maxRounds`를 실제 `jobs::create`의 모델·예산 입력으로 연결한다.
- `src/engine/video.rs`의 무API 네이티브 영상은 장면별 테마/미세 줌과 Windows 오프라인 System.Speech 경로를 추가한다. GitHub Ubuntu 집중 검증의 실제 MP4 테스트가 통과했으며, Windows 음성 품질 자체는 별도 검증이 필요하다.

### 실제 FFmpeg 강제·HTTP 통합 재검증

[최종 제품 검증 #43712789490](https://github.com/hushmind3/dominion-rust/actions/runs/43712789490) **성공**. FFmpeg/FFprobe를 runner에 필수로 준비하고 `DOMINION_REQUIRE_VIDEO_FFMPEG=1`로 무조건 실제 비디오를 생성합니다. MP4 출력은 ffprobe가 H.264·720×1280 스트림이라고 확인합니다. Rust 채팅 **19/19**, 파일 검증 **3/3**, 영상 **1/1**, 실제 HTTP·모의 모델 기반 연동 **10/10**, 프런트엔드 타입 검사·빌드를 통과했습니다. 첫 HTTP 실패에서 발견된 잘못된 도구 인수 오류 메시지를 수정하여 모델이 오류를 보고 적절히 복구하게 했습니다. `Cargo.lock`도 새 Rust 직접 의존성과 일치시켰습니다.

## 검증되지 않은 범위

정적 경로 일치가 전체 동작의 완전 동등성을 증명하지는 않는다. 라이브 상용 제공자별 실제 청구 비용, Windows 오프라인 음성의 환경별 사용 가능성, 공개 프런티어 벤치마크, 전체 99개 HTTP e2e 사례 및 유료 모델을 이용한 장기 다세대 RSI 완주는 여기서 완료를 주장하지 않는다.
