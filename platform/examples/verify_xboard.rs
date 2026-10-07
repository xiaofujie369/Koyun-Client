use client_platform::{
    panel::{Credentials, PanelAdapter, Secret, XBoardAdapter},
    scope::TenantId,
};
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct TestAccount {
    base_url: String,
    email: String,
    password: String,
    external_user_id: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let file = PathBuf::from(
        args.next()
            .ok_or("Expected private test-account JSON path")?,
    );
    let origin: SocketAddr = args
        .next()
        .ok_or("Expected explicit origin IP:port")?
        .to_str()
        .ok_or("Invalid origin")?
        .parse()?;
    let content = Zeroizing::new(std::fs::read(file)?);
    let account: TestAccount =
        serde_json::from_slice(&content).map_err(|_| "Invalid test-account file")?;
    let tenant = TenantId::parse("integration_test").map_err(|_| "Invalid test tenant")?;
    let adapter = XBoardAdapter::new(tenant, &account.base_url, Some(origin))?;
    let credentials = Credentials {
        email: account.email,
        password: Secret::new(account.password),
    };
    let session = adapter.authenticate(&credentials).await?;
    if session.external_user_id != account.external_user_id {
        return Err("Stable user ID did not match test account".into());
    }
    let entitlement = adapter.get_entitlement(&session).await?;
    let profile = adapter.get_subscription(&session).await?;
    println!(
        "login=ok stable_identity=ok entitlement=ok profile_yaml=ok profile_bytes={} device_limit={:?}",
        profile.content().len(),
        entitlement.device_limit
    );
    Ok(())
}
