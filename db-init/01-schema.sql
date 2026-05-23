-- Initial schema for autopipe_wf.
-- Mirrors web/src/lib/server/schema.ts (drizzle source of truth).
-- Postgres runs every *.sql in /docker-entrypoint-initdb.d/ exactly once,
-- on first boot when the data directory is empty.

CREATE TABLE IF NOT EXISTS user_pipelines (
    pipeline_id    SERIAL PRIMARY KEY,
    name           VARCHAR(255) NOT NULL,
    description    TEXT,
    tools          TEXT[],
    input_formats  TEXT[],
    output_formats TEXT[],
    tags           TEXT[],
    github_url     VARCHAR(500) NOT NULL,
    metadata_json  JSONB NOT NULL,
    author         VARCHAR(255),
    version        VARCHAR(50) DEFAULT '1.0.0',
    verified       BOOLEAN DEFAULT FALSE,
    forked_from    INTEGER,
    based_on_url   VARCHAR(500),
    -- AI-detected suspicious patterns the publisher explicitly approved.
    -- Each entry: { file, line, code_snippet, concern, category? }.
    -- Shown to downloaders so they can decide whether to accept the same risks.
    security_warnings JSONB DEFAULT '[]'::jsonb,
    -- Git tag and commit SHA captured at publish time. Downloads and Hub-side
    -- rendering use these to fetch the exact published source rather than the
    -- moving HEAD of the default branch. NULL on rows published before this
    -- feature; the application code falls back to the default branch then.
    git_tag        VARCHAR(255),
    commit_sha     CHAR(40),
    created_at     TIMESTAMP DEFAULT now(),
    updated_at     TIMESTAMP DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_user_pipelines_name    ON user_pipelines (name);
CREATE INDEX IF NOT EXISTS idx_user_pipelines_git_tag ON user_pipelines (git_tag);

CREATE TABLE IF NOT EXISTS user_plugins (
    plugin_id       SERIAL PRIMARY KEY,
    name            VARCHAR(255) NOT NULL,
    description     TEXT,
    category        VARCHAR(100),
    extensions      TEXT[] DEFAULT '{}',
    tags            TEXT[],
    github_url      VARCHAR(500) NOT NULL,
    metadata_json   JSONB NOT NULL,
    readme          TEXT,
    author          VARCHAR(255),
    version         VARCHAR(50) DEFAULT '1.0.0',
    verified        BOOLEAN DEFAULT FALSE,
    forked_from     INTEGER,
    version_history JSONB DEFAULT '[]'::jsonb,
    created_at      TIMESTAMP DEFAULT now(),
    updated_at      TIMESTAMP DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_user_plugins_name ON user_plugins (name);
