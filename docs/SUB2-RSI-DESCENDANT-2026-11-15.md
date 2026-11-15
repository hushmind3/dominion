# Sub2 RSI: spawned descendant executable provenance (2026-11-15)

## Behavior
- The Rust coordinator builds a new release binary from the current adopted Git commit, verifies its `--rsi-worker-identity` response, hashes the binary, then pins that hash on every new campaign generation's job input.
- The real spawned Rust `--rsi-worker-job` process independently hashes its own executable before using providers or tools. For system-evolution campaigns, it must match the parent-pinned SHA-256, verify the compiled controller and source blueprint, and save a durable receipt with job/campaign/revision/binary/controller digest.
- The coordinator checks that receipt and rejects independent paired comparison/adoption for any system-evolution generation with a mismatched or missing executable proof.
- `rsi_system::handoff_evidence` replays the stored worker proof in addition to candidate A/B results, applied code lineage, method transfer and component coverage, so unmeasured claims do not count as successful recursive handoffs.
- The branch guard now recognizes all intended developer branches (`dominion-sub1` through `dominion-sub4` and `dominion-champion`). Cross-branch publication still requires exact recorded provenance.

## Validation
`cargo test --release spawned_worker_receipt_must_match_binary_revision_and_controller`
`cargo test --release complete_handoff_requires_real_pair_replay_complete_design_and_inherited_method_execution`
`cargo test --release requires_complete_sha`
`cargo test --release`

The synthetic fixture tests validate failure-closed identity gates. Genuine multi-generation fixed-model capability improvement requires separately captured real execution, independent frozen comparison, adoption and a successor run. This change does not claim that experiment has already passed.

## CI integration regression repaired

The `tests/fixtures/rsi-generation-worker.rs` test driver writes **synthetic**
source/edit receipts for the model-free lineage integration tests. Both
controlled proposal variants now record the actual frozen
`job.input.rsiBaseCommit`; they use the same origin-version gate as real
`rsi_worktree::finish` evidence. No bypass is added to the independent evaluator.

The room tool-selection fixture now supplies `workspace_apply_patch` to the
candidate definition list before asserting it is selected for a write task.
The runtime selector correctly cannot return a tool definition that it was
never given. The regression was observed on macOS and Ubuntu in
GitHub Actions run `43690229138`; the fix targets the failed assertions while
leaving production tool routing unchanged.
