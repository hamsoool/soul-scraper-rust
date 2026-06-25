# Soul Scraper — DOE Philippines Fuel Price API (Rust Edition)

A high-performance REST API that automatically monitors, scrapes, and aggregates fuel price PDFs from the [Department of Energy Philippines](https://doe.gov.ph) website. 

Rebuilt from Python/FastAPI to **Rust/Axum** for extreme raw throughput, concurrent PDF downloads using Tokio's `FuturesUnordered`, and zero garbage collection overhead.

**Base URL:** `https://soul-scrape-rust.onrender.com`

> ⚠️ Free-tier Render service — may take 30–60 seconds to wake up after inactivity.

---

## Data Sources

The API aggregates PDFs from two DOE page categories:

| Category | Source |
| :--- | :--- |
| **Price Adjustments** | Retail pump price adjustment bulletins (past 14 days) |
| **North Luzon Pump Prices** | Weekly North Luzon regional price monitoring (current month) |

---

## API Endpoints

### `GET /`
Index welcome page showing service metadata and available endpoints.

---

### `GET /health`
Simple health check to confirm the API and database are online.

**Response:**
```json
{ "status": "ok" }
```

---

### `GET /documents`
Returns a paginated list of aggregated documents, sorted by most recent first.

**Query Parameters:**

| Parameter | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `limit` | integer | `20` | Number of results (max: 100) |
| `offset` | integer | `0` | Pagination offset |
| `category` | string | — | Filter by `Price Adjustments` or `North Luzon Pump Prices` |

**Example:**
```
GET /documents?category=Price Adjustments&limit=5
```

**Response:**
```json
[
  {
    "id": 42,
    "source_category": "Price Adjustments",
    "title": "Price Adjustment Effective June 24, 2026",
    "source_url": "https://doe.gov.ph/articles/...",
    "pdf_url": "https://prod-cms.doe.gov.ph/documents/...",
    "published_date": "2026-06-24T00:00:00Z",
    "created_at": "2026-06-25T00:03:12Z",
    "updated_at": "2026-06-25T00:03:12Z"
  }
]
```

---

### `GET /documents/:id`
Returns full details of a specific document, including its extracted PDF text content.

**Example:**
```
GET /documents/42
```

**Response:**
```json
{
  "id": 42,
  "source_category": "Price Adjustments",
  "title": "Price Adjustment Effective June 24, 2026",
  "source_url": "https://doe.gov.ph/articles/...",
  "pdf_url": "https://prod-cms.doe.gov.ph/documents/...",
  "published_date": "2026-06-24T00:00:00Z",
  "created_at": "2026-06-25T00:03:12Z",
  "updated_at": "2026-06-25T00:03:12Z",
  "content": "Effectivity: June 24, 2026\nGasoline: -0.40/liter\nDiesel: -0.25/liter..."
}
```

---

### `GET /latest`
Returns the single most recent document for each category.

**Response:**
```json
[
  {
    "id": 42,
    "source_category": "Price Adjustments",
    "title": "Price Adjustment Effective June 24, 2026",
    ...
  },
  {
    "id": 38,
    "source_category": "North Luzon Pump Prices",
    "title": "List of North Luzon Pump Prices - CAR",
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
    "Price Adjustments": 14,
    "North Luzon Pump Prices": 10
  },
  "last_sync_time": "2026-06-25T00:03:45Z",
  "system_status": "idle"
}
```

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

This API uses a **hybrid modular architecture** to avoid cloud provider IP blocks from the DOE portal. You can run the sync CLI locally to load data directly into Supabase:

```
┌─────────────────────────────────────────────────────────────┐
│             Local Machine (Windows/Linux Sync CLI)          │
│               - Executes `cargo run --bin sync`             │
│               - Scrapes DOE portal using local IP           │
│               - Parallelizes PDF downloads (Tokio)          │
└──────────────┬──────────────────────────────────────────────┘
               │
               │ Writes data directly
               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Supabase PostgreSQL Database                │
│            - Remote shared database cluster                 │
└──────────────▲──────────────────────────────────────────────┘
               │
               │ Reads data for API clients
               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Render Hosted API Web Service               │
│               - Dockerized Axum HTTP server                 │
│               - Hosts GET /documents, /latest, etc.         │
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

Copy the example env file and add your Supabase database URL:

```bash
copy .env.example .env
```

```env
DATABASE_URL=postgresql://postgres.[YOUR_PROJECT_ID]:[PASSWORD]@aws-1-ap-southeast-1.pooler.supabase.com:5432/postgres
```

### 3. Install PDFium Binary (Windows Local Requirement)

To run the scraper locally on Windows, download the PDFium dynamic library (`pdfium.dll`) using this PowerShell command:

```powershell
Invoke-WebRequest -Uri "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.zip" -OutFile "pdfium.zip"
Expand-Archive -Path "pdfium.zip" -DestinationPath "pdfium-temp"
Copy-Item -Path "pdfium-temp\bin\pdfium.dll" -Destination "pdfium.dll"
Remove-Item -Path "pdfium.zip", "pdfium-temp" -Recurse -Force
```

### 4. Run the API Server

```bash
cargo run
```

The server will listen on `http://127.0.0.1:8000/`.

### 5. Run a Local Sync CLI

To manually trigger a local sync using your machine's IP address:

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
| Containerization | **Docker** (Multi-stage build using `rust:slim` -> `debian:slim`) |

---

## Security & Reliability

- **SSRF Prevention**: All URLs are strictly validated and DNS-resolved. Local loopback / private IPs are blocked, ensuring only valid public DOE IPs are accessed.
- **Buffer Safety**: PDF downloads are streamed and aborted immediately if they exceed 10 MB.
- **Resource Offloading**: Heavy, CPU-bound PDF text extraction is safely spawned inside Tokio's `spawn_blocking` threadpool to ensure the main server event loop is never blocked.

---

## Disclaimer

This is an independent data aggregation tool and is not affiliated with, endorsed by, or officially connected to the Department of Energy (DOE) of the Philippines.

## License

MIT License — see [LICENSE](LICENSE) for details.
