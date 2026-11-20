//! Compatibility identifiers for verified file-mutation preflight records.
//! Experimental fixture generation, synthetic workloads and promotion logic
//! were retired. These values only interpret stored evidence and active guards.
pub const EXACT_KERNEL: &str = "exact-file-replacement-v3";
pub const GUARD_KERNEL: &str = "same-file-revision-v2";
pub const GUARD_WORKLOAD: &str = "model-mediated-exact-workspace-file-v1";
