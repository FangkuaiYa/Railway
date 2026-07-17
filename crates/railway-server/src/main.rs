//! railway-server — Among Us private server runtime.

use std::sync::Arc;
use tracing::{info, warn};

use railway_server::client_manager::ClientManager;
use railway_server::config::ServerConfig;
use railway_server::game_manager::GameManager;
use railway_server::logging;
use railway_server::matchmaker;

#[tokio::main]
async fn main() {
    match std::env::var("RUST_LOG") {
        Ok(v) if !v.trim().is_empty() => {
            eprintln!(
                "[startup] RUST_LOG env var is set to '{}' — this OVERRIDES the built-in filter.",
                v
            );
            if v.contains("impostor") && !v.contains("railway") {
                eprintln!(
                    "[startup] WARNING: RUST_LOG still references the old 'impostor' crate name. \
                     After the rename to railway_server/railway_hazel/railway_protocol/railway_game_logic, \
                     this directive matches nothing and ALL logs (including errors) will be dropped silently. \
                     Unset RUST_LOG or set it to e.g. 'info,railway_server=debug,railway_hazel=debug'."
                );
            }
        }
        _ => {
            eprintln!(
                "[startup] RUST_LOG not set, using built-in default filter."
            );
        }
    }

    logging::init_tracing();

    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");

    info!("{name} starting...");

    railway_game_logic::objects::register_default_spawnables();
    info!(
        "registered {} spawnable object types",
        railway_game_logic::objects::spawn_registry::registered_count()
    );

    let config = match ServerConfig::load(None) {
        Ok(c) => c,
        Err(e) => {
            warn!("failed to load config, using defaults: {}", e);
            ServerConfig::default()
        }
    };

    info!("==============================================");
    info!("  {name} v{version}");
    info!("==============================================");
    info!("  Listen   : {}:{} (UDP)", config.listen_ip, config.listen_port);
    info!("  Public   : {}:{}", config.public_ip, config.public_port);
    info!(
        "  AntiCheat: {}",
        if config.anticheat.enabled { "enabled" } else { "disabled" }
    );
    info!("==============================================");

    if config.public_ip == "127.0.0.1" {
        warn!(
            "public_ip is 127.0.0.1 (the built-in default) — if this is a real \
             deployment, remote clients will be told to connect to their own \
             loopback address and the UDP handshake will fail/time out after \
             the HTTP step succeeds. Set `public_ip` at the TOP LEVEL of \
             config.toml (not inside a [server] table) or via the \
             IMPOSTOR_PUBLIC_IP env var."
        );
    }

    let client_manager = Arc::new(ClientManager::new(config.compatibility.clone(), config.anticheat.clone()));
    let game_manager = Arc::new(GameManager::new());

    let http_game_manager = Arc::clone(&game_manager);
    let http_client_manager = Arc::clone(&client_manager);

    if config.http.enabled {
        let http_addr = format!(
            "{}:{}",
            config.http.listen_ip, config.http.listen_port
        )
        .parse()
        .expect("invalid HTTP listen address");

        let http_config = config.clone();
        tokio::spawn(async move {
            railway_server::http::start_http_server(
                http_addr,
                http_game_manager,
                http_client_manager,
                http_config,
            )
            .await;
        });

        info!("HTTP API enabled on http://{}", http_addr);
    }

    let mm_client_manager = Arc::clone(&client_manager);
    let mm_game_manager = Arc::clone(&game_manager);

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received SIGINT, shutting down...");
        let _ = shutdown_tx_clone.send(true);
    });

    #[cfg(unix)]
    {
        let shutdown_sigterm = shutdown_tx.clone();
        use tokio::signal::unix::{signal, SignalKind};
        tokio::spawn(async move {
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            sigterm.recv().await;
            info!("received SIGTERM, shutting down...");
            let _ = shutdown_sigterm.send(true);
        });
    }

    let listen_addr = config.listen_addr();
    info!("starting matchmaker on {}", listen_addr);

    let matchmaker_handle = tokio::spawn(async move {
        matchmaker::run_matchmaker(
            listen_addr,
            mm_client_manager,
            mm_game_manager,
        )
        .await;
    });

    tokio::select! {
        _ = matchmaker_handle => {
            info!("matchmaker exited");
        }
        _ = shutdown_rx.changed() => {
            info!("shutdown initiated");
        }
    }

    info!(
        "server shutting down — {} active clients, {} active games",
        client_manager.active_count(),
        game_manager.game_count(),
    );

    info!("{name} stopped");
}
