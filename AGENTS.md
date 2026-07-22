# AGENTS.md — mcp-1c-help

## Проект

Автономный MCP сервер на Rust для полнотекстового поиска по официальной справке 1С:Предприятие 8.3 (.hbk файлы).

## Структура

```
mcp-1c-help/
├── Cargo.toml
├── README.md
├── AGENTS.md
├── .gitignore
└── src/
    ├── main.rs    — HTTP/SSE MCP сервер (axum)
    ├── hbk.rs     — Парсер .hbk бинарного формата
    ├── db.rs      — SQLite FTS5 обёртка
    ├── index.rs   — Поиск платформы + индексация
    └── tools.rs   — MCP инструменты
```

## Сборка

```bash
cargo build --release
```

Бинарник: `target/release/mcp-1c-help`

## Запуск

```bash
# Linux с установленной 1С (авто-определение)
./target/release/mcp-1c-help

# С кастомным путём
ONEC_HELP_PATH=/opt/1cv8/x86_64/8.3.27.1989/bin ./target/release/mcp-1c-help

# Другой порт
MCP_HELP_PORT=3000 ./target/release/mcp-1c-help
```

## Команды (package.json опционально)

Пока npm скрипт не добавлен — сборка напрямую через cargo.

## Зависимости

| Крейт | Назначение |
|---|---|
| tokio | async runtime |
| axum 0.7 | HTTP/SSE сервер |
| tower-http | CORS |
| serde / serde_json | JSON-RPC |
| rusqlite (bundled) | SQLite FTS5 |
| flate2 | DEFLATE для ZIP в HBK |
| scraper | HTML → текст |
| regex | Очистка HTML |
| uuid | Сессии SSE |
| chrono | Форматирование дат |
| futures / tokio-stream | SSE стримы |
| dirs | Системные пути |

## MCP транспорт

HTTP/SSE (Model Context Protocol):
- `GET /sse` — SSE поток (ответы)
- `POST /message?sessionId=X` — приём JSON-RPC запросов

## Инструменты

1. `search_1c_help` — FTS5 поиск с category/limit
2. `get_1c_help_topic` — полная тема по ID
3. `list_1c_help_versions` — статистика
4. `reindex_1c_help` — переиндексация

## Особенности

- Индекс в `%APPDATA%/mcp-1c-help/help.db` (Windows) или `~/.config/mcp-1c-help/help.db` (Linux)
- FTS5 fallback на LIKE при ошибке
- Индексация в фоновом потоке при старте
- Порядок поиска платформы: ONEC_HELP_PATH → стандартные пути Linux/Windows
