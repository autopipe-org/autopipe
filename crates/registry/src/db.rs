use common::models::{Pipeline, PipelineSummary};
use tokio_postgres::Client;

pub struct DbState {
    pub client: Client,
}

const CREATE_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS user_pipelines (
    pipeline_id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    tools TEXT[],
    input_formats TEXT[],
    output_formats TEXT[],
    tags TEXT[],
    snakefile TEXT NOT NULL,
    dockerfile TEXT NOT NULL,
    config_yaml TEXT,
    metadata_json JSONB NOT NULL,
    readme TEXT,
    author VARCHAR(255),
    version VARCHAR(50) DEFAULT '1.0.0',
    verified BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_user_pipelines_name ON user_pipelines(name);
CREATE INDEX IF NOT EXISTS idx_user_pipelines_tools ON user_pipelines USING GIN(tools);
CREATE INDEX IF NOT EXISTS idx_user_pipelines_tags ON user_pipelines USING GIN(tags);
"#;

pub async fn ensure_table(client: &Client) -> Result<(), tokio_postgres::Error> {
    client.batch_execute(CREATE_TABLE_SQL).await
}

pub async fn list_pipelines(client: &Client) -> Result<Vec<PipelineSummary>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT pipeline_id, name, description, tools, input_formats, output_formats, \
             tags, author, version, verified, created_at::text \
             FROM user_pipelines ORDER BY created_at DESC",
            &[],
        )
        .await?;

    Ok(rows
        .iter()
        .map(|r| PipelineSummary {
            pipeline_id: r.get(0),
            name: r.get(1),
            description: r.get::<_, Option<String>>(2).unwrap_or_default(),
            tools: r.get::<_, Option<Vec<String>>>(3).unwrap_or_default(),
            input_formats: r.get::<_, Option<Vec<String>>>(4).unwrap_or_default(),
            output_formats: r.get::<_, Option<Vec<String>>>(5).unwrap_or_default(),
            tags: r.get::<_, Option<Vec<String>>>(6).unwrap_or_default(),
            author: r.get::<_, Option<String>>(7).unwrap_or_default(),
            version: r.get::<_, Option<String>>(8).unwrap_or_else(|| "1.0.0".into()),
            verified: r.get::<_, Option<bool>>(9).unwrap_or(false),
            created_at: r.get(10),
        })
        .collect())
}

pub async fn search_pipelines(
    client: &Client,
    query: &str,
) -> Result<Vec<PipelineSummary>, tokio_postgres::Error> {
    let pattern = format!("%{}%", query);
    let rows = client
        .query(
            "SELECT pipeline_id, name, description, tools, input_formats, output_formats, \
             tags, author, version, verified, created_at::text \
             FROM user_pipelines \
             WHERE name ILIKE $1 OR description ILIKE $2 OR $3 = ANY(tools) OR $3 = ANY(tags) \
             ORDER BY created_at DESC",
            &[&pattern, &pattern, &query],
        )
        .await?;

    Ok(rows
        .iter()
        .map(|r| PipelineSummary {
            pipeline_id: r.get(0),
            name: r.get(1),
            description: r.get::<_, Option<String>>(2).unwrap_or_default(),
            tools: r.get::<_, Option<Vec<String>>>(3).unwrap_or_default(),
            input_formats: r.get::<_, Option<Vec<String>>>(4).unwrap_or_default(),
            output_formats: r.get::<_, Option<Vec<String>>>(5).unwrap_or_default(),
            tags: r.get::<_, Option<Vec<String>>>(6).unwrap_or_default(),
            author: r.get::<_, Option<String>>(7).unwrap_or_default(),
            version: r.get::<_, Option<String>>(8).unwrap_or_else(|| "1.0.0".into()),
            verified: r.get::<_, Option<bool>>(9).unwrap_or(false),
            created_at: r.get(10),
        })
        .collect())
}

pub async fn get_pipeline(
    client: &Client,
    id: i32,
) -> Result<Option<Pipeline>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT pipeline_id, name, description, tools, input_formats, output_formats, \
             tags, snakefile, dockerfile, config_yaml, metadata_json, readme, \
             author, version, verified, created_at::text, updated_at::text \
             FROM user_pipelines WHERE pipeline_id = $1",
            &[&id],
        )
        .await?;

    Ok(row.map(|r| Pipeline {
        pipeline_id: Some(r.get(0)),
        name: r.get(1),
        description: r.get::<_, Option<String>>(2).unwrap_or_default(),
        tools: r.get::<_, Option<Vec<String>>>(3).unwrap_or_default(),
        input_formats: r.get::<_, Option<Vec<String>>>(4).unwrap_or_default(),
        output_formats: r.get::<_, Option<Vec<String>>>(5).unwrap_or_default(),
        tags: r.get::<_, Option<Vec<String>>>(6).unwrap_or_default(),
        snakefile: r.get::<_, Option<String>>(7).unwrap_or_default(),
        dockerfile: r.get::<_, Option<String>>(8).unwrap_or_default(),
        config_yaml: r.get::<_, Option<String>>(9).unwrap_or_default(),
        metadata_json: r.get(10),
        readme: r.get::<_, Option<String>>(11).unwrap_or_default(),
        author: r.get::<_, Option<String>>(12).unwrap_or_default(),
        version: r.get::<_, Option<String>>(13).unwrap_or_else(|| "1.0.0".into()),
        verified: r.get::<_, Option<bool>>(14).unwrap_or(false),
        created_at: r.get(15),
        updated_at: r.get(16),
    }))
}

pub async fn insert_pipeline(
    client: &Client,
    p: &Pipeline,
) -> Result<i32, tokio_postgres::Error> {
    let row = client
        .query_one(
            "INSERT INTO user_pipelines \
             (name, description, tools, input_formats, output_formats, tags, \
              snakefile, dockerfile, config_yaml, metadata_json, readme, \
              author, version, verified) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
             RETURNING pipeline_id",
            &[
                &p.name,
                &p.description,
                &p.tools,
                &p.input_formats,
                &p.output_formats,
                &p.tags,
                &p.snakefile,
                &p.dockerfile,
                &p.config_yaml,
                &p.metadata_json,
                &p.readme,
                &p.author,
                &p.version,
                &p.verified,
            ],
        )
        .await?;
    Ok(row.get(0))
}

pub async fn update_pipeline(
    client: &Client,
    id: i32,
    p: &Pipeline,
) -> Result<bool, tokio_postgres::Error> {
    let count = client
        .execute(
            "UPDATE user_pipelines SET \
             description = $1, tools = $2, input_formats = $3, output_formats = $4, \
             tags = $5, snakefile = $6, dockerfile = $7, config_yaml = $8, \
             metadata_json = $9, readme = $10, author = $11, version = $12, \
             verified = $13, updated_at = CURRENT_TIMESTAMP \
             WHERE pipeline_id = $14",
            &[
                &p.description,
                &p.tools,
                &p.input_formats,
                &p.output_formats,
                &p.tags,
                &p.snakefile,
                &p.dockerfile,
                &p.config_yaml,
                &p.metadata_json,
                &p.readme,
                &p.author,
                &p.version,
                &p.verified,
                &id,
            ],
        )
        .await?;
    Ok(count > 0)
}

pub async fn delete_pipeline(
    client: &Client,
    id: i32,
) -> Result<bool, tokio_postgres::Error> {
    let count = client
        .execute("DELETE FROM user_pipelines WHERE pipeline_id = $1", &[&id])
        .await?;
    Ok(count > 0)
}
