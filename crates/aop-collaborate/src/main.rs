//! The binary: read the config, open the database, migrate it, serve.
//!
//! Migrations run at startup rather than from a separate command, matching
//! alterion-auth, because a self-hoster who has to remember a second step
//! will one day not remember it, and the failure looks like the application
//! being broken.

use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware::Logger, web};
use anyhow::{Context, Result};
use aop_collaborate::auth::IdpClient;
use aop_collaborate::config::{Config, config_path};
use aop_collaborate::live::Hub;
use aop_collaborate::schema::Migrator;
use aop_collaborate::state::AppState;
use sea_orm::Database;
use sea_orm_migration::MigratorTrait;

#[actix_web::main]
async fn main() -> Result<()> {
    let path = config_path();
    Config::create_if_missing(&path)?;
    let config = Config::load(&path)?;

    env_logger::Builder::new()
        .parse_filters(&config.log_level)
        .init();
    log::info!("configuration from {}", path.display());
    log::info!("identity provider: {}", config.issuer);

    let db = Database::connect(&config.database_url)
        .await
        .context("connect to the database")?;
    Migrator::up(&db, None).await.context("run migrations")?;

    let idp = IdpClient::new(&config)?;
    let config = Arc::new(config);
    let state = web::Data::new(AppState {
        db,
        idp,
        hub: Arc::new(Hub::new()),
        config: config.clone(),
    });

    let bind = config.bind_address.clone();
    log::info!("listening on {bind}");

    let server_config = config.clone();
    HttpServer::new(move || {
        // An empty origin list is not "allow everything": the desktop app
        // sends no Origin at all and is unaffected, so the only thing a wide
        // open list would help is a browser somebody else wrote.
        let mut cors = Cors::default()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);
        for origin in &server_config.allowed_origins {
            cors = cors.allowed_origin(origin);
        }

        App::new()
            .wrap(cors)
            .wrap(Logger::default())
            .app_data(state.clone())
            .configure(aop_collaborate::handlers::routes)
    })
    .bind(&bind)
    .with_context(|| format!("bind {bind}"))?
    .run()
    .await?;

    Ok(())
}
