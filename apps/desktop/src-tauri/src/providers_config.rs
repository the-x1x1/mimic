//! Building the provider registry from settings and secrets.

use std::sync::Arc;

use mimic_core::db::Db;
use mimic_core::providers::http::{AnthropicProvider, LocalHttpProvider};
use mimic_core::providers::{ModelProvider, SecretStore};

pub const DEFAULT_LOCAL_URL: &str = "http://127.0.0.1:11434/v1";
pub const DEFAULT_LOCAL_MODEL: &str = "llama3.2:3b";
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-5";

/// The local provider is always offered, even when nothing is listening on the
/// port: its health check is what tells the user to start Ollama, and an empty
/// provider list would leave Settings with nothing to explain.
pub fn build(db: &Db, secrets: &dyn SecretStore) -> Vec<Arc<dyn ModelProvider>> {
    let base_url = db
        .get_setting::<String>("generation.localUrl")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_LOCAL_URL.to_string());
    let local_model = db
        .get_setting::<String>("generation.localModel")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string());
    let anthropic_model = db
        .get_setting::<String>("generation.anthropicModel")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string());

    vec![
        Arc::new(LocalHttpProvider { base_url, model: local_model }),
        Arc::new(AnthropicProvider::from_secrets(&anthropic_model, secrets)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_core::providers::{MemorySecrets, ProviderRegistry};

    #[test]
    fn defaults_put_a_local_provider_first_and_it_is_the_one_chosen() {
        let db = Db::open_in_memory().unwrap();
        let registry = ProviderRegistry::new(build(&db, &MemorySecrets::default()));
        let list = registry.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "local");
        assert!(list[0].local);
        assert!(!list[1].local, "the hosted provider is labelled as such");
        assert_eq!(registry.default_id().as_deref(), Some("local"));
    }

    #[test]
    fn settings_change_the_endpoint_and_the_locality_claim_follows() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting("generation.localUrl", &"http://192.168.0.9:1234/v1").unwrap();
        db.set_setting("generation.localModel", &"qwen2.5:14b").unwrap();
        let registry = ProviderRegistry::new(build(&db, &MemorySecrets::default()));
        let local = registry.get("local").unwrap().info();
        assert_eq!(local.model, "qwen2.5:14b");
        assert!(!local.local, "pointing 'local' at another machine stops it being local");
        assert_eq!(registry.default_id().as_deref(), Some("local"), "it is still the first listed");
    }

    #[test]
    fn a_blank_setting_falls_back_to_the_default_rather_than_an_empty_url() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting("generation.localUrl", &"   ").unwrap();
        let registry = ProviderRegistry::new(build(&db, &MemorySecrets::default()));
        assert!(registry.get("local").unwrap().info().local);
    }
}
