use super::*;

pub struct MockAdapter {
    tenant_id: TenantId,
}

impl MockAdapter {
    pub fn new(tenant_id: TenantId) -> Self {
        Self { tenant_id }
    }

    fn validate(&self, session: &PanelSession) -> Result<(), PanelError> {
        if session.tenant_id != self.tenant_id {
            return Err(PanelError::TenantMismatch);
        }
        if session.external_user_id != "demo-user" || session.credential.expose() != "demo-session"
        {
            return Err(PanelError::SessionExpired);
        }
        Ok(())
    }
}

#[async_trait]
impl PanelAdapter for MockAdapter {
    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    async fn authenticate(&self, credentials: &Credentials) -> Result<PanelSession, PanelError> {
        if credentials
            .email
            .trim()
            .eq_ignore_ascii_case("demo@example.com")
            && credentials.password.expose() == "demo"
        {
            Ok(PanelSession {
                tenant_id: self.tenant_id.clone(),
                external_user_id: "demo-user".into(),
                credential: Secret::new("demo-session".into()),
            })
        } else {
            Err(PanelError::InvalidCredentials)
        }
    }

    async fn get_user(&self, session: &PanelSession) -> Result<PanelUser, PanelError> {
        self.validate(session)?;
        Ok(PanelUser {
            external_user_id: "demo-user".into(),
            email: "demo@example.com".into(),
            disabled: false,
        })
    }

    async fn get_entitlement(&self, session: &PanelSession) -> Result<Entitlement, PanelError> {
        self.validate(session)?;
        Ok(Entitlement {
            expires_at: None,
            quota_bytes: 1073741824,
            upload_bytes: 0,
            download_bytes: 0,
            device_limit: Some(2),
        })
    }

    async fn get_subscription(
        &self,
        session: &PanelSession,
    ) -> Result<ProfileDocument, PanelError> {
        self.validate(session)?;
        ProfileDocument::parse(b"proxies:\n  - name: Demo\n    type: ss\n    server: 192.0.2.1\n    port: 443\n    cipher: aes-128-gcm\n    password: mock-only\nproxy-groups:\n  - name: Proxy\n    type: select\n    proxies: [Demo]\nrules:\n  - MATCH,Proxy\n".to_vec())
    }
}
