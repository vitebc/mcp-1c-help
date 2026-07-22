use serde_json::{json, Value};
use crate::db;

pub fn handle_tool_call(
    name: &str,
    args: &Value,
    db_opt: &Option<db::HelpDb>,
    is_indexing: bool,
) -> Value {
    if db_opt.is_none() {
        let msg = if is_indexing {
            "База данных справки 1С подготавливается (индексация). Пожалуйста, подождите 1-3 минуты и повторите запрос."
        } else {
            "База данных справки 1С недоступна."
        };
        return json!({
            "content": [{"type": "text", "text": msg}]
        });
    }

    let db = db_opt.as_ref().unwrap();

    match name {
        "search_1c_help" => search_1c_help(db, args),
        "get_1c_help_topic" => get_1c_help_topic(db, args),
        "list_1c_help_versions" => list_1c_help_versions(db),
        "reindex_1c_help" => {
            json!({
                "content": [{
                    "type": "text",
                    "text": "Переиндексация запускается через API управления сервером."
                }]
            })
        }
        _ => json!({
            "content": [{"type": "text", "text": format!("Неизвестный инструмент: {}", name)}]
        }),
    }
}

fn search_1c_help(db: &db::HelpDb, args: &Value) -> Value {
    let query = args.get("query")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .unwrap_or("");

    if query.is_empty() {
        return json!({
            "content": [{"type": "text", "text": "Ошибка: укажите поисковый запрос."}]
        });
    }

    let limit = args.get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .min(50) as u32;

    let category = args.get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    match db.search(query, category, limit) {
        Ok(results) => {
            if results.is_empty() {
                return json!({
                    "content": [{
                        "type": "text",
                        "text": format!("По запросу \"{}\" ничего не найдено в справке 1С.", query)
                    }]
                });
            }

            let items: Vec<String> = results
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    format!(
                        "**{}. {}**\nID: `{}`\n{}\n",
                        i + 1,
                        r.title,
                        r.topic_id,
                        r.excerpt
                    )
                })
                .collect();

            json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "## Результаты поиска по справке 1С: \"{}\"\n\n{}",
                        query,
                        items.join("\n---\n")
                    )
                }]
            })
        }
        Err(e) => json!({
            "content": [{"type": "text", "text": format!("Ошибка поиска: {}", e)}]
        }),
    }
}

fn get_1c_help_topic(db: &db::HelpDb, args: &Value) -> Value {
    let topic_id = args.get("topic_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .unwrap_or("");

    if topic_id.is_empty() {
        return json!({
            "content": [{"type": "text", "text": "Ошибка: укажите topic_id."}]
        });
    }

    match db.get_topic(topic_id) {
        Ok(Some(row)) => {
            json!({
                "content": [{
                    "type": "text",
                    "text": format!("# {}\n\n{}", row.title, row.content)
                }]
            })
        }
        Ok(None) => {
            json!({
                "content": [{"type": "text", "text": format!("Тема \"{}\" не найдена.", topic_id)}]
            })
        }
        Err(e) => {
            json!({
                "content": [{"type": "text", "text": format!("Ошибка: {}", e)}]
            })
        }
    }
}

fn list_1c_help_versions(db: &db::HelpDb) -> Value {
    match db.get_info() {
        Ok((version, count, indexed_at)) => {
            match version {
                Some(v) => {
                    let date_str = indexed_at
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|secs| {
                            let dur = std::time::Duration::from_secs(secs);
                            let epoch = std::time::SystemTime::UNIX_EPOCH + dur;
                            let datetime: chrono::DateTime<chrono::Utc> = epoch.into();
                            datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
                        })
                        .unwrap_or_else(|| "неизвестно".to_string());

                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "## 1С:Справка — Статус\n\n✅ Готово\n- Версия платформы: **{}**\n- Тем в базе: **{}**\n- Дата индексации: {}",
                                v, count, date_str
                            )
                        }]
                    })
                }
                None => {
                    json!({
                        "content": [{
                            "type": "text",
                            "text": "База данных не содержит проиндексированных версий."
                        }]
                    })
                }
            }
        }
        Err(e) => {
            json!({
                "content": [{"type": "text", "text": format!("Ошибка: {}", e)}]
            })
        }
    }
}
