use datafusion::prelude::*;
use std::time::Instant;

struct KnowledgeBase {
    ctx: SessionContext,
}

impl KnowledgeBase {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let ctx = SessionContext::new();
        
        ctx.sql("CREATE SCHEMA kb")
            .await?
            .collect()
            .await?;

        ctx.sql(r#"
            CREATE TABLE kb.nodes (
                id VARCHAR(64),
                content VARCHAR,
                doc VARCHAR,
                org VARCHAR,
                time TIMESTAMP
            )
        "#)
        .await?
        .collect()
        .await?;

        ctx.sql(r#"
            CREATE TABLE kb.edges (
                id VARCHAR(129),
                o_id VARCHAR(64),
                d_id VARCHAR(64),
                time TIMESTAMP
            )
        "#)
        .await?
        .collect()
        .await?;

        Ok(Self { ctx })
    }

    async fn unique_insert(&self, 
        content_vec: Vec<&str>,
        doc: &str,
        org: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {   
        // Build VALUES clause for nodes
        let node_values: String = content_vec
            .iter()
            .map(|c| {
                format!("(md5('{c}_{doc}_{org}'),'{c}','{doc}','{org}', now())")
            })
            .collect::<Vec<String>>()
            .join(",");
        
        let edge_values: String = content_vec
            .windows(2)
            .map(|w| format!("(md5('{0}_{doc}_{org}__{1}_{doc}_{org}'), md5('{0}_{doc}_{org}'), md5('{1}_{doc}_{org}'), now())", w[0], w[1]))
            .collect::<Vec<String>>()
            .join(",");

        // Single transaction-like batch
        let query = format!(r#"
            INSERT INTO kb.nodes 
            SELECT * FROM (VALUES {node_values}) AS new_nodes(id, content, doc, org, time)
            WHERE id NOT IN (SELECT id FROM kb.nodes)
        "#);
        
        self.ctx.sql(&query).await?.collect().await?;

        if !edge_values.is_empty() {
            let query = format!(r#"
                INSERT INTO kb.edges 
                SELECT * FROM (VALUES {edge_values}) AS new_edges(id, o_id, d_id, time)
                WHERE id NOT IN (SELECT id FROM kb.edges)
            "#);
            
            self.ctx.sql(&query).await?.collect().await?;
        }

        Ok(())
    }

    async fn recursive_trace_latest(&self, o_node_str: &str, doc: &str, org: &str) -> Result<(), Box<dyn std::error::Error>> {
        let result = self.ctx.sql(&format!(r#"
            WITH RECURSIVE nodes(node_1, depth) AS (
                SELECT md5('{o_node_str}_{doc}_{org}') as node_1, 0 as depth
                UNION ALL
                SELECT subq.d_id as node_1, nodes.depth + 1 as depth
                FROM nodes
                INNER JOIN (
                    SELECT o_id, d_id, ROW_NUMBER() OVER(PARTITION BY o_id ORDER BY time DESC) as row_num 
                    FROM kb.edges
                ) subq ON nodes.node_1 = subq.o_id
                WHERE subq.row_num = 1
            )
            SELECT * FROM nodes LEFT JOIN kb.nodes ON nodes.node_1 = kb.nodes.id ORDER BY depth
        "#)).await?.collect().await?;
        println!("{:?}", result);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let now = Instant::now();

    let kb = KnowledgeBase::new().await?;

    let content_vec: Vec<&str> = vec!["<ORIGIN_doc.md>","# This is a header", "This is text", "## This is another header"];
    let doc = "doc.md";
    let org = "myorg";
    kb.unique_insert(content_vec, doc, org).await?;

    let content_vec: Vec<&str> = vec!["<ORIGIN_doc.md>","# This is a newer header", "This is text", "## This is another header"];
    kb.unique_insert(content_vec, doc, org).await?;

    let content_vec: Vec<&str> = vec!["<ORIGIN_doc.md>","# This is a header", "This is text", "## This is another header", "This is new stuff", "### A bunch of new","stuff"];
    kb.unique_insert(content_vec, doc, org).await?;

    let query_res = kb.ctx.sql("SELECT * FROM kb.nodes").await?.collect().await?;
    println!("------Final Nodes-----\n{:?}", query_res);

    let query_res = kb.ctx.sql("SELECT * FROM kb.edges").await?.collect().await?;
    println!("------Final Edges-----\n{:?}", query_res);

    println!("----Full Trace----");
    kb.recursive_trace_latest("<ORIGIN_doc.md>", doc, org).await?;

    let elapsed_time = now.elapsed();
    println!("Running full process took {} milliseconds.", elapsed_time.as_millis());
    
    Ok(())
}