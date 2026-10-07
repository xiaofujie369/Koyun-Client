use super::{profile::MAX_PROFILE_BYTES, *};
use reqwest::{
    Client, Method, RequestBuilder, StatusCode, Url,
    header::{AUTHORIZATION, HeaderValue},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use std::{net::SocketAddr, time::Duration};

const MAX_JSON_BYTES: usize = 1024 * 1024;

#[cfg(test)]
mod tests;

pub struct XBoardAdapter {
    tenant_id: TenantId,
    base: Url,
    client: Client,
}

#[derive(Clone, Copy)]
enum Operation {
    Login,
    Account,
    Subscription,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct LoginData {
    auth_data: String,
}

impl XBoardAdapter {
    pub fn new(
        tenant_id: TenantId,
        base: &str,
        pinned_origin: Option<SocketAddr>,
    ) -> Result<Self, PanelError> {
        let base = Url::parse(base).map_err(|_| PanelError::InvalidConfiguration)?;
        if base.scheme() != "https"
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(PanelError::InvalidConfiguration);
        }
        let mut builder = Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20));
        if let Some(origin) = pinned_origin {
            builder = builder.resolve(
                base.host_str().ok_or(PanelError::InvalidConfiguration)?,
                origin,
            );
        }
        let client = builder
            .build()
            .map_err(|_| PanelError::InvalidConfiguration)?;
        Ok(Self {
            tenant_id,
            base,
            client,
        })
    }

    fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, PanelError> {
        let url = self
            .base
            .join(path)
            .map_err(|_| PanelError::InvalidConfiguration)?;
        Ok(self.client.request(method, url))
    }

    fn authenticated(&self, path: &str, credential: &Secret) -> Result<RequestBuilder, PanelError> {
        let value = credential.expose();
        if !value.starts_with("Bearer ") || value.len() <= 7 {
            return Err(PanelError::SessionExpired);
        }
        let mut header = HeaderValue::from_str(value).map_err(|_| PanelError::SessionExpired)?;
        header.set_sensitive(true);
        Ok(self
            .request(Method::GET, path)?
            .header(AUTHORIZATION, header))
    }

    async fn bytes(
        request: RequestBuilder,
        operation: Operation,
        limit: usize,
    ) -> Result<Vec<u8>, PanelError> {
        let mut response = request.send().await.map_err(|_| PanelError::Unavailable)?;
        let status = response.status();
        if !status.is_success() {
            if response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("text/html"))
            {
                return Err(PanelError::Unavailable);
            }
            return Err(match status {
                StatusCode::TOO_MANY_REQUESTS => PanelError::RateLimited,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => match operation {
                    Operation::Login => PanelError::InvalidCredentials,
                    Operation::Account => PanelError::SessionExpired,
                    Operation::Subscription => PanelError::EntitlementDenied,
                },
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
                    if matches!(operation, Operation::Login) =>
                {
                    PanelError::InvalidCredentials
                }
                _ if status.is_server_error() => PanelError::Unavailable,
                _ => PanelError::InvalidResponse,
            });
        }
        if response
            .content_length()
            .is_some_and(|len| len > limit as u64)
        {
            return Err(PanelError::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| PanelError::Unavailable)?
        {
            if chunk.len() > limit.saturating_sub(bytes.len()) {
                return Err(PanelError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    async fn json<T: DeserializeOwned>(
        request: RequestBuilder,
        operation: Operation,
    ) -> Result<T, PanelError> {
        let bytes = Zeroizing::new(Self::bytes(request, operation, MAX_JSON_BYTES).await?);
        serde_json::from_slice::<Envelope<T>>(&bytes)
            .map(|r| r.data)
            .map_err(|_| PanelError::InvalidResponse)
    }

    fn require_tenant(&self, session: &PanelSession) -> Result<(), PanelError> {
        if session.tenant_id == self.tenant_id {
            Ok(())
        } else {
            Err(PanelError::TenantMismatch)
        }
    }

    async fn user(&self, credential: &Secret) -> Result<PanelUser, PanelError> {
        let data: Value = Self::json(
            self.authenticated("/api/v1/user/info", credential)?,
            Operation::Account,
        )
        .await?;
        let id = number(&data, "id").map_err(|_| PanelError::StableIdentityUnavailable)?;
        if id == 0 {
            return Err(PanelError::StableIdentityUnavailable);
        }
        let email = data["email"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(PanelError::InvalidResponse)?;
        let disabled = match &data["banned"] {
            Value::Bool(b) => *b,
            v if v.as_u64() == Some(0) => false,
            v if v.as_u64() == Some(1) => true,
            _ => return Err(PanelError::InvalidResponse),
        };
        Ok(PanelUser {
            external_user_id: id.to_string(),
            email: email.to_owned(),
            disabled,
        })
    }

    async fn subscription_data(&self, session: &PanelSession) -> Result<Value, PanelError> {
        self.require_tenant(session)?;
        let user = self.get_user(session).await?;
        if user.disabled {
            return Err(PanelError::AccountDisabled);
        }
        Self::json(
            self.authenticated("/api/v1/user/getSubscribe", &session.credential)?,
            Operation::Account,
        )
        .await
    }
}

#[async_trait]
impl PanelAdapter for XBoardAdapter {
    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    async fn authenticate(&self, credentials: &Credentials) -> Result<PanelSession, PanelError> {
        let request = self
            .request(Method::POST, "/api/v1/passport/auth/login")?
            .json(&serde_json::json!({
                "email": credentials.email,
                "password": credentials.password.expose(),
            }));
        let login: LoginData = Self::json(request, Operation::Login).await?;
        let credential = Secret::new(login.auth_data);
        let user = self.user(&credential).await?;
        if user.disabled {
            return Err(PanelError::AccountDisabled);
        }
        Ok(PanelSession {
            tenant_id: self.tenant_id.clone(),
            external_user_id: user.external_user_id,
            credential,
        })
    }

    async fn get_user(&self, session: &PanelSession) -> Result<PanelUser, PanelError> {
        self.require_tenant(session)?;
        let user = self.user(&session.credential).await?;
        if user.external_user_id != session.external_user_id {
            return Err(PanelError::IdentityMismatch);
        }
        Ok(user)
    }

    async fn get_entitlement(&self, session: &PanelSession) -> Result<Entitlement, PanelError> {
        entitlement(&self.subscription_data(session).await?)
    }

    async fn get_subscription(
        &self,
        session: &PanelSession,
    ) -> Result<ProfileDocument, PanelError> {
        let data = self.subscription_data(session).await?;
        let token = data["token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(PanelError::InvalidResponse)?;
        let request = self
            .request(Method::GET, "/api/v1/client/subscribe")?
            .query(&[("token", token), ("flag", "clash.meta")]);
        ProfileDocument::parse(
            Self::bytes(request, Operation::Subscription, MAX_PROFILE_BYTES).await?,
        )
    }
}

fn number(data: &Value, field: &str) -> Result<u64, PanelError> {
    data[field]
        .as_u64()
        .or_else(|| data[field].as_str().and_then(|s| s.parse().ok()))
        .ok_or(PanelError::InvalidResponse)
}

fn entitlement(data: &Value) -> Result<Entitlement, PanelError> {
    Ok(Entitlement {
        expires_at: if data
            .get("expired_at")
            .ok_or(PanelError::InvalidResponse)?
            .is_null()
        {
            None
        } else {
            Some(number(data, "expired_at")?)
        },
        quota_bytes: number(data, "transfer_enable")?,
        upload_bytes: number(data, "u")?,
        download_bytes: number(data, "d")?,
        device_limit: if data
            .get("device_limit")
            .ok_or(PanelError::InvalidResponse)?
            .is_null()
        {
            None
        } else {
            Some(
                u32::try_from(number(data, "device_limit")?)
                    .map_err(|_| PanelError::InvalidResponse)?,
            )
        },
    })
}
