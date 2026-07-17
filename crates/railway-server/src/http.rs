//! HTTP API — matches the original Impostor C# controllers exactly.
//!
//! Endpoints:
//! - `GET  /`                          — Hello diagnostic page
//! - `POST /api/user`                  — Get auth token
//! - `PUT  /api/games`                 — Get address to host a new game
//! - `GET  /api/games`                 — List public games (old flow)
//! - `POST /api/games?gameId=`         — Get address of a specific game
//! - `GET  /api/games/{gameId}`        — Show a specific game by code
//! - `GET  /api/games/filtered`        — Show filtered lobbies

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{Json, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info};

use crate::config::ServerConfig;
use crate::client_manager::ClientManager;
use crate::game_manager::{GameManager, PublicGameInfo};

/// Application state shared with axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub game_manager: Arc<GameManager>,
    pub client_manager: Arc<ClientManager>,
    pub public_ip: String,
    pub public_port: u16,
}

/// Start the HTTP API server.
pub async fn start_http_server(
    listen_addr: SocketAddr,
    game_manager: Arc<GameManager>,
    client_manager: Arc<ClientManager>,
    config: ServerConfig,
) {
    let state = AppState {
        game_manager,
        client_manager,
        public_ip: config.public_ip.clone(),
        public_port: config.public_port,
    };

    let app = Router::new()
        // Hello
        .route("/", get(hello))
        // Auth token
        .route("/api/user", post(get_token))
        // Games
        .route("/api/games", get(list_games).put(host_game).post(find_game))
        .route("/api/games/:game_id", get(show_game))
        .route("/api/games/filtered", get(filtered_lobbies))
        // Filters & filter tags
        .route("/api/filters", get(get_filters))
        .route("/api/filtertags", get(get_filtertags))
        // Catch-all: log any unmatched route
        .fallback(fallback)
        // Log ALL requests with method + URI before routing
        .layer(middleware::from_fn(log_all_requests))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    info!("HTTP API listening on {}", listen_addr);

    let listener = match tokio::net::TcpListener::bind(listen_addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("failed to bind HTTP server on {}: {}", listen_addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        error!("HTTP server error: {}", e);
    }
}

// ── Middleware ───────────────────────────────────────────────

/// Log every incoming HTTP request with method and URI.
async fn log_all_requests(req: Request<axum::body::Body>, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let query = uri.query().unwrap_or("");

    let headers = req.headers().clone();
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");

    if !query.is_empty() {
        info!("HTTP {} {}?{}  [{}]  UA={}", method, path, query, content_type, user_agent);
    } else {
        info!("HTTP {} {}  [{}]  UA={}", method, path, content_type, user_agent);
    }

    next.run(req).await
}

// ── Handlers ──────────────────────────────────────────────────

/// `GET /` — Hello page showing the server is running.
async fn hello() -> &'static str {
    "Impostor is running, please configure your Among Us to connect to a game\n\
     To generate a region file, go to https://impostor.github.io/Impostor\n"
}

/// `GET /api/filters` — Return permitted search filters.
/// Client expects: `{"filters": ["Tags", "PlayerSpeed", "Roles", ...]}`
/// where each string is a `Filters` enum value.
async fn get_filters() -> Json<Value> {
    Json(json!({
        "filters": [
            "Tags",
            "PlayerSpeed",
            "Roles",
            "KillCooldown",
            "VotingTime",
            "NumImposters",
            "VisualTasks",
            "AnonymousVotes",
            "ConfirmEjects",
            "DiscussionTime",
            "EmergencyCooldown",
            "NumEmergencyMeetings",
            "NumCommonTasks",
            "NumShortTasks",
            "NumLongTasks",
            "KillDistance"
        ]
    }))
}

/// `GET /api/filtertags?lang=` — Return filter tags (JSON array of strings).
async fn get_filtertags(Query(_params): Query<FilterTagsParams>) -> Json<Value> {
    Json(json!(["All", "Beginner", "Expert", "Serious", "Casual", "Roleplay", "Hide n Seek"]))
}

/// Catch-all fallback — log unmatched requests.
async fn fallback(req: axum::http::Request<axum::body::Body>) -> (StatusCode, String) {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    info!("UNMATCHED REQUEST: {method} {uri}");
    for (name, value) in headers.iter() {
        info!("  header: {name}: {value:?}");
    }
    (StatusCode::NOT_FOUND, format!("Not Found: {method} {uri}\n"))
}

