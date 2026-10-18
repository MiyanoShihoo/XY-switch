//! Passive observations never establish model identity. Offline reference work is
//! deliberately kept out of the proxy hot path.
mod invariant;
mod reference;
mod shadow;
mod stream;
mod tokenizer;

pub use invariant::{AuditRecord, Observation, audit, audit_with_aliases};
pub use reference::{
    RankingRequest, RankingResult, ReferenceFingerprint, ReferenceRequest, generate_reference,
    load_reference, rank_references,
};
pub use shadow::{
    ShadowCompleted, ShadowEngine, ShadowOutcome, ShadowPair, ShadowRegressionReport,
    ShadowSnapshot, regress_shadow,
};
pub use stream::{AxisResult, AxisResults, StreamAccumulator, StreamBilling, StreamSummary};
pub use tokenizer::LocalTokenizer;
