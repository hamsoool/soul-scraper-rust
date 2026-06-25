use std::collections::HashMap;

use bytes::Bytes;
use chrono::{DateTime, Datelike, Duration, Utc};
use futures::stream::{FuturesUnordered, StreamExt};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use sqlx::PgPool;
use tracing::{debug, error, info, warn};

use crate::{
    config::Settings,
    db::{self, NewDocument},
    error::{AppError, Result},
    security::validate_url,
};

// ---------------------------------------------------------------------------
// Target sources
// ---------------------------------------------------------------------------

pub struct Source {
    pub url: &'static str,
    pub category: &'static str,
}

pub const SOURCES: &[Source] = &[
    Source {
        url: "https://doe.gov.ph/articles/group/liquid-fuels?maincat=Retail%20Pump%20Prices&subcategory=Price%20Adjustments&display_type=Card",
        category: "Price Adjustments",
    },
    Source {
        url: "https://doe.gov.ph/articles/group/liquid-fuels?maincat=Retail%20Pump%20Prices&subcategory=North%20Luzon%20Pump%20Prices&display_type=Card",
        category: "North Luzon Pump Prices",
    },
];

// ---------------------------------------------------------------------------
// Static compiled regexes
// ---------------------------------------------------------------------------

static RE_MMDDYYYY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(0[1-9]|1[0-2])(0[1-9]|[12]\d|3[01])(20\d{2})\b").unwrap());

static RE_YEAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(20\d{2})\b").unwrap());

static RE_DAY: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(\d{1,2})\b").unwrap());

static RE_PDF_SUFFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r"-pdf\d*$").unwrap());

// Finds any 4-digit year starting with 20; caller validates non-adjacent digits
static RE_PDF_YEAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(20\d{2})").unwrap());

static RE_PDF_MONTH: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*").unwrap());

// Match day number after a separator; capture group 1 = day digits
static RE_PDF_DAY: Lazy<Regex> = Lazy::new(|| Regex::new(r"[-_](\d{1,2})[-_]").unwrap());

static RE_NUXT_ARGS_END: Lazy<Regex> = Lazy::new(|| Regex::new(r"\}\s*\(").unwrap());

static RE_VOID: Lazy<Regex> = Lazy::new(|| Regex::new(r"void 0").unwrap());

static RE_NEW_DATE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"new Date\(\d+\)").unwrap());

static RE_ARTICLE_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{id:(\d+)").unwrap());

/// Month name → number mapping (full names).
fn month_map() -> HashMap<&'static str, u32> {
    let mut m = HashMap::new();
    let names = [
        "january", "february", "march", "april", "may", "june", "july",
        "august", "september", "october", "november", "december",
    ];
    for (i, name) in names.iter().enumerate() {
        m.insert(*name, (i + 1) as u32);
    }
    m
}

/// Month abbreviation → number.
fn month_abbrev_map() -> HashMap<&'static str, u32> {
    HashMap::from([
        ("jan", 1), ("feb", 2), ("mar", 3), ("apr", 4), ("may", 5), ("jun", 6),
        ("jul", 7), ("aug", 8), ("sep", 9), ("oct", 10), ("nov", 11), ("dec", 12),
    ])
}

// ---------------------------------------------------------------------------
// Date parsing helpers
// ---------------------------------------------------------------------------

/// Extracts a UTC datetime from free-form text (article titles, link text).
/// Mirrors `parse_date_from_text` in scraper.py.
pub fn parse_date_from_text(text: &str, fallback: Option<DateTime<Utc>>) -> DateTime<Utc> {
    let now = Utc::now();
    let fallback = fallback.unwrap_or(now);

    if text.is_empty() {
        return fallback;
    }

    let lower = text.to_lowercase();

    // 1. MMDDYYYY pattern e.g. "05262026"
    if let Some(cap) = RE_MMDDYYYY.captures(text) {
        let m: u32 = cap[1].parse().unwrap_or(0);
        let d: u32 = cap[2].parse().unwrap_or(0);
        let y: i32 = cap[3].parse().unwrap_or(0);
        if let Some(dt) = chrono::NaiveDate::from_ymd_opt(y, m, d)
            .and_then(|nd| nd.and_hms_opt(0, 0, 0))
            .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, Utc))
        {
            return dt;
        }
    }

    // 2. Named month pattern e.g. "June 2 to 8, 2026"
    let months = month_map();
    let mut found_month: Option<u32> = None;
    for (name, &val) in &months {
        if lower.contains(name) {
            found_month = Some(val);
            break;
        }
    }
    let month = match found_month {
        Some(m) => m,
        None => return fallback,
    };

    let year: i32 = RE_YEAR
        .captures(text)
        .and_then(|c| c[1].parse().ok())
        .unwrap_or_else(|| fallback.year());

    let day: u32 = RE_DAY
        .captures_iter(text)
        .filter_map(|c| c[1].parse::<u32>().ok().filter(|&d| (1..=31).contains(&d)))
        .next()
        .unwrap_or(1);

    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|nd| nd.and_hms_opt(0, 0, 0))
        .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, Utc))
        .unwrap_or(fallback)
}

