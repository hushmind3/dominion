# 독립 에이전트 하네스

`crates/harness`는 제품 서버와 분리된 Rust 하네스 코어입니다. 기존 `src/engine/chat` 루프를 대체할 목적으로 만들었고, 지금은 라이브러리와 CLI로 동작합니다.

## 구성

- **루프**: 한 라운드 = 모델 호출 1회. 최대 라운드·도구 호출 수를 넘으면 중단합니다.
- **도구**: `Tool` 트레이트. `fs_read`, `fs_write`, `fs_list`, `shell`을 기본 제공하며 모두 작업 폴더 밖으로 나갈 수 없습니다.
- **정책**: `Policy`가 도구 실행을 허용·거부·승인대기로 결정합니다. 거부되면 실행을 멈추고 이벤트로 남깁니다.
- **컨텍스트**: 시스템·사용자 메시지는 보존하고, 오래된 assistant/tool 메시지만 예산에 맞게 제거합니다.
- **프로바이더**: `Provider` 트레이트. OpenAI 호환 어댑터(`openai`)를 포함합니다.
- **이벤트**: 라운드·도구·거부·컨텍스트 정리를 구조화된 이벤트로 보고합니다.

## CLI

```bash
cargo build --release -p dominion-harness --bin dominion-harness

./target/release/dominion-harness \
  --base-url http://127.0.0.1:8931/v1 \
  --model mock \
  --task "hello.txt를 만들어줘" \
  --workdir /tmp/demo \
  --allow-write
```

- `--allow-write`, `--allow-shell`을 주지 않으면 해당 도구는 모델에 노출되지 않고 실행되지 않습니다.
- 완료가 아니면 종료 코드 1입니다.

## 테스트

```bash
cargo test -p dominion-harness
```

검증 항목: 도구 실행·거부, 라운드/도구 상한, 컨텍스트 축소, 사용자 지시 불변, 경로 탈출 차단, 셸 종료 코드, OpenAI 응답 파싱.

## 아직 안 된 것

- 제품 서버(`/api/chat`)에 연결되지 않았습니다. 지금은 라이브러리·CLI 단계입니다.
- TypeScript SDK는 서버 API가 확정된 뒤 붙입니다.
- 샌드박스는 경로 제한만 있고 OS 격리는 없습니다.
