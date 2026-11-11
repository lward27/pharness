//! Governed agent workspaces.
//!
//! A workspace session is an isolated sandbox that runs under a strict
//! profile and a policy composed from layered authorities. This crate holds
//! the session lifecycle and execution-provider contract, the sandbox Pod
//! contract, egress rules, broker tokens, the capability broker and the
//! browser terminal. It builds on the agent, tool and safety-policy
//! primitives of `pharness-core` and has no dependency on the SDLC stage,
//! run or WorkItem model.
//!
//! The types still carry their original `Campus` names; Campus is the first
//! deployment of this boundary.

#![forbid(unsafe_code)]

pub mod campus_broker_token;
pub mod campus_capability;
pub mod campus_egress;
pub mod campus_lab_session;
pub mod campus_policy;
pub mod campus_profile;
pub mod campus_sandbox_pod;
pub mod campus_terminal;

pub use campus_lab_session::{
    CampusDeleteOutcome, CampusExecutionProvider, CampusLabDesiredState, CampusLabObservedState,
    CampusLabOperationKind, CampusLabOperationStatus, CampusLabSessionState,
    CampusLabTransitionError, CampusObservation, CampusProviderError, CampusProvisionOutcome,
    CampusProvisionSpec, CampusTeardownHook, CampusWorkloadRef, FakeCampusProvider, FakeFault,
    FakeFaultPoint, FakeTeardownHook, OwnedCampusResource,
};
pub use campus_policy::{
    CampusApproval, CampusApproverRole, CampusAuthority, CampusAuthorityDenies,
    CampusDecisionOutcome, CampusDecisionReason, CampusDecisionRecord, CampusEffectiveConstraints,
    CampusMandatoryDenies, CampusPolicyConstraints, CampusPolicyDimension, CampusPolicyError,
    CampusPolicyLayer, CampusPolicyLayerRef, CampusPolicySource, EffectiveCampusPolicy,
    CAMPUS_POLICY_LAYER_SCHEMA, EFFECTIVE_CAMPUS_POLICY_SCHEMA, MAX_HARD_TOKENS,
};
pub use campus_profile::{
    CampusAgentBudget, CampusCapabilityBinding, CampusCapabilityScope, CampusEgressDestination,
    CampusIsolation, CampusLaunchGate, CampusModelPolicy, CampusNetworkMode, CampusNetworkProfile,
    CampusProfile, CampusProfileError, CampusProfileKind, CampusProfileRef, CampusResourceCeilings,
    CampusStudentChoices, CAMPUS_PROFILE_SCHEMA, SECURITY_PROFILE_QUALIFICATION,
};
