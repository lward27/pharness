use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    AgentControl,
    Filesystem,
    Shell,
    Git,
    KubernetesRead,
    KubernetesWrite,
    RegistryRead,
    RegistryWrite,
    TektonRead,
    TektonStartRun,
    ArgoRead,
    ArgoSync,
    DatabaseRead,
    DatabaseBackup,
    DatabaseMigration,
    ObservabilityRead,
    RagRead,
    RagWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCapability {
    pub kind: CapabilityKind,
    pub mutates: bool,
    pub production_risk: bool,
}

impl ToolCapability {
    pub const fn new(kind: CapabilityKind, mutates: bool, production_risk: bool) -> Self {
        Self {
            kind,
            mutates,
            production_risk,
        }
    }

    pub const fn read_only(kind: CapabilityKind) -> Self {
        Self::new(kind, false, false)
    }

    pub const fn mutating(kind: CapabilityKind, production_risk: bool) -> Self {
        Self::new(kind, true, production_risk)
    }
}

#[cfg(test)]
mod tests {
    use super::{CapabilityKind, ToolCapability};

    #[test]
    fn serializes_cluster_capability_kind_as_snake_case() {
        let serialized = serde_json::to_string(&CapabilityKind::TektonStartRun).unwrap();
        assert_eq!(serialized, "\"tekton_start_run\"");
    }

    #[test]
    fn preserves_mutation_and_production_risk_flags() {
        let capability = ToolCapability::mutating(CapabilityKind::ArgoSync, true);
        let round_trip: ToolCapability =
            serde_json::from_str(&serde_json::to_string(&capability).unwrap()).unwrap();

        assert_eq!(round_trip.kind, CapabilityKind::ArgoSync);
        assert!(round_trip.mutates);
        assert!(round_trip.production_risk);
    }
}
