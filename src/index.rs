use std::path::PathBuf;
use std::fs;
use crate::hbk;
use crate::db;

#[derive(Clone)]
pub struct PlatformInfo {
    pub version: String,
    pub bin_path: PathBuf,
}

const HBK_FILES: [(&str, &str); 3] = [
    ("shcntx_ru.hbk", "syntax"),
    ("shquery_ru.hbk", "query"),
    ("shlang_ru.hbk", "language"),
];

pub fn find_platform() -> Option<PlatformInfo> {
    if let Ok(path) = std::env::var("ONEC_HELP_PATH") {
        let path = path.trim().to_string();
        let p = PathBuf::from(&path);
        if p.join("shcntx_ru.hbk").exists() {
            let version = p.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("custom")
                .to_string();
            return Some(PlatformInfo { version, bin_path: p });
        }
        if let Some(platform) = find_platform_in_dir(&path) {
            return Some(platform);
        }
    }

    let search_paths: &[&str] = if cfg!(target_os = "windows") {
        &[
            r"C:\Program Files\1cv8",
            r"C:\Program Files (x86)\1cv8",
        ]
    } else {
        &[
            "/opt/1cv8",
            "/opt/1cv8/x86_64",
            "/usr/share/1cv8",
        ]
    };

    for base in search_paths {
        if let Some(platform) = find_platform_in_dir(base) {
            return Some(platform);
        }
    }

    None
}

fn find_platform_in_dir(base_path: &str) -> Option<PlatformInfo> {
    let base = PathBuf::from(base_path);
    if !base.is_dir() {
        return None;
    }

    let mut platforms: Vec<PlatformInfo> = Vec::new();

    if let Ok(entries) = fs::read_dir(&base) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            if !name_str.chars().all(|c| c.is_ascii_digit() || c == '.') {
                continue;
            }
            let parts: Vec<&str> = name_str.split('.').collect();
            if parts.len() != 4 {
                continue;
            }

            let bin_path = entry.path().join("bin");
            if !bin_path.is_dir() {
                continue;
            }
            if bin_path.join("shcntx_ru.hbk").exists() {
                platforms.push(PlatformInfo {
                    version: name_str,
                    bin_path,
                });
            }
        }
    }

    if platforms.is_empty() {
        return None;
    }

    platforms.sort_by(|a, b| {
        let a_parts: Vec<u32> = a.version.split('.').filter_map(|p| p.parse().ok()).collect();
        let b_parts: Vec<u32> = b.version.split('.').filter_map(|p| p.parse().ok()).collect();
        for i in 0..4 {
            let a_v = a_parts.get(i).copied().unwrap_or(0);
            let b_v = b_parts.get(i).copied().unwrap_or(0);
            if a_v != b_v {
                return b_v.cmp(&a_v);
            }
        }
        std::cmp::Ordering::Equal
    });

    Some(platforms.swap_remove(0))
}

pub fn run_indexing(platform: &PlatformInfo, db: &db::HelpDb) -> Result<(), String> {
    let version = &platform.version;

    eprintln!("[1c-help] Начало индексации версии {}", version);
    db.delete_version(version)?;

    for (filename, category) in &HBK_FILES {
        let hbk_path = platform.bin_path.join(filename);
        if !hbk_path.exists() {
            eprintln!("[1c-help] Файл не найден: {}", hbk_path.display());
            continue;
        }

        let hbk_path_str = hbk_path.to_string_lossy().to_string();
        let hbk_filename = filename.to_string();

        let pages = hbk::iter_hbk_pages(&hbk_path_str, |parsed, total| {
            let pct = if total > 0 { (parsed as f64 / total as f64 * 100.0) as u32 } else { 0 };
            eprintln!("HELP_STATUS:indexing:{}:{}:{}:{}", pct, total, parsed, hbk_filename);
        })?;

        eprintln!("[1c-help] {}: получено {} страниц", hbk_filename, pages.len());

        let mut batch: Vec<db::TopicRow> = Vec::with_capacity(100);
        for page in &pages {
            let (title, text) = extract_text(&page.html);
            let topic_id = format!("{}/{}/{}", version, category, page.name.replace('\\', "/"));

            batch.push(db::TopicRow {
                topic_id,
                title,
                content: text,
                category: category.to_string(),
                version: version.to_string(),
            });

            if batch.len() >= 100 {
                db.insert_topics(&batch)?;
                batch.clear();
            }
        }
        if !batch.is_empty() {
            db.insert_topics(&batch)?;
        }
    }

    let count = db.topic_count(version)?;
    db.set_meta("indexed_version", version)?;
    db.set_meta("topic_count", &count.to_string())?;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    db.set_meta("indexed_at", &now_secs.to_string())?;

    eprintln!("HELP_STATUS:ready:{}:{}", version, count);
    eprintln!("[1c-help] Индексация завершена: {} тем", count);

    Ok(())
}

fn extract_text(html: &str) -> (String, String) {
    use scraper::{Html, Selector};

    let re = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>|<style[^>]*>.*?</style>")
        .unwrap();
    let cleaned = re.replace_all(html, "");

    let document = Html::parse_document(&cleaned);

    let title_sel = Selector::parse("h1, h2, title").unwrap();
    let title = document
        .select(&title_sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Без названия".to_string());

    let body_sel = Selector::parse("body").unwrap();
    let text = document
        .select(&body_sel)
        .next()
        .map(|el| {
            el.text()
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let text: String = text.chars().take(10000).collect();

    (title, text)
}