/// Extracts the start date from a DOE PDF URL filename.
/// Mirrors `parse_date_from_pdf_url` in scraper.py.
pub fn parse_date_from_pdf_url(url: &str) -> Option<DateTime<Utc>> {
    let filename = url.trim_end_matches('/').rsplit('/').next()?;
    let filename = filename.to_lowercase();
    // Step 1: 4-digit year
    // Append a trailing '-' so RE_PDF_DAY (which requires a trailing separator) always matches
    let filename = RE_PDF_SUFFIX.replace(&filename, "");
    let filename_with_trail = format!("{}-", filename);

    // Find the rightmost 20XX not surrounded by other digits
    // (manual digit-boundary check — Rust regex has no lookbehind)
    let year_match = RE_PDF_YEAR
        .find_iter(&filename_with_trail)
        .filter(|m| {
            let bytes = filename_with_trail.as_bytes();
            let start = m.start();
            let end = m.end();
            let before_ok = start == 0 || !bytes[start - 1].is_ascii_digit();
            let after_ok = end >= bytes.len() || !bytes[end].is_ascii_digit();
            before_ok && after_ok
        })
        .last()?; // use the last match (year appears near end of filename)

    let year: i32 = year_match.as_str().parse().ok()?;
    let before_year = &filename_with_trail[..year_match.start()];

    // Step 2: find first month abbreviation before the year
    let abbrevs = month_abbrev_map();
    let first_month_cap = RE_PDF_MONTH.captures(before_year)?;
    let month_match = first_month_cap.get(1)?;
    let month_abbrev = &month_match.as_str()[..3];
    let month = *abbrevs.get(month_abbrev)?;

    // Step 3: first day number after the month token (trailing '-' ensures match)
    let after_month = &before_year[first_month_cap.get(0)?.end()..];
    let day: u32 = RE_PDF_DAY
        .captures(after_month)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(1);

    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|nd| nd.and_hms_opt(0, 0, 0))
        .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, Utc))
}

// ---------------------------------------------------------------------------
// Nuxt state parser
// ---------------------------------------------------------------------------

/// Parsed output from the Nuxt IIFE block.
struct NuxtState {
    /// param_name → resolved value mapping
    mapping: HashMap<String, Value>,
    /// Raw body string of the IIFE `{return ...}`
    body: String,
}

