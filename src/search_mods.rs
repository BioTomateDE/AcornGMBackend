use sqlx::PgPool;
use uuid::Uuid;

// PostgreSQL query for basic search
pub async fn basic_search(pool: &PgPool, raw_query: &str) -> Result<Vec<Uuid>, String> {
    // Normalize the query: remove punctuation, handle whitespace
    let normalized_query = raw_query
        .replace(|c: char| !c.is_alphanumeric() && !c.is_whitespace(), " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" & ");  // AND operator for tsquery

    let records = sqlx::query!(
        r#"
        SELECT 
            id,
            title,
            description,
            -- Combined relevance score with:
            -- 1. Standard full-text search ranking
            ts_rank_cd(
                setweight(to_tsvector('english', title), 'A') || 
                setweight(to_tsvector('english', description), 'B'),
                to_tsquery($1)
            ) * 0.7 + 
            -- 2. Bonus for exact phrase matches (ordered terms)
            ts_rank_cd(
                setweight(to_tsvector('english', title), 'A') || 
                setweight(to_tsvector('english', description), 'B'),
                phraseto_tsquery('english', $2)
            ) * 0.3 AS relevance
        FROM mods
        WHERE 
            to_tsvector('english', title) @@ to_tsquery($1) OR
            to_tsvector('english', description) @@ to_tsquery($1)
        ORDER BY relevance DESC
        LIMIT 50
        "#,
        normalized_query,
        raw_query,
    )
        .fetch_all(pool)
        .await
        .map_err(|e|format!("Could not search mods (basic mode): {e}"))?;

    Ok(records.iter().map(|i| i.id).collect())
}


