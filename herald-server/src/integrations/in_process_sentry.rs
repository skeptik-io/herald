//! In-process SentryOps — runs SentryEngine in-process against any Store backend.

use std::sync::Arc;

use shroudb_acl::{PolicyEffect, PolicyPrincipal, PolicyRequest, PolicyResource};
use shroudb_sentry_engine::engine::{SentryConfig, SentryEngine};
use shroudb_store::Store;

use super::{SentryError, SentryOps};

pub struct InProcessSentryOps<S: Store> {
    engine: Arc<SentryEngine<S>>,
}

impl<S: Store> InProcessSentryOps<S> {
    pub async fn new(store: Arc<S>) -> Result<Self, SentryError> {
        let config = SentryConfig::default();
        let engine = SentryEngine::new(store, config, None)
            .await
            .map_err(|e| SentryError::Operation(format!("engine init: {e}")))?;
        Ok(Self {
            engine: Arc::new(engine),
        })
    }

    pub fn engine(&self) -> &Arc<SentryEngine<S>> {
        &self.engine
    }
}

#[async_trait::async_trait]
impl<S: Store> SentryOps for InProcessSentryOps<S> {
    async fn evaluate(
        &self,
        subject: &str,
        action: &str,
        resource: &str,
    ) -> Result<(), SentryError> {
        let request = PolicyRequest {
            principal: PolicyPrincipal {
                id: subject.to_string(),
                roles: vec![],
                claims: Default::default(),
            },
            action: action.to_string(),
            resource: PolicyResource {
                id: resource.to_string(),
                resource_type: "herald".to_string(),
                attributes: Default::default(),
            },
        };

        // If no policies are defined, permit by default.
        // Herald's authorization is opt-in — define policies to restrict access.
        match self.engine.evaluate_request(&request).await {
            Ok(signed) => match signed.decision {
                PolicyEffect::Permit => Ok(()),
                PolicyEffect::Deny => {
                    // If no policy matched (default-deny with zero policies), permit.
                    if signed.matched_policy.is_none() {
                        Ok(())
                    } else {
                        Err(SentryError::Denied(format!(
                            "policy={} denied",
                            signed.matched_policy.as_deref().unwrap_or("unknown"),
                        )))
                    }
                }
            },
            Err(e) => Err(SentryError::Operation(e.to_string())),
        }
    }
}