/// Parses the Nuxt IIFE JS block and builds a param→value map.
/// Direct port of `parse_nuxt_state` in scraper.py.
fn parse_nuxt_state(js: &str) -> Option<NuxtState> {
    // Find `(function(`
    let func_start = js.find("(function(")?;
    let param_start = func_start + "(function(".len();
    let param_end = js[param_start..].find(')')? + param_start;
    let params_str = &js[param_start..param_end];
    let params: Vec<String> = params_str
        .split(',')
        .map(|p| p.trim().to_string())
        .collect();

    // Find `{return ` to locate the body start
    let return_str = "{return ";
    let return_idx = js[param_end..].find(return_str)? + param_end;
    let body_start = return_idx + return_str.len();

    // Last `}(` in the string marks where body ends and args list begins
    let boundary = RE_NUXT_ARGS_END.find_iter(js).last()?;
    let body_end = boundary.start();
    let arg_start = boundary.end() - 1; // points to `(`

    let body = js[body_start..body_end].to_string();

    // Balance-track parentheses to find end of args list
    let mut balance: i32 = 0;
    let mut arg_end = None;
    for (i, ch) in js[arg_start..].char_indices() {
        match ch {
            '(' => balance += 1,
            ')' => {
                balance -= 1;
                if balance == 0 {
                    arg_end = Some(arg_start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let arg_end = arg_end?;
    let args_str = &js[arg_start + 1..arg_end];

    // Normalise JS-isms before parsing as JSON array
    let args_clean = RE_VOID.replace_all(args_str, "null");
    let args_clean = RE_NEW_DATE.replace_all(&args_clean, "null");
    let json_str = format!("[{}]", args_clean);

    let args_list: Vec<Value> = serde_json::from_str(&json_str).ok()?;

    // Build param → value mapping
    let mapping: HashMap<String, Value> = params
        .into_iter()
        .zip(args_list.into_iter())
        .collect();

    Some(NuxtState { mapping, body })
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// Fetches `url` with up to 3 retries using exponential backoff.
async fn fetch_with_backoff(client: &Client, url: &str) -> Result<reqwest::Response> {
    let mut delay = std::time::Duration::from_secs(1);
    let max_delay = std::time::Duration::from_secs(10);
    let retries = 3;

    for attempt in 0..retries {
        match client.get(url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    return Ok(resp);
                }
                let status = resp.status();
                if attempt == retries - 1 {
                    return Err(AppError::Http(
                        resp.error_for_status().unwrap_err(),
                    ));
                }
                warn!(
                    "HTTP attempt {} failed for {}: status {}. Retrying in {}ms…",
                    attempt + 1,
                    url,
                    status,
                    delay.as_millis()
                );
            }
            Err(e) => {
                if attempt == retries - 1 {
                    return Err(AppError::Http(e));
                }
                warn!(
                    "HTTP attempt {} failed for {}: {}. Retrying in {}ms…",
                    attempt + 1,
                    url,
                    e,
                    delay.as_millis()
                );
            }
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(max_delay);
    }

    Err(AppError::Scraper("Retries exhausted".to_string()))
}

/// Downloads a PDF with streaming and enforces size limits.
pub async fn download_pdf_stream(client: &Client, url: &str, max_bytes: usize) -> Result<Bytes> {
    validate_url(url)?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(AppError::Http)?;

    response.error_for_status_ref().map_err(|e| AppError::Http(e))?;

    // Check Content-Length header
    if let Some(len) = response.content_length() {
        if len as usize > max_bytes {
            return Err(AppError::Scraper(format!(
                "PDF exceeds size limit: {} bytes (limit {})",
                len, max_bytes
            )));
        }
    }

    // Stream chunks
    let mut buf = Vec::with_capacity(1024 * 512);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(AppError::Http)?;
        buf.extend_from_slice(&chunk);
        if buf.len() > max_bytes {
            return Err(AppError::Scraper(format!(
                "PDF exceeded size limit during download ({} bytes)",
                buf.len()
            )));
        }
    }

    Ok(Bytes::from(buf))
}

// ---------------------------------------------------------------------------
// PDF text extraction — pdfium-render (spawn_blocking to avoid blocking async)
// ---------------------------------------------------------------------------

/// Extracts all text from PDF bytes using pdfium-render.
/// Runs in a dedicated thread pool via `spawn_blocking` so the async runtime
/// is never blocked by the heavy rendering work.
pub async fn extract_pdf_text(pdf_bytes: Bytes) -> String {
    tokio::task::spawn_blocking(move || extract_pdf_text_sync(&pdf_bytes))
        .await
        .unwrap_or_default()
}

fn extract_pdf_text_sync(pdf_bytes: &[u8]) -> String {
    use pdfium_render::prelude::*;

    // bind_to_library returns Result<Box<dyn PdfiumLibraryBindings>, PdfiumError>
    // Pdfium::new() takes the bindings directly (not a Result)
    let bindings = Pdfium::bind_to_library(
        Pdfium::pdfium_platform_library_name_at_path("./"),
    )
    .or_else(|_| Pdfium::bind_to_system_library());

    let bindings = match bindings {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to load PDFium library: {}", e);
            return String::new();
        }
    };

    let pdfium = Pdfium::new(bindings);

    let doc = match pdfium.load_pdf_from_byte_slice(pdf_bytes, None) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to load PDF from bytes: {}", e);
            return String::new();
        }
    };

    let mut pages_text = Vec::new();
    for page in doc.pages().iter() {
        match page.text() {
            Ok(text_obj) => pages_text.push(text_obj.all()),
            Err(e) => tracing::warn!("Error reading page text: {}", e),
        }
    }

    pages_text.join("\n")
}


// ---------------------------------------------------------------------------
// Value resolver helper
// ---------------------------------------------------------------------------

/// Given a raw token from the Nuxt body (e.g. a variable name like `_0` or a
/// quoted string like `"hello"`), resolve it to a plain Rust String using the
/// parameter mapping if available.
fn resolve_str(token: &str, mapping: &HashMap<String, Value>) -> String {
    let token = token.trim();

    // Quoted literal
    if (token.starts_with('"') && token.ends_with('"'))
        || (token.starts_with('\'') && token.ends_with('\''))
    {
        let inner = &token[1..token.len() - 1];
        // Attempt unicode-escape decode (Rust strings are already UTF-8, but JS
        // source may have \u00xx sequences).
        return unescape_js(inner);
    }

    // Variable reference
    if let Some(val) = mapping.get(token) {
        return match val {
            Value::String(s) => unescape_js(s),
            Value::Number(n) => n.to_string(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
    }

    // Fall back to raw token
    token.to_string()
}

/// Best-effort JS unicode escape decoder (\uXXXX sequences).
fn unescape_js(s: &str) -> String {
    static RE_UNICODE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"\\u([0-9a-fA-F]{4})").unwrap());

    RE_UNICODE
        .replace_all(s, |caps: &regex::Captures| {
            let code = u32::from_str_radix(&caps[1], 16).unwrap_or(0);
            char::from_u32(code)
                .map(|c| c.to_string())
                .unwrap_or_default()
        })
        .into_owned()
}

// ---------------------------------------------------------------------------
// Page scraper
// ---------------------------------------------------------------------------

/// Metadata for one PDF document found on a source page.
#[derive(Debug, Clone)]
pub struct ScrapedRecord {
    pub source_category: String,
    pub title: String,
    pub source_url: String,
    pub pdf_url: String,
    pub published_date: DateTime<Utc>,
}

/// Scrapes a DOE page and returns extracted PDF document metadata.
/// Mirrors `scrape_source_page` in scraper.py.
pub async fn scrape_source_page(
    client: &Client,
    source_url: &str,
    category: &str,
    _settings: &Settings,

) -> Result<Vec<ScrapedRecord>> {
    validate_url(source_url).map_err(|e| {
        error!("Security blocked source URL {}: {}", source_url, e);
        e
    })?;

    info!("Fetching source list page: {}", source_url);

    let response = fetch_with_backoff(client, source_url).await?;
    let html_text = response.text().await.map_err(AppError::Http)?;

    // Find Nuxt state script tag
    let document = Html::parse_document(&html_text);
    let script_sel = Selector::parse("script").unwrap();

    let nuxt_script = document
        .select(&script_sel)
        .find(|el| {
            el.inner_html().contains("__NUXT__")
        })
        .map(|el| el.inner_html());

    let nuxt_js = match nuxt_script {
        Some(js) => js,
        None => {
            error!("Nuxt script tag not found on page {}", source_url);
            return Ok(vec![]);
        }
    };

    let state = match parse_nuxt_state(&nuxt_js) {
        Some(s) => s,
        None => {
            error!("Failed to parse Nuxt state for {}", source_url);
            return Ok(vec![]);
        }
    };

    // Locate the articles array in the body
    let articles_marker = "articles:[";
    let articles_start = match state.body.find(articles_marker) {
        Some(pos) => pos,
        None => {
            warn!("No articles list found in Nuxt state for {}", source_url);
            return Ok(vec![]);
        }
    };

    // Balance-track brackets to find the end of the articles array
    let body_from_articles = &state.body[articles_start + articles_marker.len() - 1..];
    let mut balance: i32 = 0;
    let mut articles_end = None;
    for (i, ch) in body_from_articles.char_indices() {
        match ch {
            '[' => balance += 1,
            ']' => {
                balance -= 1;
                if balance == 0 {
                    articles_end = Some(articles_start + articles_marker.len() - 1 + i);
                    break;
                }
            }
            _ => {}
        }
    }

    let articles_end = match articles_end {
        Some(e) => e,
        None => {
            warn!("Unbalanced articles array in Nuxt state for {}", source_url);
            return Ok(vec![]);
        }
    };

    let articles_str = &state.body[articles_start..=articles_end];

    // Find individual article objects by {id: pattern
    let article_starts: Vec<usize> = RE_ARTICLE_ID
        .find_iter(articles_str)
        .map(|m| m.start())
        .collect();

    let mut records: Vec<ScrapedRecord> = Vec::new();
    let now = Utc::now();
    let two_weeks_ago = now - Duration::days(14);
    let cms_base = "https://prod-cms.doe.gov.ph";

    let a_sel = Selector::parse("a").unwrap();

    for (i, &start_idx) in article_starts.iter().enumerate() {
        let end_idx = if i + 1 < article_starts.len() {
            article_starts[i + 1]
        } else {
            articles_str.len() - 1
        };
        let segment = &articles_str[start_idx..end_idx];

        // Extract article id
        let _art_id = match RE_ARTICLE_ID.captures(segment).and_then(|c| c.get(1)) {

            Some(m) => m.as_str(),
            None => continue,
        };

        // Extract and resolve title
        static RE_TITLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"title:([^,\}]+)").unwrap());
        let title_var = RE_TITLE
            .captures(segment)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        let title = resolve_str(title_var, &state.mapping);

        // Extract and resolve datePublished
        static RE_DATE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"datePublished:([^,\|\}]+)").unwrap());
        let date_var = RE_DATE
            .captures(segment)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        let date_val = resolve_str(date_var, &state.mapping);

        let fallback_dt = date_val
            .replace('Z', "+00:00")
            .parse::<DateTime<Utc>>()
            .ok()
            .unwrap_or(now);

        // Extract and resolve content HTML
        static RE_CONTENT: Lazy<Regex> =
            Lazy::new(|| Regex::new(r#"content:("(?:[^"\\]|\\.)*")"#).unwrap());
        let content_html_raw = match RE_CONTENT.captures(segment).and_then(|c| c.get(1)) {
            Some(m) => m.as_str(),
            None => continue,
        };

        let content_html = match serde_json::from_str::<Value>(content_html_raw) {
            Ok(Value::String(s)) => unescape_js(&s),
            _ => unescape_js(content_html_raw.trim_matches('"')),
        };

        if content_html.is_empty() {
            continue;
        }

        // Parse links from content HTML
        let art_doc = Html::parse_fragment(&content_html);

        // Resolve slug for article URL
        static RE_SLUG: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"friendlyUrlPath:([^,\}]+)").unwrap());
        let slug_var = RE_SLUG
            .captures(segment)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        let slug = resolve_str(slug_var, &state.mapping);

        let source_article_url = if slug.is_empty() {
            source_url.to_string()
        } else {
            format!("https://doe.gov.ph/articles/{slug}")
        };

        for link in art_doc.select(&a_sel) {
            let href = match link.value().attr("href") {
                Some(h) => h,
                None => continue,
            };

            if !href.contains("/documents/") || !href.contains("guest") {
                continue;
            }

            let pdf_url = if href.starts_with("http") {
                href.to_string()
            } else {
                format!("{cms_base}{href}")
            };

            let link_text = link.text().collect::<String>().trim().to_string();

            let doc_title = if category == "Price Adjustments" {
                if link_text.len() > 10 {
                    link_text.clone()
                } else {
                    title.clone()
                }
            } else {
                format!("{title} - {link_text}")
            };

            // Date filtering
            let published_dt = if category == "Price Adjustments" {
                let dt = parse_date_from_text(&link_text, Some(fallback_dt));
                if dt < two_weeks_ago {
                    info!("Filtering out '{}' published at {} (older than 2 weeks)", doc_title, dt.date_naive());
                    continue;
                }
                dt
            } else {
                // North Luzon: use URL date
                let url_date = match parse_date_from_pdf_url(&pdf_url) {
                    Some(d) => d,
                    None => {
                        info!("Skipping '{}': cannot parse date from URL '{}'", doc_title, pdf_url);
                        continue;
                    }
                };
                if url_date.year() != now.year() || url_date.month() != now.month() {
                    info!(
                        "Filtering out '{}' — PDF date {} is not in {}-{:02}",
                        doc_title, url_date.date_naive(), now.year(), now.month()
                    );
                    continue;
                }
                url_date
            };

            records.push(ScrapedRecord {
                source_category: category.to_string(),
                title: doc_title,
                source_url: source_article_url.clone(),
                pdf_url,
                published_date: published_dt,
            });
        }
    }

    Ok(records)
}

