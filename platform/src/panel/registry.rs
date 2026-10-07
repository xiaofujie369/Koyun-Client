use super::{PanelAdapter, PanelError};
use crate::scope::TenantId;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct AdapterRegistry {
    adapters: HashMap<TenantId, Arc<dyn PanelAdapter>>,
}

impl AdapterRegistry {
    pub fn register(&mut self, adapter: Arc<dyn PanelAdapter>) -> Result<(), PanelError> {
        use std::collections::hash_map::Entry;
        match self.adapters.entry(adapter.tenant_id().clone()) {
            Entry::Occupied(_) => Err(PanelError::DuplicateTenant),
            Entry::Vacant(entry) => {
                entry.insert(adapter);
                Ok(())
            }
        }
    }

    pub fn get(&self, tenant: &TenantId) -> Result<Arc<dyn PanelAdapter>, PanelError> {
        self.adapters
            .get(tenant)
            .cloned()
            .ok_or(PanelError::UnknownTenant)
    }
}
