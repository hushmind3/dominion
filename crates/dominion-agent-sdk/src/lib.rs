//! Stable DOMINION Rust SDK facade. No agent turn loop or database.
//! Executable policies are provided by the separate algorithms crate;
//! the trusted contracts, authority fences and scheduling live in runtime.
pub use dominion_agent_runtime as runtime;
pub use dominion_agent_runtime::run_contract::{RunPhase,RunRef,RunSurface,RunTransition,
    RoomEvent,RoomAgentBinding,RUN_CONTRACT_VERSION};
pub use dominion_agent_runtime::{Action,Receipt,Strategy,ToolSpec,BoxFuture,Limits,
    CriterionKind,Criterion,RunRequest,Decision,ModelReply,ModelTurn,Model,
    Tools,Evidence,Journal,DispatchResult,Dispatcher,Status,CheckResult,Snapshot,
    AgentPorts,AgentLoop,dispatch_batch,dispatch_fence,DispatchFence,HarnessDrive,DriveAdmission};
pub use dominion_agent_runtime::schema::validate_arguments;
pub use dominion_agent_algorithms::{AgentAlgorithm,StandardAlgorithm,FocusedAlgorithm,Intent,
    Kernel,ReActLoop,tool_relevance,tool_relevance_with_schema};
pub use dominion_agent_algorithms::replay;
