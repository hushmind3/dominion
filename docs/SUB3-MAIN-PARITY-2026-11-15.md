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

## 검증 범위와 남은 목표

이 변경 묶음의 `cargo test`, UI 타입 검사, 라이브 제공자·실청구 토큰 검증은 아직 수행되지 않았다. 기존 성공한 CI 결과를 이 커밋의 통과로 간주하지 않는다. 메인 전체 API/하네스의 전 기능 동등성도 아직 별도 종단 간 회귀 비교가 필요하다. 다음 검증 대상은 `cargo test --bin dominion-rust engine::chat::tests`, `cargo test --bin dominion-rust engine::verification`, 웹 타입 검사 및 사용자 실제 작업·비용 A/B다.
