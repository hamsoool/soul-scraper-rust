# Soul Scraper — DOE Philippines Fuel Price API

A REST API that automatically monitors, scrapes, and aggregates fuel price PDFs from the [Department of Energy Philippines](https://doe.gov.ph) website. Built with FastAPI and deployed on Render, with data stored in Supabase PostgreSQL.

**Base URL:** `https://soul-scaper.onrender.com`

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

### `GET /health`
Simple health check to confirm the API is running.

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

### `GET /documents/{id}`
Returns full details of a specific document including its extracted PDF text content.

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
Returns the single most recent document for each category. Useful for quick checks on the latest price adjustments.

**Response:**
```json
[
  {
    "id": 42,
    "source_category": "Price Adjustments",
    ...
  },
  {
    "id": 38,
    "source_category": "North Luzon Pump Prices",
    ...
  }
]
```

---

### `GET /stats`
Returns database statistics and current scraper status.

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

This API uses a **hybrid sync architecture** due to DOE's IP-based access restrictions on cloud providers:

```
Local Machine (Windows Task Scheduler — daily at 12:00 AM)
  └─ sync.py
       ├─ scrapes doe.gov.ph using local IP
       ├─ downloads & parses PDFs (PyMuPDF)
       └─ writes extracted data to Supabase

Supabase PostgreSQL (shared database)
  └─ soul-scaper.onrender.com (Render — free tier)
       ├─ GET /documents
       ├─ GET /latest
       └─ GET /stats
```

The scraper runs locally because the DOE website blocks requests from cloud provider IP ranges (Render, GitHub Actions, etc.). The Render deployment handles all read-only API traffic.

---

## Local Setup

### 1. Clone & Install

```bash
git clone https://github.com/hamsoool/soul-scaper.git
cd soul-scaper
python -m venv venv
venv\Scripts\activate        # Windows
pip install -r requirements.txt
```

### 2. Configure Environment

Copy the example env file and fill in your values:

```bash
cp .env.example .env
```

```env
# Supabase PostgreSQL (production)
DATABASE_URL=postgresql://postgres:[PASSWORD]@db.[PROJECT].supabase.co:5432/postgres

# Or SQLite for local testing (no Supabase needed)
# DATABASE_URL=sqlite+aiosqlite:///./doe_scraper.db
```

### 3. Run the API Server

```bash
uvicorn app.main:app --reload
```

Interactive Swagger docs available at: **http://127.0.0.1:8000/docs**

### 4. Run a Manual Sync

```bash
python sync.py
```

---

## Tech Stack

| Component | Technology |
| :--- | :--- |
| Web Framework | FastAPI (async) |
| Database | PostgreSQL via Supabase (`asyncpg`) / SQLite (local) |
| ORM | SQLAlchemy 2.0 (async) |
| HTTP Client | HTTPX (streamed downloads) |
| HTML Parsing | BeautifulSoup4 |
| PDF Extraction | PyMuPDF (fitz) |
| Scheduler | APScheduler (`AsyncIOScheduler`) |
| Deployment | Docker on Render (free tier) |

---

## Security

- **SSRF Prevention**: All URLs are validated and DNS-resolved before fetching. Only `doe.gov.ph` and `prod-cms.doe.gov.ph` resolving to public IPs are allowed.
- **Size Limits**: PDF downloads are streamed and aborted if they exceed 10 MB.
- **Timeouts**: All HTTP requests are capped at 30 seconds.
- **Thread Safety**: Blocking PDF parsing is offloaded to `asyncio.to_thread` to keep the API responsive.

---

## Disclaimer

This is an independent data aggregation tool and is not affiliated with, endorsed by, or officially connected to the Department of Energy (DOE) of the Philippines. All data is sourced from publicly available records on the official DOE portal.

## License

MIT License — see [LICENSE](LICENSE) for details.
