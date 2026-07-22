use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

mod db;
mod hbk;
mod index;
mod tools;

const DEFAULT_PORT: u16 = 3010;

struct SessionState {
    tx: mpsc::Sender<String>,
}

struct AppState {
    sessions: Mutex<HashMap<String, SessionState>>,
    db: Mutex<Option<db::HelpDb>>,
    is_indexing: Mutex<bool>,
    current_platform: Mutex<Option<index::PlatformInfo>>,
}

// ─────────────── MCP Tool Definitions ───────────────

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "search_1c_help",
            "description": concat!(
                "Полнотекстовый поиск по официальной справке платформы 1С:Предприятие 8.3. ",
                "Ищет по всем разделам: встроенный язык, объектная модель, язык запросов. ",
                "Используй для поиска методов, свойств, операторов, функций встроенного языка.",
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Поисковый запрос (название метода, объекта, функции или описание задачи)"
                    },
                    "limit": {
                        "type": "number",
                        "description": "Максимальное количество результатов (по умолчанию 5)"
                    },
                    "category": {
                        "type": "string",
                        "enum": ["syntax", "query", "language", "all"],
                        "description": "Раздел справки: syntax — объектная модель, query — язык запросов, language — встроенный язык"
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "get_1c_help_topic",
            "description": concat!(
                "Получить полное содержимое темы из справки 1С по её идентификатору. ",
                "Используй topic_id из результатов search_1c_help.",
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "topic_id": {
                        "type": "string",
                        "description": "Идентификатор темы из результатов поиска"
                    }
                },
                "required": ["topic_id"]
            }
        }),
        json!({
            "name": "list_1c_help_versions",
            "description": "Получить список проиндексированных версий платформы 1С и статистику.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "reindex_1c_help",
            "description": concat!(
                "Принудительно пересоздать индекс справки 1С:Предприятие. ",
                "Используй если база данных справки пустая или устаревшая.",
            ),
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
    ]
}

// ─────────────── JSON-RPC Handlers ───────────────

fn handle_initialize() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "mcp-1c-help",
            "version": "0.1.0"
        }
    })
}

fn handle_tools_list() -> Value {
    json!({ "tools": tool_definitions() })
}

async fn handle_tools_call(params: &Value, state: &Arc<AppState>) -> Value {
    let tool_name = params["name"].as_str().unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    if tool_name == "reindex_1c_help" {
        let mut is_indexing = state.is_indexing.lock().await;
        if *is_indexing {
            return json!({
                "content": [{"type": "text", "text": "⏳ Индексация уже выполняется. Подождите завершения."}]
            });
        }

        let platform = state.current_platform.lock().await.clone();
        match platform {
            Some(p) => {
                *is_indexing = true;
                let state_clone = Arc::clone(state);
                let platform_clone = index::PlatformInfo {
                    version: p.version.clone(),
                    bin_path: p.bin_path.clone(),
                };

                tokio::spawn(async move {
                    let db_path = get_db_path();
                    match db::HelpDb::new(&db_path) {
                        Ok(new_db) => {
                            if let Err(e) = index::run_indexing(&platform_clone, &new_db) {
                                eprintln!("[1c-help] Reindex error: {}", e);
                            }
                            *state_clone.db.lock().await = Some(new_db);
                        }
                        Err(e) => {
                            eprintln!("[1c-help] DB init error during reindex: {}", e);
                        }
                    }
                    *state_clone.is_indexing.lock().await = false;
                });

                json!({
                    "content": [{"type": "text", "text": "🔄 Переиндексация запущена. Займёт 1-3 минуты."}]
                })
            }
            None => {
                json!({
                    "content": [{"type": "text", "text": "⚠️ Платформа 1С не найдена. Переиндексация невозможна."}]
                })
            }
        }
    } else {
        let db_guard = state.db.lock().await;
        let is_idx = *state.is_indexing.lock().await;
        tools::handle_tool_call(tool_name, &args, &db_guard, is_idx)
    }
}

async fn process_request(
    method: &str,
    params: &Value,
    state: &Arc<AppState>,
) -> Result<Value, (i32, String)> {
    match method {
        "initialize" => Ok(handle_initialize()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(handle_tools_list()),
        "tools/call" => Ok(handle_tools_call(params, state).await),
        _ => Err((-32601, format!("Method not found: {}", method))),
    }
}

// ─────────────── HTTP Handlers ───────────────

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel::<String>(256);

    {
        let mut sessions = state.sessions.lock().await;
        sessions.insert(
            session_id.clone(),
            SessionState { tx: tx.clone() },
        );
    }

    let _ = tx
        .send(format!(
            "event: endpoint\ndata: /message?sessionId={}\n\n",
            session_id
        ))
        .await;

    eprintln!("[1c-help] SSE session opened: {}", session_id);

    let stream = ReceiverStream::new(rx).map(|msg| Ok(Event::default().data(msg)));

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn message_handler(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
    body: String,
) -> impl IntoResponse {
    let session_id = match params.get("sessionId") {
        Some(id) => id.clone(),
        None => {
            eprintln!("[1c-help] POST /message — missing sessionId");
            return (axum::http::StatusCode::BAD_REQUEST, "Missing sessionId").into_response();
        }
    };

    let request: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[1c-help] Invalid JSON-RPC: {}", e);
            return (axum::http::StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e))
                .into_response();
        }
    };

    let id = request.get("id").cloned();
    let method = request["method"].as_str().unwrap_or("").to_string();
    let req_params = request.get("params").cloned().unwrap_or(json!({}));

    match process_request(&method, &req_params, &state).await {
        Ok(result) => {
            if let Some(id_val) = id {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id_val,
                    "result": result
                });

                let msg = format!(
                    "data: {}\n\n",
                    serde_json::to_string(&response).unwrap_or_default()
                );

                let sessions = state.sessions.lock().await;
                if let Some(session) = sessions.get(&session_id) {
                    let _ = session.tx.send(msg).await;
                } else {
                    eprintln!("[1c-help] Session not found: {}", session_id);
                }
            }
            (axum::http::StatusCode::ACCEPTED, "Accepted").into_response()
        }
        Err((code, msg)) => {
            if let Some(id_val) = id {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id_val,
                    "error": { "code": code, "message": msg }
                });

                let resp_msg = format!(
                    "data: {}\n\n",
                    serde_json::to_string(&response).unwrap_or_default()
                );

                let sessions = state.sessions.lock().await;
                if let Some(session) = sessions.get(&session_id) {
                    let _ = session.tx.send(resp_msg).await;
                }
            }
            (axum::http::StatusCode::ACCEPTED, "Accepted").into_response()
        }
    }
}

