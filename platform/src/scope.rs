#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TenantId(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeError {
    InvalidIdentifier,
    TenantMismatch,
    AccountMismatch,
}

impl TenantId {
    pub fn parse(value: &str) -> Result<Self, ScopeError> {
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(ScopeError::InvalidIdentifier);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountScope {
    tenant_id: TenantId,
    account_id: String,
}

impl AccountScope {
    pub fn new(tenant_id: TenantId, account_id: String) -> Result<Self, ScopeError> {
        if account_id.is_empty() || account_id.len() > 128 {
            return Err(ScopeError::InvalidIdentifier);
        }
        Ok(Self {
            tenant_id,
            account_id,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub fn require_owner(&self, resource: &Self) -> Result<(), ScopeError> {
        if self.tenant_id != resource.tenant_id {
            return Err(ScopeError::TenantMismatch);
        }
        if self.account_id != resource.account_id {
            return Err(ScopeError::AccountMismatch);
        }
        Ok(())
    }
}
