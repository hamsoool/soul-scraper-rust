# Soul Scraper — Configurable Website Aggregator API (Rust Edition)

A high-performance REST API that scrapes, downloads, and aggregates files (PDFs, images, etc.) from any Nuxt.js-based CMS website.

Built with **Rust/Axum** for extreme throughput — all file downloads run concurrently via Tokio's `FuturesUnordered` with zero GC overhead.

---

## Configuration (`sources.json`)

All scrape targets are defined in a single JSON file at the project root. Copy the example to get started:

```bash
cp sources.example.json sources.json
```

Then edit `sources.json` to point at your target:

```json
{
  "target_url": "https://example.gov.ph",
  "aggregators": [
    {
      "url": "https://example.gov.ph/articles/category-1",
      "category": "Category 1",
      "file_types": ["pdf"]
    },
    {
      "url": "https://example.gov.ph/articles/category-2",
      "category": "Category 2",
      "file_types": ["pdf", "jpg", "png"]
    }
  ]
}
```

| Field | Description |
| :--- | :--- |
| `target_url` | Base URL of the target website (used to resolve relative links) |
| `aggregators[].url` | Specific Nuxt.js page to scrape for file links |
| `aggregators[].category` | Label assigned to files found on this page |
| `aggregators[].file_types` | File extensions to look for (e.g. `pdf`, `jpg`, `png`, `xlsx`) |

> `sources.json` is git-ignored — each machine has its own config. Use `sources.example.json` as a reference.

Set a custom path with the `SOURCES_CONFIG_PATH` env var (default: `sources.json`).

---

## API Endpoints

### `GET /`
Index welcome page showing available endpoints.

---

### `GET /health`
Simple health check.

**Response:**
```json
{ "status": "ok" }
```

---

### `GET /categories`
Returns the full scrape configuration (target URL + all aggregators).

**Response:**
```json
{
  "target_url": "https://example.gov.ph",
  "aggregators": [
    {
      "url": "https://example.gov.ph/articles/category-1",
      "category": "Category 1",
      "file_types": ["pdf"]
    }
  ]
}
```

---

### `GET /documents`
Returns a paginated list of aggregated documents, sorted by most recent first.

**Query Parameters:**

| Parameter | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `limit` | integer | `20` | Number of results (max: 100) |
| `offset` | integer | `0` | Pagination offset |
| `category` | string | — | Filter by any configured category |

**Example:**
```
GET /documents?category=Category%201&limit=5
```

**Response:**
```json
[
  {
    "id": 42,
    "source_category": "Category 1",
    "title": "Example Document",
    "source_url": "https://example.gov.ph/articles/...",
    "pdf_url": "https://cms.example.gov.ph/documents/...",
    "published_date": "2026-06-24T00:00:00Z",
    "created_at": "2026-06-25T00:03:12Z",
    "updated_at": "2026-06-25T00:03:12Z"
  }
]
```

---

### `GET /documents/:id`
Returns full details of a specific document, including extracted text content (PDF only).

**Example:**
```
GET /documents/42
```

**Response:**
```json
{
  "id": 42,
  "source_category": "Category 1",
  "title": "Example Document",
  "source_url": "https://example.gov.ph/articles/...",
  "pdf_url": "https://cms.example.gov.ph/documents/...",
  "published_date": "2026-06-24T00:00:00Z",
  "created_at": "2026-06-25T00:03:12Z",
  "updated_at": "2026-06-25T00:03:12Z",
  "content": "Extracted PDF text content..."
}
```

---

### `GET /latest`
Returns the single most recent document for **each** category found in the database.

**Response:**
```json
[
  {
    "id": 42,
    "source_category": "Category 1",
    "title": "Example Document",
    ...
  }
]
```

---

### `GET /stats`
Returns database statistics and scraper status.

**Response:**
```json
{
  "total_documents": 24,
  "documents_by_category": {
    "Category 1": 14,
    "Category 2": 10
  },
  "last_sync_time": "2026-06-25T00:03:45Z",
  "system_status": "idle"
}
```

`documents_by_category` auto-populates from all categories in the database.

---

