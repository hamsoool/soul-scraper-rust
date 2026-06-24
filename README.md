# DOE PDF Aggregator Service

A highly optimized PDF aggregation and extraction API designed to automatically monitor and scrape document updates from the Department of Energy (DOE) Philippines.

Specifically targets:
1. **Price Adjustments**: `https://doe.gov.ph/articles/group/liquid-fuels?maincat=Retail%20Pump%20Prices&subcategory=Price%20Adjustments&display_type=Card`
2. **NCR Pump Prices**: `https://doe.gov.ph/articles/group/liquid-fuels?maincat=Retail%20Pump%20Prices&subcategory=NCR%20Pump%20Prices&display_type=Card`

---

## Scraper Date Filtering Rules

To optimize database storage and keep records clean, the scraper implements dynamic date filters:
* **NCR Pump Prices**: Only downloads and extracts PDF links belonging to the **current calendar month and year** (e.g. only June 2026 reports).
* **Price Adjustments**: Only downloads and extracts PDFs published within the **past 2 weeks** (last 14 days) relative to the execution timestamp.

---

## Technical Architecture

- **Web Framework**: FastAPI (fully asynchronous architecture)
- **Database Backend**: PostgreSQL (SQLAlchemy ORM + `asyncpg` async connection pooling) or SQLite (for local development via `aiosqlite`)
- **Scraper & Parsing Engine**: HTTPX (streamed response downloads) + BeautifulSoup4 + PyMuPDF (asynchronous text extraction)
- **Scheduler**: APScheduler (`AsyncIOScheduler` executing inside event loop)

---

## Security Safeguards

- **SSRF Prevention**: All requested URLs are parsed and resolved via DNS. Only hostnames mapping to the specific allowed list (`doe.gov.ph`, `www.doe.gov.ph`, `prod-cms.doe.gov.ph`) and resolving to public, non-private IP addresses are fetched. Loopback, private networks, local hosts, internal networks, and invalid protocols are strictly rejected.
- **Resource Protection**:
  - HTTP client timeouts are strictly set (default: 30s).
  - PDF files are streamed in chunks and aborted immediately if the file size exceeds `10MB` to avoid memory exhaustion attacks.
  - Blocking operations (like PDF parsing) are offloaded to separate threads using `asyncio.to_thread` to maintain FastAPI responsiveness.

---

## REST API Endpoints

| Method | Endpoint | Description | Response Details |
| :--- | :--- | :--- | :--- |
| **GET** | `/health` | Diagnostics check. | `{"status": "ok"}` |
| **GET** | `/documents` | Query paginated list of documents. | Supports `limit`, `offset`, and `category` filtering. |
| **GET** | `/documents/{id}` | Retrieve details of a specific document. | Returns title, metadata, and full text content. |
| **GET** | `/latest` | Get the single newest document for each category. | Fast check on the latest retail adjustments. |
| **POST** | `/sync` | Trigger an immediate manual scrape in background. | Returns `202 Accepted` immediately (runs in background). |
| **GET** | `/stats` | View metrics, file counts, and scraper status. | Database document counts and last sync status. |

---

## Local Setup

### 1. Installation

Clone the project repository and copy the environment configuration template:
```bash
cp .env.example .env
```

Create a virtual environment and install production dependencies:
```bash
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate
pip install -r requirements.txt
```

### 2. Configure Your Database URL

Open the `.env` file in the root of the project and set **one** of the following options:

#### Option A: Supabase (Remote Serverless PostgreSQL)
Paste your connection URI from the Supabase Settings > Database dashboard:
```env
DATABASE_URL=postgresql://postgres:[YOUR-PASSWORD]@db.[YOUR-PROJECT-ID].supabase.co:5432/postgres
```
*(The service automatically translates the scheme to `postgresql+asyncpg://` for async communication).*

#### Option B: Zero-Setup SQLite (Local Testing)
Install the `aiosqlite` driver and define a local file database:
```bash
pip install aiosqlite
```
Configure your `.env`:
```env
DATABASE_URL=sqlite+aiosqlite:///./doe_scraper.db
```

### 3. Running the Server

Launch the local FastAPI development server:
```bash
uvicorn app.main:app --reload
```
You can now access the interactive API docs (Swagger UI) at **[http://127.0.0.1:8000/docs](http://127.0.0.1:8000/docs)**.

---

## Triggering a Manual Sync

You can trigger a scraper run manually while the server is running:

* **Interactive Docs**: Go to [http://127.0.0.1:8000/docs](http://127.0.0.1:8000/docs), expand the `POST /sync` block, click **Try it out** and then **Execute**.
* **Curl Command**:
  ```bash
  curl -X POST http://127.0.0.1:8000/sync
  ```
* **PowerShell**:
  ```powershell
  Invoke-RestMethod -Uri "http://127.0.0.1:8000/sync" -Method Post
  ```

---

## Deployment to Render

This service is configured for direct deployment on Render. Since you are using Supabase as your PostgreSQL database, you only need to deploy the FastAPI container:

1. Create a new **Web Service** on Render and connect your GitHub repository.
2. Configure settings:
   - **Runtime**: `Docker`
   - **Plan**: `Free`
3. Add the following **Environment Variables**:
   - `DATABASE_URL` = (Your Supabase Connection URI)
   - `PORT` = `8000`
   - `SYNC_INTERVAL_HOURS` = `24`
4. Deploy the service.
5. *(Optional)* Create a Render **Cron Job** with schedule `0 0 * * *` and command `curl -X POST https://your-service-name.onrender.com/sync` to ensure the server automatically wakes up and syncs every 24 hours.

---

## Disclaimer

This project is an independent data aggregation tool and is not affiliated, associated, authorized, endorsed by, or in any way officially connected with the Department of Energy (DOE) of the Philippines or any of its agencies. 

All retrieved data, documents, and prices are public records sourced directly from the official DOE portal and are subject to the original publisher's terms.

## License

This project is open-source software licensed under the [MIT License](LICENSE).

