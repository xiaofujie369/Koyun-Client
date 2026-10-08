use client_platform::{
    api::{ApiState, router},
    crypto::MasterKey,
    panel::{AdapterRegistry, MockAdapter, XBoardAdapter},
    scope::TenantId,
    store::Store,
};
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "serve".into());
    if mode == "migrate" {
        let url = std::env::var("MIGRATION_DATABASE_URL")?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .map_err(|_| "Migration database unavailable")?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|_| "Database migration failed")?;
        println!("Database migrations applied");
        return Ok(());
    }
    if mode != "serve" {
        return Err("Expected serve or migrate".into());
    }
    let url = std::env::var("DATABASE_URL")?;
    let key = MasterKey::from_hex(&std::env::var("PLATFORM_MASTER_KEY")?)
        .map_err(|_| "Invalid master key")?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .map_err(|_| "Database unavailable")?;
    let store = Arc::new(
        Store::new(pool, key)
            .await
            .map_err(|_| "Runtime database role must be available and cannot bypass RLS")?,
    );
    let mut adapters = AdapterRegistry::default();
    for entry in store
        .public_tenants()
        .await
        .map_err(|_| "Tenant catalog unavailable")?
    {
        let tenant = TenantId::parse(entry["id"].as_str().ok_or("Invalid tenant")?)
            .map_err(|_| "Invalid tenant")?;
        let connection = store
            .tenant_connection(&tenant)
            .await
            .map_err(|_| "Tenant configuration unavailable")?;
        match connection.panel_type.as_str() {
            "mock" if std::env::var("ENABLE_DEMO").as_deref() == Ok("true") => {
                adapters.register(Arc::new(MockAdapter::new(tenant)))?
            }
            "xboard" => {
                let origin: Option<SocketAddr> =
                    std::env::var(format!("PANEL_ORIGIN_{}", tenant.as_str()))
                        .ok()
                        .map(|v| v.parse())
                        .transpose()?;
                adapters.register(Arc::new(XBoardAdapter::new(
                    tenant,
                    connection.base_url.as_deref().ok_or("Missing panel URL")?,
                    origin,
                )?))?;
            }
            "mock" => {}
            _ => return Err("Unsupported panel type".into()),
        }
    }
    let address: SocketAddr = std::env::var("LISTEN_ADDRESS")
        .unwrap_or_else(|_| "127.0.0.1:8099".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Platform API listening on {address}");
    axum::serve(
        listener,
        router(ApiState {
            store,
            adapters: Arc::new(adapters),
        })
        .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