### `POST /sync`
Triggers a manual scrape in the background. Returns immediately with `202 Accepted`.

**Response:**
```json
{
  "status": "accepted",
  "message": "Manual synchronization has been queued and is executing in the background.",
  "processed_count": 0,
  "errors": []
}
```

---

## Architecture

The API uses a **hybrid modular architecture** — the sync CLI runs on your local machine (avoids cloud IP blocks), writes to a shared database, and the API server reads from it.

```
┌─────────────────────────────────────────────────────────────┐
│             Local Machine (Windows/Linux Sync CLI)          │
│               - Executes `cargo run --bin sync`             │
│               - Reads targets from sources.json             │
│               - Scrapes configured Nuxt.js pages            │
│               - Parallelizes file downloads (Tokio)         │
└──────────────┬──────────────────────────────────────────────┘
               │
               │ Writes data directly
               ▼
┌─────────────────────────────────────────────────────────────┐
│                 PostgreSQL Database (Supabase/Render)       │
│            - Shared single-connection point                 │
└──────────────▲──────────────────────────────────────────────┘
               │
               │ Reads data for API clients
               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Hosted API Web Service (Render/Docker)      │
│               - Dockerized Axum HTTP server                 │
│               - Serves GET /documents, /latest, etc.        │
└─────────────────────────────────────────────────────────────┘
```

---

## Local Setup

### 1. Clone & Pre-requisites

```bash
git clone https://github.com/hamsoool/soul-scraper-rust.git
cd soul-scraper-rust
```

Make sure you have the [Rust toolchain installed](https://rustup.rs/).

### 2. Configure Environment

Copy the example env file and add your database URL:

```bash
copy .env.example .env
```

```env
DATABASE_URL=postgresql://postgres:[PASSWORD]@localhost:5432/scraper
```

### 3. Configure Scrape Targets

Copy the example config and edit it:

```bash
copy sources.example.json sources.json
```

Then edit `sources.json` with your target website, categories, and file types. See the [Configuration](#configuration-sourcesjson) section above for the format.

> Allowed domains are **automatically extracted** from the URLs you configure. No separate allowlist needed.

### 4. Install PDFium Binary (Windows — for PDF text extraction)

```powershell
Invoke-WebRequest -Uri "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.zip" -OutFile "pdfium.zip"
Expand-Archive -Path "pdfium.zip" -DestinationPath "pdfium-temp"
Copy-Item -Path "pdfium-temp\bin\pdfium.dll" -Destination "pdfium.dll"
Remove-Item -Path "pdfium.zip", "pdfium-temp" -Recurse -Force
```

### 5. Run the API Server

```bash
cargo run
```

The server will listen on `http://127.0.0.1:8000/`.

### 6. Run a Local Sync CLI

```bash
cargo run --bin sync
```

---

## Tech Stack

| Component | Technology |
| :--- | :--- |
| Language | **Rust** (Edition 2021) |
| Web Framework | **Axum 0.7** (zero-copy routing stack) |
| Async Runtime | **Tokio** (multi-threaded, work-stealing scheduler) |
| Database | PostgreSQL via **SQLx** (async pool + automatic migration engine) |
| HTTP Client | **reqwest** (gzip/brotli streaming client) |
| HTML Parsing | **scraper** crate (CSS selection engine) |
| PDF Extraction | **pdfium-render** (Google PDFium C++ bindings) |
| Configuration | **JSON** file (`sources.json`), reloaded on restart |
| Containerization | **Docker** (Multi-stage build using `rust:slim` -> `debian:slim`) |

---

## Security & Reliability

- **SSRF Prevention**: All URLs are strictly validated and DNS-resolved. Local loopback / private IPs are blocked. The allowlist is automatically populated from `sources.json` at startup.
- **Buffer Safety**: File downloads are streamed and aborted immediately if they exceed 10 MB.
- **Resource Offloading**: Heavy, CPU-bound PDF text extraction is safely spawned inside Tokio's `spawn_blocking` threadpool to ensure the main server event loop is never blocked.

---

## License

MIT License — see [LICENSE](LICENSE) for details.
