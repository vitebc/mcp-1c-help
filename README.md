# mcp-1c-help

Standalone MCP сервер для поиска по официальной справке платформы 1С:Предприятие 8.3.

Работает через HTTP/SSE транспорт — поддерживается любыми MCP-клиентами (Claude Desktop, Cursor IDE, Copilot, и др.).

## Архитектура

```
┌──────────────┐     HTTP/SSE      ┌──────────────────────┐
│  MCP Client  │ ◄──────────────►  │  mcp-1c-help (Rust)  │
│  (Claude,    │    GET /sse       │                      │
│   Cursor,    │    POST /message  │  ┌────────────────┐  │
│   Copilot)   │                   │  │  HBK parser    │  │
└──────────────┘                   │  │  (shcntx_ru,   │  │
                                   │  │   shquery_ru,  │  │
                                   │  │   shlang_ru)   │  │
                                   │  ├────────────────┤  │
                                   │  │  SQLite FTS5   │  │
                                   │  │  (поисковый    │  │
                                   │  │   индекс)      │  │
                                   │  └────────────────┘  │
                                   └──────────────────────┘
```

## Быстрый старт

### Требования
- Установленная платформа 1С:Предприятие 8.3 (для доступа к `.hbk` файлам справки)
- Rust (для сборки)

### Сборка

```bash
cargo build --release
```

### Запуск

```bash
# Автоматическое определение путей 1С
./target/release/mcp-1c-help

# Кастомный путь к платформе
ONEC_HELP_PATH=/opt/1cv8/x86_64/8.3.27.1989 ./target/release/mcp-1c-help

# Кастомный порт (по умолчанию 3010)
MCP_HELP_PORT=3010 ./target/release/mcp-1c-help
```

### Подключение MCP клиента

```json
{
  "mcpServers": {
    "1c-help": {
      "url": "http://localhost:3010/sse"
    }
  }
}
```

## MCP Инструменты

| Инструмент | Описание |
|---|---|
| `search_1c_help` | Полнотекстовый поиск по справке (FTS5) с фильтром по категории |
| `get_1c_help_topic` | Полное содержимое темы по ID |
| `list_1c_help_versions` | Статус: версия, кол-во тем, дата индексации |
| `reindex_1c_help` | Принудительная переиндексация |

### Параметры search_1c_help

- `query` (обязательный) — поисковый запрос
- `limit` (опционально, по умолч. 5) — макс. результатов
- `category` (опционально) — раздел: `syntax`, `query`, `language`, `all`

## Эндпоинты

| Метод | Путь | Описание |
|---|---|---|
| `GET` | `/health` | Health check |
| `GET` | `/sse` | SSE поток для MCP |
| `POST` | `/message?sessionId=...` | JSON-RPC запросы |

## Переменные окружения

| Переменная | По умолчанию | Описание |
|---|---|---|
| `ONEC_HELP_PATH` | — | Путь к папке bin платформы 1С |
| `MCP_HELP_PORT` | `3010` | HTTP порт сервера |

## Поиск платформы 1С

Сервер автоматически ищет установленную платформу в стандартных путях:

- **Linux**: `/opt/1cv8`, `/opt/1cv8/x86_64`, `/usr/share/1cv8`
- **Windows**: `C:\Program Files\1cv8`, `C:\Program Files (x86)\1cv8`

Или через `ONEC_HELP_PATH`.

## Формат данных

- Индекс хранится в `~/.config/mcp-1c-help/help.db` (Linux) или `%APPDATA%/mcp-1c-help/help.db` (Windows)
- SQLite FTS5 с токенизатором `unicode61`
- При смене версии платформы индекс перестраивается автоматически