// ---------------------------------------------------------------------------
// Sync orchestrator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncResult {
    pub status: String,
    pub processed_count: u64,
    pub duration_seconds: f64,
    pub errors: Vec<String>,
    pub cleanup_outdated: u64,
    pub cleanup_duplicates: u64,
}

/// Full sync: scrape both sources, download new PDFs in parallel, save to DB.
/// This is the Rust equivalent of `sync_doe_data` in scraper.py — with the
/// key upgrade that all PDF downloads are executed concurrently via
/// `FuturesUnordered` instead of sequentially.
pub async fn sync_doe_data(pool: &PgPool, settings: &Settings) -> SyncResult {
    info!("Starting sync_doe_data execution…");
    let start = std::time::Instant::now();
    let mut errors: Vec<String> = Vec::new();
    let mut processed_count: u64 = 0;

    // Pre-sync cleanup
    let (outdated, dupes) = match db::cleanup_outdated(pool).await {
        Ok(counts) => counts,
        Err(e) => {
            errors.push(format!("Cleanup failed: {e}"));
            (0, 0)
        }
    };
    info!("Pre-sync cleanup: {} outdated, {} duplicates removed", outdated, dupes);

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(settings.http_timeout_seconds))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .expect("Failed to build HTTP client");

    // Step 1 & 2: Scrape all source pages
    let mut all_records: Vec<ScrapedRecord> = Vec::new();
    for source in SOURCES {
        match scrape_source_page(&client, source.url, source.category, settings).await {
            Ok(recs) => {
                info!("Extracted {} document links from '{}'", recs.len(), source.category);
                all_records.extend(recs);
            }
            Err(e) => {
                let msg = format!("Failed to scrape '{}': {}", source.category, e);
                error!("{}", msg);
                errors.push(msg);
            }
        }
    }

    // Deduplicate by pdf_url within this sync run
    let mut seen = std::collections::HashSet::new();
    let unique_records: Vec<ScrapedRecord> = all_records
        .into_iter()
        .filter(|r| seen.insert(r.pdf_url.clone()))
        .collect();

    info!("Total deduplicated candidate links: {}", unique_records.len());

    // Step 3: Filter out already-existing records
    let mut new_records: Vec<ScrapedRecord> = Vec::new();
    for record in unique_records {
        match db::exists_by_pdf_url(pool, &record.pdf_url).await {
            Ok(true) => {
                debug!("Document already exists: {}", record.pdf_url);
            }
            Ok(false) => new_records.push(record),
            Err(e) => {
                let msg = format!("DB check failed for {}: {}", record.pdf_url, e);
                error!("{}", msg);
                errors.push(msg);
            }
        }
    }

    info!("{} new PDFs to download and process", new_records.len());

    // Step 4 & 5: Download + extract PDF text CONCURRENTLY
    // Each new record gets its own tokio task; we collect them with FuturesUnordered
    // for maximum throughput without waiting on a sequential loop.
    let max_bytes = settings.max_pdf_size_bytes;

    type TaskResult = std::result::Result<(ScrapedRecord, String), (String, String)>;

    let mut futures = FuturesUnordered::new();

    for record in new_records {
        let client_clone = client.clone();
        let url = record.pdf_url.clone();

        futures.push(tokio::spawn(async move {
            info!("Downloading PDF: {}", url);
            let bytes = match download_pdf_stream(&client_clone, &url, max_bytes).await {
                Ok(b) => b,
                Err(e) => {
                    return TaskResult::Err((url, format!("Download failed: {e}")));
                }
            };
            let text = extract_pdf_text(bytes).await;
            TaskResult::Ok((record, text))
        }));
    }

    // Collect results as they complete
    while let Some(join_result) = futures.next().await {
        match join_result {
            Err(e) => {
                let msg = format!("Task panic: {e}");
                error!("{}", msg);
                errors.push(msg);
            }
            Ok(Err((url, err_msg))) => {
                error!("Failed to process {}: {}", url, err_msg);
                errors.push(format!("Failed to process {url}: {err_msg}"));
            }
            Ok(Ok((record, text))) => {
                let new_doc = NewDocument {
                    source_category: record.source_category,
                    title: record.title,
                    source_url: record.source_url,
                    pdf_url: record.pdf_url.clone(),
                    content: if text.is_empty() { None } else { Some(text) },
                    published_date: Some(record.published_date),
                };

                match db::insert_document(pool, &new_doc).await {
                    Ok(id) if id > 0 => {
                        info!("Saved document id={} url={}", id, new_doc.pdf_url);
                        processed_count += 1;
                    }
                    Ok(_) => {
                        debug!("Document already existed (race condition): {}", new_doc.pdf_url);
                    }
                    Err(e) => {
                        let msg = format!("DB insert failed for {}: {}", new_doc.pdf_url, e);
                        error!("{}", msg);
                        errors.push(msg);
                    }
                }
            }
        }
    }

    let duration = start.elapsed().as_secs_f64();
    let status = if errors.is_empty() {
        "success"
    } else if processed_count > 0 {
        "partial_success"
    } else {
        "error"
    };

    info!(
        "sync_doe_data finished: {} processed, {:.2}s, {} error(s)",
        processed_count, duration, errors.len()
    );

    SyncResult {
        status: status.to_string(),
        processed_count,
        duration_seconds: duration,
        errors,
        cleanup_outdated: outdated,
        cleanup_duplicates: dupes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_date_from_text -----------------------------------------------

    #[test]
    fn test_parse_mmddyyyy() {
        let dt = parse_date_from_text("File 05262026 report", None);
        assert_eq!(dt.month(), 5);
        assert_eq!(dt.day(), 26);
        assert_eq!(dt.year(), 2026);
    }

    #[test]
    fn test_parse_named_month_range() {
        let dt = parse_date_from_text("June 2 to 8, 2026", None);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 2);
        assert_eq!(dt.year(), 2026);
    }

    #[test]
    fn test_parse_empty_text_returns_fallback() {
        use chrono::TimeZone;
        let fallback = Utc.with_ymd_and_hms(2025, 1, 15, 0, 0, 0).unwrap();
        let dt = parse_date_from_text("", Some(fallback));
        assert_eq!(dt, fallback);
    }

    // ---- parse_date_from_pdf_url --------------------------------------------

    #[test]
    fn test_url_standard_june() {
        let url = "https://prod-cms.doe.gov.ph/documents/d/guest/lf-price-monitoring-for-june-16-22-2026-pdf";
        let dt = parse_date_from_pdf_url(url).unwrap();
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 16);
        assert_eq!(dt.year(), 2026);
    }

    #[test]
    fn test_url_cross_month_uses_first_month() {
        let url = "https://prod-cms.doe.gov.ph/documents/d/guest/lf-price-monitoring-for-may-26-june-1-2026-pdf";
        let dt = parse_date_from_pdf_url(url).unwrap();
        assert_eq!(dt.month(), 5); // First month wins (May)
        assert_eq!(dt.day(), 26);
        assert_eq!(dt.year(), 2026);
    }

    #[test]
    fn test_url_december() {
        let url = "https://prod-cms.doe.gov.ph/documents/d/guest/lf-price-monitoring-for-dec-10-16-2024-pdf";
        let dt = parse_date_from_pdf_url(url).unwrap();
        assert_eq!(dt.month(), 12);
        assert_eq!(dt.day(), 10);
        assert_eq!(dt.year(), 2024);
    }

    #[test]
    fn test_url_nluz_format() {
        let url = "https://prod-cms.doe.gov.ph/documents/d/guest/nluz_regiii_dec-10-16_2024-pdf";
        let dt = parse_date_from_pdf_url(url).unwrap();
        assert_eq!(dt.month(), 12);
        assert_eq!(dt.day(), 10);
        assert_eq!(dt.year(), 2024);
    }

    #[test]
    fn test_url_no_year_returns_none() {
        let url = "https://prod-cms.doe.gov.ph/documents/d/guest/some-random-doc";
        assert!(parse_date_from_pdf_url(url).is_none());
    }

    // ---- security -----------------------------------------------------------

    #[test]
    fn test_validate_http_scheme_rejected() {
        let result = crate::security::validate_url("http://doe.gov.ph/foo");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_blocked_domain() {
        let result = crate::security::validate_url("https://evil.com/malware.pdf");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_empty_url() {
        let result = crate::security::validate_url("");
        assert!(result.is_err());
    }
}