/// `POST /api/user` — Get an authentication token.
///
/// Request body: `{Puid, Username, ClientVersion, Language}`
/// Response: raw Base64-encoded token string (text/plain, NOT JSON-wrapped)
async fn get_token(body: String) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), StatusCode> {
    #[derive(Deserialize)]
    struct TokenRequest {
        #[serde(rename = "Puid")]
        puid: String,
        #[serde(rename = "ClientVersion")]
        client_version: i32,
    }

    let req: TokenRequest = serde_json::from_str(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    #[derive(Serialize)]
    struct Token {
        #[serde(rename = "Content")]
        content: TokenPayload,
        #[serde(rename = "Hash")]
        hash: String,
    }

    #[derive(Serialize)]
    struct TokenPayload {
        #[serde(rename = "Puid")]
        puid: String,
        #[serde(rename = "ClientVersion")]
        client_version: i32,
        #[serde(rename = "ExpiresAt")]
        expires_at: String,
    }

    let token = Token {
        content: TokenPayload {
            puid: req.puid,
            client_version: req.client_version,
            // Match the C# Impostor default exactly — note: no trailing
            // "Z". The real `DateTime(2012, 12, 21)` has an unspecified
            // Kind, and System.Text.Json's default serialization of an
            // unspecified-kind DateTime omits the UTC "Z" suffix.
            expires_at: "2012-12-21T00:00:00".into(),
        },
        hash: "impostor_was_here".into(),
    };

    let json = serde_json::to_string(&token).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    use base64::{Engine as _, engine::general_purpose::STANDARD};

    // IMPORTANT: Among Us 2024+ reads the response body directly as a raw
    // Base64 string — it does NOT JSON-parse the body first. Wrapping the
    // Base64 in JSON quotes (as `Json(json!(...))` does) produces a body
    // like `"base64..."` where the leading `"` is not a valid Base64
    // character. The client then fails with "FormatException: The input
    // is not a valid Base-64 string".
    //
    // Return as `text/plain` with the raw Base64 bytes — no JSON wrapper.
    use axum::http::header;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain")],
        STANDARD.encode(json.as_bytes()),
    ))
}

/// `PUT /api/games` — Get address to host a new game on.
/// Returns `{Ip, Port}` — Ip as a uint32 number matching C# IPAddress.Address.
async fn host_game(State(state): State<AppState>) -> Json<Value> {
    let ip_num = resolve_ip_to_network_order(&state.public_ip);
    Json(json!({
        "Ip": ip_num,
        "Port": state.public_port,
    }))
}

/// `POST /api/games?gameId=` — Get the address of a specific game.
async fn find_game(
    State(state): State<AppState>,
    Query(params): Query<FindGameParams>,
) -> Result<Json<Value>, StatusCode> {
    if params.game_id == 0 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let game = state.game_manager.find(params.game_id);
    match game {
        Some(_g) => Ok(Json(json!({
            "Ip": resolve_ip_to_network_order(&state.public_ip),
            "Port": state.public_port,
        }))),
        None => Ok(Json(json!({
            "Errors": [{"Reason": 3}]
        }))),
    }
}

/// `GET /api/games?mapId=&lang=&numImpostors=` — List public games (old flow).
async fn list_games(
    State(state): State<AppState>,
    Query(params): Query<ListGamesParams>,
) -> Json<Value> {
    let games: Vec<Value> = state
        .game_manager
        .public_games(&state.client_manager)
        .into_iter()
        .filter(|g| {
            // Map filter (bitmask)
            if params.map_id != 0 && (params.map_id & (1 << g.map)) == 0 {
                return false;
            }
            // Impostor count filter
            if params.num_impostors != 0 && g.num_impostors != params.num_impostors as u8 {
                return false;
            }
            true
        })
        .map(|g| game_to_listing(&state, &g))
        .collect();

    Json(json!(games))
}


/// `GET /api/games/{gameId}` — Show a specific game by code.
async fn show_game(
    State(state): State<AppState>,
    Path(game_id): Path<i32>,
) -> Result<Json<Value>, StatusCode> {
    let game = state.game_manager.find(game_id);
    match game {
        Some(g) => {
            let info = GameManager::game_info_from(&g, &state.client_manager);
            let listing = game_to_listing(&state, &info);
            Ok(Json(json!({
                "Errors": serde_json::Value::Null,
                "Game": listing,
            })))
        }
        None => Ok(Json(json!({
            "Errors": [{"Reason": 3}],
            "Game": null,
        }))),
    }
}

