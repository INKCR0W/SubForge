use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use crate::{CoreError, CoreResult};

#[derive(Debug, Clone, Default)]
pub struct RefreshRegistry {
    running_sources: Arc<Mutex<HashSet<String>>>,
}

#[derive(Debug)]
pub struct RefreshGuard {
    source_id: String,
    running_sources: Arc<Mutex<HashSet<String>>>,
}

impl RefreshRegistry {
    pub fn global() -> Self {
        static GLOBAL_REFRESH_REGISTRY: OnceLock<RefreshRegistry> = OnceLock::new();
        GLOBAL_REFRESH_REGISTRY
            .get_or_init(RefreshRegistry::default)
            .clone()
    }

    pub fn try_acquire(&self, source_id: &str) -> CoreResult<RefreshGuard> {
        let mut running_sources = self
            .running_sources
            .lock()
            .map_err(|_| CoreError::RefreshAlreadyRunning(source_id.to_string()))?;
        if running_sources.contains(source_id) {
            return Err(CoreError::RefreshAlreadyRunning(source_id.to_string()));
        }
        running_sources.insert(source_id.to_string());
        Ok(RefreshGuard {
            source_id: source_id.to_string(),
            running_sources: Arc::clone(&self.running_sources),
        })
    }
}

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        if let Ok(mut running_sources) = self.running_sources.lock() {
            running_sources.remove(&self.source_id);
        }
    }
}
