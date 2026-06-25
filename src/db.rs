use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::error::{AppError, Result};

/// Database model for a scraped document. Maps 1:1 to the `documents` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: i32,
    pub source_category: String,
    pub title: String,
    pub source_url: String,
    pub pdf_url: String,
    pub content: Option<String>,
    pub published_date: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Lightweight version for list endpoints (no content field).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentListItem {
    pub id: i32,
    pub source_category: String,
    pub title: String,
    pub source_url: String,
    pub pdf_url: String,
    pub published_date: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new document record.
#[derive(Debug, Clone)]
pub struct NewDocument {
    pub source_category: String,
    pub title: String,
    pub source_url: String,
    pub pdf_url: String,
    pub content: Option<String>,
    pub published_date: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Row mappers
// ---------------------------------------------------------------------------

fn map_list_item(row: &sqlx::postgres::PgRow) -> DocumentListItem {
    DocumentListItem {
        id: row.get("id"),
        source_category: row.get("source_category"),
        title: row.get("title"),
        source_url: row.get("source_url"),
        pdf_url: row.get("pdf_url"),
        published_date: row.get("published_date"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn map_document(row: &sqlx::postgres::PgRow) -> Document {
    Document {
        id: row.get("id"),
        source_category: row.get("source_category"),
        title: row.get("title"),
        source_url: row.get("source_url"),
        pdf_url: row.get("pdf_url"),
        content: row.get("content"),
        published_date: row.get("published_date"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Returns a paginated list of documents, newest first, optionally filtered by category.
pub async fn get_documents(
    pool: &PgPool,
    category: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<DocumentListItem>> {
    let sql_base = r#"
        SELECT id, source_category, title, source_url, pdf_url,
               published_date, created_at, updated_at
        FROM documents
    "#;

    let rows = match category {
        Some(cat) => {
            sqlx::query(&format!(
                "{sql_base} WHERE source_category ILIKE $1
                 ORDER BY published_date DESC NULLS LAST, created_at DESC
                 LIMIT $2 OFFSET $3"
            ))
            .bind(cat)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query(&format!(
                "{sql_base} ORDER BY published_date DESC NULLS LAST, created_at DESC
                 LIMIT $1 OFFSET $2"
            ))
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows.iter().map(map_list_item).collect())
}

/// Returns a single document by id (with content).
pub async fn get_document_by_id(pool: &PgPool, id: i32) -> Result<Document> {
    let row = sqlx::query(
        r#"
        SELECT id, source_category, title, source_url, pdf_url,
               content, published_date, created_at, updated_at
        FROM documents
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.as_ref().map(map_document).ok_or(AppError::NotFound)
}

/// Returns the latest document per category.
pub async fn get_latest_per_category(pool: &PgPool) -> Result<Vec<DocumentListItem>> {
    let categories = ["Price Adjustments", "North Luzon Pump Prices"];
    let mut results = Vec::new();

    for cat in &categories {
        let row = sqlx::query(
            r#"
            SELECT id, source_category, title, source_url, pdf_url,
                   published_date, created_at, updated_at
            FROM documents
            WHERE source_category = $1
            ORDER BY published_date DESC NULLS LAST, created_at DESC
            LIMIT 1
            "#,
        )
        .bind(cat)
        .fetch_optional(pool)
        .await?;

        if let Some(r) = row {
            results.push(map_list_item(&r));
        }
    }

    Ok(results)
}

/// Returns total document count and per-category counts.
pub async fn get_counts(pool: &PgPool) -> Result<(i64, std::collections::HashMap<String, i64>)> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents")
        .fetch_one(pool)
        .await?;

    let categories = ["Price Adjustments", "North Luzon Pump Prices"];
    let mut by_cat = std::collections::HashMap::new();
    for cat in &categories {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM documents WHERE source_category = $1",
        )
        .bind(cat)
        .fetch_one(pool)
        .await?;
        by_cat.insert(cat.to_string(), count);
    }

    Ok((total, by_cat))
}

/// Returns the most recent `created_at` timestamp in the table.
pub async fn get_latest_created_at(pool: &PgPool) -> Result<Option<DateTime<Utc>>> {
    let ts: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT created_at FROM documents ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    Ok(ts)
}

/// Returns true if the documents table is empty.
pub async fn is_empty(pool: &PgPool) -> Result<bool> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents")
        .fetch_one(pool)
        .await?;
    Ok(count == 0)
}

/// Checks whether a document with the given pdf_url already exists.
pub async fn exists_by_pdf_url(pool: &PgPool, pdf_url: &str) -> Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM documents WHERE pdf_url = $1")
            .bind(pdf_url)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

/// Inserts a new document row. Returns the assigned id (0 if conflicted).
pub async fn insert_document(pool: &PgPool, doc: &NewDocument) -> Result<i32> {
    let row = sqlx::query(
        r#"
        INSERT INTO documents (source_category, title, source_url, pdf_url, content, published_date)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (pdf_url) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(&doc.source_category)
    .bind(&doc.title)
    .bind(&doc.source_url)
    .bind(&doc.pdf_url)
    .bind(&doc.content)
    .bind(doc.published_date)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.get::<i32, _>("id")).unwrap_or(0))
}

/// Deletes documents older than 2 months and removes filename-based duplicates.
/// Returns (outdated_deleted, duplicate_deleted).
pub async fn cleanup_outdated(pool: &PgPool) -> Result<(u64, u64)> {
    let two_months_ago = Utc::now() - chrono::Duration::days(60);

    // 1. Delete old records
    let outdated = sqlx::query(
        "DELETE FROM documents WHERE published_date < $1 AND published_date IS NOT NULL",
    )
    .bind(two_months_ago)
    .execute(pool)
    .await?
    .rows_affected();

    if outdated > 0 {
        tracing::info!("Cleanup: removed {} outdated document(s)", outdated);
    }

    // 2. Deduplicate by PDF filename (keep newest created_at per filename)
    let rows = sqlx::query("SELECT id, pdf_url FROM documents ORDER BY created_at DESC")
        .fetch_all(pool)
        .await?;

    let mut seen = std::collections::HashSet::new();
    let mut dup_ids: Vec<i32> = Vec::new();

    for row in &rows {
        let pdf_url: String = row.get("pdf_url");
        let filename = pdf_url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_lowercase();

        if filename.is_empty() {
            continue;
        }
        if seen.contains(&filename) {
            dup_ids.push(row.get("id"));
        } else {
            seen.insert(filename);
        }
    }

    let duplicate = if !dup_ids.is_empty() {
        sqlx::query("DELETE FROM documents WHERE id = ANY($1)")
            .bind(&dup_ids)
            .execute(pool)
            .await?
            .rows_affected()
    } else {
        0
    };

    if duplicate > 0 {
        tracing::info!("Cleanup: removed {} duplicate document(s)", duplicate);
    }

    Ok((outdated, duplicate))
}