async fn health_handler() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "mcp-1c-help",
        "version": "0.1.0"
    }))
}

// ─────────────── Database Path ───────────────

fn get_db_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| "/tmp".to_string());
    let dir = PathBuf::from(&base).join("mcp-1c-help");
    std::fs::create_dir_all(&dir).ok();
    dir.join("help.db")
}

// ─────────────── Startup Logic ───────────────

async fn startup_indexing(state: Arc<AppState>) {
    let platform = index::find_platform();
    {
        let mut p = state.current_platform.lock().await;
        *p = platform.clone();
    }

    let platform = match platform {
        Some(p) => {
            eprintln!("[1c-help] Найдена платформа: {} ({})", p.version, p.bin_path.display());
            p
        }
        None => {
            eprintln!("[1c-help] Платформа 1С не найдена. Укажите ONEC_HELP_PATH.");
            eprintln!("HELP_STATUS:unavailable:1C Platform not found");
            return;
        }
    };

    let db_path = get_db_path();
    let db_exists = db_path.exists();

    let needs_indexing = if db_exists {
        match db::HelpDb::new(&db_path) {
            Ok(temp_db) => {
                let version = temp_db.get_meta("indexed_version").ok().flatten();
                let count: u32 = temp_db
                    .get_meta("topic_count")
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                if version.as_deref() != Some(&platform.version) {
                    eprintln!(
                        "[1c-help] Версия изменилась: {:?} → {}. Индексация...",
                        version, platform.version
                    );
                    true
                } else if count == 0 {
                    eprintln!("[1c-help] База пуста. Индексация...");
                    true
                } else {
                    eprintln!("[1c-help] База актуальна: {} тем.", count);
                    *state.db.lock().await = Some(temp_db);
                    eprintln!("HELP_STATUS:ready:{}:{}", platform.version, count);
                    false
                }
            }
            Err(_) => true,
        }
    } else {
        true
    };

    if needs_indexing {
        match db::HelpDb::new(&db_path) {
            Ok(new_db) => {
                *state.is_indexing.lock().await = true;
                *state.db.lock().await = Some(new_db);
                eprintln!("HELP_STATUS:indexing:0:1000:Запуск индексации...");

                let state_clone = Arc::clone(&state);
                let plat = index::PlatformInfo {
                    version: platform.version.clone(),
                    bin_path: platform.bin_path.clone(),
                };

                tokio::task::spawn_blocking(move || {
                    let db_path = get_db_path();
                    match db::HelpDb::new(&db_path) {
                        Ok(index_db) => {
                            if let Err(e) = index::run_indexing(&plat, &index_db) {
                                eprintln!("[1c-help] Ошибка индексации: {}", e);
                                eprintln!("HELP_STATUS:unavailable:Indexing failed: {}", e);
                            }
                            let rt = tokio::runtime::Handle::current();
                            rt.block_on(async {
                                *state_clone.db.lock().await = Some(index_db);
                                *state_clone.is_indexing.lock().await = false;
                            });
                        }
                        Err(e) => {
                            eprintln!("[1c-help] Ошибка открытия БД: {}", e);
                            eprintln!("HELP_STATUS:unavailable:DB error");
                            let rt = tokio::runtime::Handle::current();
                            rt.block_on(async {
                                *state_clone.is_indexing.lock().await = false;
                            });
                        }
                    }
                });
            }
            Err(e) => {
                eprintln!("[1c-help] Ошибка инициализации БД: {}", e);
                eprintln!("HELP_STATUS:unavailable:DB init failed");
            }
        }
    }
}

// ─────────────── Entry Point ───────────────

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("MCP_HELP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let state = Arc::new(AppState {
        sessions: Mutex::new(HashMap::new()),
        db: Mutex::new(None),
        is_indexing: Mutex::new(false),
        current_platform: Mutex::new(None),
    });

    // Start background indexing
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        startup_indexing(state_clone).await;
    });

    let app = Router::new()
        .route("/sse", get(sse_handler))
        .route("/message", post(message_handler))
        .route("/health", get(health_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    eprintln!("[1c-help] MCP сервер запущен на http://{}", addr);
    eprintln!("[1c-help] SSE endpoint: http://{}/sse", addr);
    eprintln!("[1c-help] Health: http://{}/health", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}
