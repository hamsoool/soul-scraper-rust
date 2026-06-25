-- Soul Scraper — Documents table
-- Mirrors the SQLAlchemy model in the original Python project exactly.

CREATE TABLE IF NOT EXISTS documents (
    id              SERIAL PRIMARY KEY,
    source_category VARCHAR(255)  NOT NULL,
    title           VARCHAR(1024) NOT NULL,
    source_url      VARCHAR(2048) NOT NULL,
    pdf_url         VARCHAR(2048) UNIQUE NOT NULL,
    content         TEXT,
    published_date  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_documents_pdf_url        ON documents (pdf_url);
CREATE INDEX IF NOT EXISTS idx_documents_published_date ON documents (published_date);
CREATE INDEX IF NOT EXISTS idx_documents_created_at     ON documents (created_at);
