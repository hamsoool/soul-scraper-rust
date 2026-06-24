import logging
from contextlib import asynccontextmanager
from typing import List, Optional

from fastapi import FastAPI, Depends, HTTPException, BackgroundTasks, Query, status
from fastapi.middleware.cors import CORSMiddleware
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.future import select
from sqlalchemy import func

from app.config import settings
from app.database import engine, Base, get_db, async_session
from app.models import Document
from app.schemas import DocumentListItem, DocumentRead, StatsResponse, SyncResponse
from app.scheduler import start_scheduler, stop_scheduler, sync_state, execute_sync_job
from app.scraper import sync_doe_data

# Configure logging format and levels
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s"
)
logger = logging.getLogger(__name__)

@asynccontextmanager
async def lifespan(app: FastAPI):
    # Startup actions
    logger.info("Initializing database and starting services...")
    async with engine.begin() as conn:
        # Automatically creates tables if they don't exist
        await conn.run_sync(Base.metadata.create_all)
    
    # Start APScheduler for the 24h background job if enabled
    if settings.ENABLE_SCRAPER_SCHEDULER:
        start_scheduler()
        
        # If the database is completely empty, trigger a sync task in the background
        async with async_session() as session:
            result = await session.execute(select(func.count(Document.id)))
            count = result.scalar()
            if count == 0:
                logger.info("Database is empty. Queueing first sync job...")
                asyncio_loop = asyncio.get_event_loop()
                asyncio_loop.create_task(execute_sync_job())
            
    yield
    
    # Shutdown actions
    logger.info("Shutting down services...")
    if settings.ENABLE_SCRAPER_SCHEDULER:
        stop_scheduler()
    await engine.dispose()

import asyncio

app = FastAPI(
    title="Department of Energy Philippines PDF Aggregator API",
    description="A highly optimized API to fetch, scrape, and aggregate DOE price adjustments and NCR pump price PDFs.",
    version="1.0.0",
    lifespan=lifespan
)

# Enable CORS for cross-origin frontend queries
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

@app.get("/health", tags=["System"])
async def health_check():
    """Simple API health check endpoint."""
    return {"status": "ok", "timestamp": func.now()}

@app.get("/documents", response_model=List[DocumentListItem], tags=["Documents"])
async def list_documents(
    category: Optional[str] = Query(None, description="Filter by category (e.g. 'Price Adjustments' or 'NCR Pump Prices')"),
    limit: int = Query(20, ge=1, le=100, description="Number of items to retrieve"),
    offset: int = Query(0, ge=0, description="Offset for pagination"),
    db: AsyncSession = Depends(get_db)
):
    """Retrieves a paginated list of aggregated documents, optionally filtered by category."""
    stmt = select(Document).order_by(Document.published_date.desc(), Document.created_at.desc())
    if category:
        stmt = stmt.filter(Document.source_category.ilike(category))
        
    stmt = stmt.offset(offset).limit(limit)
    result = await db.execute(stmt)
    documents = result.scalars().all()
    return documents

@app.get("/documents/{id}", response_model=DocumentRead, tags=["Documents"])
async def get_document(id: int, db: AsyncSession = Depends(get_db)):
    """Retrieves detailed information for a specific document, including its parsed text content."""
    stmt = select(Document).filter(Document.id == id)
    result = await db.execute(stmt)
    document = result.scalars().first()
    if not document:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Document with ID {id} not found."
        )
    return document

@app.get("/latest", response_model=List[DocumentListItem], tags=["Documents"])
async def get_latest_documents(db: AsyncSession = Depends(get_db)):
    """Retrieves the single most recent document for each of the source categories."""
    categories = ["Price Adjustments", "NCR Pump Prices"]
    latest_docs = []
    
    for cat in categories:
        stmt = (
            select(Document)
            .filter(Document.source_category == cat)
            .order_by(Document.published_date.desc(), Document.created_at.desc())
            .limit(1)
        )
        result = await db.execute(stmt)
        doc = result.scalars().first()
        if doc:
            latest_docs.append(doc)
            
    return latest_docs

async def run_manual_sync():
    """Wrapper task for manual background sync."""
    if sync_state["is_syncing"]:
        logger.warning("Manual sync requested but a sync session is already in progress.")
        return
        
    sync_state["is_syncing"] = True
    async with async_session() as session:
        try:
            result = await sync_doe_data(session)
            sync_state["last_sync_time"] = datetime.now(timezone.utc)
            sync_state["last_sync_result"] = result
            logger.info(f"Manual sync completed. Result: {result}")
        except Exception as e:
            logger.error(f"Error during manual sync: {e}")
            sync_state["last_sync_result"] = {"status": "error", "message": str(e)}
        finally:
            sync_state["is_syncing"] = False

@app.post("/sync", response_model=SyncResponse, status_code=status.HTTP_202_ACCEPTED, tags=["System"])
async def trigger_sync(background_tasks: BackgroundTasks):
    """Triggers the DOE website scraper manually in the background without blocking the API."""
    if sync_state["is_syncing"]:
        return SyncResponse(
            status="running",
            message="A synchronization session is already in progress.",
            processed_count=0,
            errors=[]
        )
        
    # Queue the sync execution to run in FastAPI's background thread/task pool
    background_tasks.add_task(run_manual_sync)
    
    return SyncResponse(
        status="accepted",
        message="Manual synchronization has been queued and is executing in the background.",
        processed_count=0,
        errors=[]
    )

@app.get("/stats", response_model=StatsResponse, tags=["System"])
async def get_stats(db: AsyncSession = Depends(get_db)):
    """Returns diagnostics and summary statistics about the database and scraper runs."""
    # Count total documents
    total_result = await db.execute(select(func.count(Document.id)))
    total_count = total_result.scalar() or 0
    
    # Count documents per category
    categories = ["Price Adjustments", "NCR Pump Prices"]
    cat_counts = {}
    for cat in categories:
        cat_result = await db.execute(select(func.count(Document.id)).filter(Document.source_category == cat))
        cat_counts[cat] = cat_result.scalar() or 0
        
    system_status = "syncing" if sync_state["is_syncing"] else "idle"
    
    # Get last sync time from DB if state has none (e.g. server restarted recently)
    last_sync = sync_state["last_sync_time"]
    if not last_sync:
        latest_doc_stmt = select(Document.created_at).order_by(Document.created_at.desc()).limit(1)
        latest_doc_result = await db.execute(latest_doc_stmt)
        last_sync = latest_doc_result.scalar()
        
    return StatsResponse(
        total_documents=total_count,
        documents_by_category=cat_counts,
        last_sync_time=last_sync,
        system_status=system_status
    )
