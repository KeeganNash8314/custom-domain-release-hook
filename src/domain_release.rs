use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct ReleaseLedger {
    inner: Arc<RwLock<HashMap<String, BuildEvent>>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BuildEvent {
    pub build_id: String,
    pub domain: String,
    pub state: BuildState,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildState {
    WaitingForDomain,
    Released,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DomainVerified {
    pub event: String,
    pub data: VerifiedData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VerifiedData {
    pub domain: String,
}

impl ReleaseLedger {
    pub async fn track(&self, build_id: String, domain: String) -> BuildEvent {
        let event = BuildEvent {
            build_id: build_id.clone(),
            domain,
            state: BuildState::WaitingForDomain,
        };
        self.inner.write().await.insert(build_id, event.clone());
        event
    }

    pub async fn apply_verification(&self, notification: &DomainVerified) -> Vec<BuildEvent> {
        if notification.event != "dns.domain.verified" {
            return Vec::new();
        }
        let mut builds = self.inner.write().await;
        let mut released = Vec::new();
        for build in builds.values_mut() {
            if build.domain == notification.data.domain
                && build.state == BuildState::WaitingForDomain
            {
                build.state = BuildState::Released;
                released.push(build.clone());
            }
        }
        released
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verification_releases_only_builds_for_that_domain() {
        let ledger = ReleaseLedger::default();
        ledger
            .track("build-42".into(), "docs.example.com".into())
            .await;
        ledger
            .track("build-43".into(), "api.example.com".into())
            .await;

        let released = ledger
            .apply_verification(&DomainVerified {
                event: "dns.domain.verified".into(),
                data: VerifiedData {
                    domain: "docs.example.com".into(),
                },
            })
            .await;

        assert_eq!(released.len(), 1);
        assert_eq!(released[0].build_id, "build-42");
        assert_eq!(released[0].state, BuildState::Released);
    }
}
