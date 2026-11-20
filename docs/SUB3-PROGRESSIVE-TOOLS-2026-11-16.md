# Sub3 2026: cache-stable progressive tools inside the native Rust agent

## 2026-11-20 update: metadata fusion and contract-fenced bridge

- The Rust SDK and chat now both rank tools using names/descriptions **plus** bounded JSON Schema field vocabulary. This catches opaque MCP tools whose domain appears only in their argument names, without adding embeddings, network calls or a second authorized registry.
- Native discovery receipts must match current tool JSON parameters, effect/read-only classification and displayed description at bridge admission. Changed metadata requires searching again; host approvals and actual schema validation are still mandatory.
- Coverage: unit regressions in `crates/dominion-agent-sdk/src/tool_discovery.rs` and `src/engine/chat/runtime/capability.rs`. No 27B task accuracy, real token billing or runtime validation is asserted before those checks execute.
- External context: [MCP 2026-07-28 tools specification](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/tools.mdx) (tool lists can change) and [SCOUT hybrid tool retrieval, Aug 2026](https://arxiv.org/abs/2608.23992). DOMINION implements deterministic lexical+schema fusion, **not** SCOUT's dense vector stage.

## Cause and execution path

The product's `src/engine/native_agent.rs` resolves the entire owner-approved built-in + MCP catalogue and passes it into `crates/dominion-agent-sdk/src/lib.rs`. Previously, each model round copied **every** tool name, description and JSON schema into the legacy JSON system prompt, and native providers received every schema in `complete_with_tools`. A 100-tool catalogue therefore incurred work even when only code inspection or shell validation was needed.

There was also an actual **64-tool** size gate in both the prompt API and `RunRequest::validate`. The limit blocked large MCP deployments before a planner could start, irrespective of its token budget.

The implementation is now a deterministic, Rust-owned two-stage catalogue:

1. `src/api/jobs.rs` admits at most 1,024 explicitly authorized tools; the Rust SDK additionally caps aggregate allowlist-name bytes at 65,536 and rejects invalid identifiers. Existing small allowlists behave identically.
2. `src/engine/native_agent.rs` still owns the full permitted tool collection from the *real* product registries. It never trusts tool names inserted by model output as permissions.
3. `crates/dominion-agent-sdk/src/tool_discovery.rs` supplies a bounded, stable model-visible prefix of at most **13** tool declarations when the authorized catalogue exceeds 18 entries. It scores English/Korean/Chinese task vocabulary against actual tool names and descriptions. The shortlist contains built-in search and call bridge functions; smaller catalogues still expose every tool.
4. `dominion_tool_search({query,limit})` runs locally on the kernel's approved catalogue without an extra remote index, vector model or account. It returns actual matched tool names, descriptions and JSON argument schemas as durable read-only evidence, not a new permission grant.
5. The following model turn calls the fixed `dominion_tool_call({name,arguments})`. Rust resolves it **only** when a successful same-run search receipt included that name and the full owner-approved registry still contains it. The real original operation ID, target tool name and target arguments are committed as the execution receipt. The actual tool then flows through the existing production approval, recovery, permissions, provider, and cost paths. An unauthorized or undiscovered name executes no external effect.
6. Unlike an incremental `setTools` implementation, search **never changes the provider function declaration list**. This preserves the tool-schema portion of the provider's KV-cache prefix across planning turns. The selected initial list changes only if the owner's goal/directive changes.
7. `crates/dominion-agent-sdk/src/context.rs` retains the full local search output as a durable receipt, projects a bounded summary, and allows retrieving full older schema content via `dominion_receipt_read` instead of repeating an external call.

This is a shared SDK-kernel change. The existing GUI, REST API and TypeScript SDK already invoke the same Rust job. It adds no new UI menu, separate agent framework, new model dependency or cross-branch modification.

## Test evidence

- `crates/dominion-agent-sdk/src/tool_discovery.rs`: 80+ synthetic tools, deterministic shortlist, Unicode lexical hints, schema-size reduction, invalid query rejection, allowed-target-only bridge, exact receipt promotion and unchanged small catalogues.
- `crates/dominion-agent-sdk/tests/progressive_tool_discovery.rs`: scripted provider-native 90-tool end-to-end run. An initially hidden archive tool is discovered, then invoked through the generic fixed bridge. Across three turns the **native declaration list remains exactly equal**; only the real host operation executes. An excluded sensitive tool remains unreachable and absent. The test checks the actual durable receipt name and payload.
- Whole-product checks: product Rust build, core tests, API/UI parity, TypeScript SDK and Windows/macOS/Linux CI run on the branch head. See GitHub Actions for exact results, including any pending/failed checks.

**Measurement boundary:** the regression suite measures serialized schema bytes from a synthetic catalogue and exact function list stability. It does **not** establish real token billing savings or SWE-bench accuracy without fixed-model same-task trials. Discovery can add a model turn for a previously hidden tool and a generic bridged call is serialized conservatively as a potential write. These are explicit quality/latency tradeoffs.

## Upstream inspected (2026)

- Model Context Protocol, [2026-07-28 client practices](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/docs/2026-07-28/develop/clients/client-best-practices.mdx): progressive schema disclosure instead of eager full-catalog injection.
- Microsoft Foundry, [Tool search: Finding the right tool at the right time](https://commandline.microsoft.com/tool-search-toolboxes-foundry/) (2026-07-29): sparse retrieval can reduce token overhead; their tool-search benchmark savings **are not Dominion measurements**.
- Qwen Code, [actual tool-search code](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/tool-search.ts), [tests](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/tool-search.test.ts) and [fixed-prefix bridge design](https://github.com/QwenLM/qwen-code/blob/main/docs/design/toolsearch-preload-threshold.md): native function declarations must not be rebuilt after every search because prompt caching depends on the tool prefix. Dominion uses an independently authored Rust registry, receipts and dispatch bridge, rather than importing Qwen's TypeScript execution loop.
- 2026 Chinese practical discussion: [工具太多塞不下：Agent 动态工具检索实战](https://walterwang0x01.github.io/portfolio/posts/tool-retrieval-for-agents/) (2026-06-03): dynamic retrieval, near-duplicate tool confusion, and cache invalidation are linked costs.

## Scope and forward validation

The host still controls final task acceptance, file SHA evidence, external approvals, and durable operation budgets. Catalogue ranker is a conservative cheap lexical first stage, not a claimed semantic embedding model. A large synthetic toolset is supported without a large language model; actual task-level success and cost need instrumented real-provider evaluation. Frontend visual layout and existing user records remain unchanged.