/// `GET /api/games/filtered` — Show filtered lobbies.
async fn filtered_lobbies(State(state): State<AppState>) -> Json<Value> {
    // Matches the real C# `GamesController.ShowFilteredLobbies` EXACTLY:
    // it does NOT parse any `filter` query param at all — it just filters
    // by `IsPublic && GameState == NotStarted && PlayerCount < MaxPlayers`.
    // A previous version of this endpoint invented a `filter=<json>`
    // GameMode-matching scheme that doesn't exist in the real server; if
    // that parsing ever disagreed with what the client expected, it would
    // return zero results instead of "too many" — much worse than doing
    // nothing.
    let all_games = state.game_manager.all_games();
    let public_games: Vec<Value> = all_games
        .iter()
        .filter(|g| {
            g.is_public.load(std::sync::atomic::Ordering::Relaxed)
                && g.state() == railway_game_logic::GameState::NotStarted
                && {
                    let info = GameManager::game_info_from(g, &state.client_manager);
                    (info.player_count as u8) < info.max_players
                }
        })
        .map(|g| game_to_listing(&state, &GameManager::game_info_from(g, &state.client_manager)))
        .collect();

    let matching_count = public_games.len();
    info!(
        "GAME_SEARCH: total_games={} matching_games={}",
        all_games.len(),
        matching_count
    );

    // IMPORTANT: the real response's top-level keys are `Games`/`Metadata`
    // (capital, matching C#'s default System.Text.Json property-name
    // preservation — no camelCase policy is configured for this
    // endpoint). A previous version used lowercase `games`/`metadata`,
    // which the client's exact-case JSON deserializer would never find,
    // so search always came back empty regardless of what was actually
    // on the server.
    Json(json!({
        "Games": public_games,
        "Metadata": {
            "allGamesCount": all_games.len(),
            "matchingGamesCount": matching_count,
        },
    }))
}

// ── Helpers ───────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct ListGamesParams {
    #[serde(default, rename = "mapId")]
    map_id: i32,
    #[serde(default)]
    #[allow(dead_code)]
    lang: i32,
    #[serde(default, rename = "numImpostors")]
    num_impostors: i32,
}

#[derive(Deserialize)]
struct FindGameParams {
    // The client sends this as `?gameId=...` (camelCase, matching C#'s
    // JSON.NET default serialization of the property name). Without the
    // rename, axum's `Query` extractor deserializes strictly by field
    // name and would never match `gameId` against `game_id`, so this
    // always silently fell back to the `#[serde(default)]` value of 0 —
    // which the handler then rejected as "missing" with a 400. This is
    // why typing in a room code could never find a game, regardless of
    // whether the code was valid.
    #[serde(default, rename = "gameId")]
    game_id: i32,
}

#[derive(Deserialize)]
struct FilterTagsParams {
    #[serde(default)]
    #[allow(dead_code)]
    lang: u32,
}

/// Resolve public IP and return as network-byte-order u32 (matching C# IPAddress.Address).
/// Convert an IPv4 address string/hostname to the packed integer format
/// expected by the Among Us client, matching C#'s (obsolete but still used)
/// `IPAddress.Address` property.
///
/// IMPORTANT: `IPAddress.Address` is NOT the address in network byte order
/// (big-endian). It is the little-endian packing of the octets, i.e. for
/// `a.b.c.d` the value is `a | (b << 8) | (c << 16) | (d << 24)`.
/// For example 127.0.0.1 -> 16777343 (0x0100007F), not 2130706433 (0x7F000001).
/// Using big-endian here (as `u32::from_be_bytes`/`u32::from(Ipv4Addr)` do)
/// produces a value the client decodes as the wrong IP, which is why UDP
/// connections silently fail after the client gets this from the HTTP API.
fn resolve_ip_to_network_order(ip_str: &str) -> u32 {
    // Try to parse as IPv4 directly
    if let Ok(v4) = ip_str.parse::<Ipv4Addr>() {
        return u32::from_le_bytes(v4.octets());
    }
    // Try to resolve as hostname
    if let Ok(addrs) = (ip_str, 0).to_socket_addrs() {
        for addr in addrs {
            if let IpAddr::V4(v4) = addr.ip() {
                return u32::from_le_bytes(v4.octets());
            }
        }
    }
    // Fallback: 127.0.0.1 -> 16777343 (matches C# IPAddress.Loopback.Address)
    0x0100007F
}

/// Convert the server's public IP string to a u32 (matching C# IPAddress.Address).
/// See `resolve_ip_to_network_order` for why little-endian packing is required.
fn parse_ip_to_u32(ip_str: &str) -> u32 {
    // Try to parse as IPv4
    if let Ok(addr) = ip_str.parse::<Ipv4Addr>() {
        return u32::from_le_bytes(addr.octets());
    }
    // Try to resolve as a hostname and take the first IPv4 result
    if let Ok(addrs) = (ip_str, 0).to_socket_addrs() {
        for addr in addrs {
            if let IpAddr::V4(v4) = addr.ip() {
                return u32::from_le_bytes(v4.octets());
            }
        }
    }
    // Fallback: loopback
    0x0100007F
}

fn game_to_listing(state: &AppState, g: &PublicGameInfo) -> Value {
    json!({
        "IP": parse_ip_to_u32(&state.public_ip),
        "Port": state.public_port,
        "GameId": g.code,
        "PlayerCount": g.player_count,
        "HostName": g.host_name,
        "TrueHostName": g.host_name,
        "HostPlatformName": g.host_platform_name,
        "Platform": g.host_platform,
        "QuickChat": g.chat_mode,
        "Age": 0,
        "MaxPlayers": g.max_players,
        "NumImpostors": g.num_impostors,
        "MapId": g.map,
        "Language": g.keywords,
        "Options": "",
    })
}

