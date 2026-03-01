use datafusion::prelude::*;
use datafusion::arrow::array::{Array, StringViewArray};
use datafusion::common::HashSet;
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
                id VARCHAR,
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
                id VARCHAR,
                o_id VARCHAR,
                d_id VARCHAR,
                time TIMESTAMP
            )
        "#)
        .await?
        .collect()
        .await?;

        Ok(Self { ctx })
    }

    async fn recursive_trace_latest(&self, o_node: &str) -> Result<(), Box<dyn std::error::Error>> {
        let result = self.ctx.sql(&format!(r#"
            WITH RECURSIVE nodes(node_1) AS (
                SELECT '{o_node}' as node_1
                UNION ALL
                SELECT subq.d_id as node_1 
                FROM nodes
                INNER JOIN (
                    SELECT o_id, d_id, ROW_NUMBER() OVER(PARTITION BY o_id ORDER BY time DESC) as row_num 
                    FROM kb.edges
                ) subq ON nodes.node_1 = subq.o_id
                WHERE subq.row_num = 1
            )
            SELECT node_1 FROM nodes
        "#)).await?.collect().await?;
        println!("{:?}", result);
        Ok(())
    }

    async fn unique_edge_insert(&self, content_vec: Vec<&str>)-> Result<(),Box<dyn std::error::Error>>{
        let insert_edges = content_vec
            .windows(2)
            .map(|c| format!("('{0}_{1}', '{0}' , '{1}', now())", c[0], c[1]))
            .collect::<Vec<String>>()
            .join(",");

        let query = format!(
            r#"
            WITH new_edges (id, o_id, d_id, time) AS (
                VALUES {insert_edges}
            )
            SELECT new_edges.* 
            FROM new_edges
            LEFT JOIN kb.edges k ON new_edges.id = k.id
            WHERE k.id IS NULL
            "#
        );

        let batches = self.ctx.sql(&query).await?.collect().await?;

        if !batches.is_empty() {
            for batch in batches {
            let temp_table = "fresh_nodes_temp";
            self.ctx.register_batch(temp_table, batch)?;

            self.ctx
                .sql(&format!("INSERT INTO kb.edges SELECT * FROM {}", temp_table))
                .await?
                .collect()
                .await?;

            self.ctx.deregister_table(temp_table)?;
            }
        }

        Ok(())
    }

    async fn unique_node_insert(
        &self,
        content_vec: Vec<&str>,
        doc: &str,
        org: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let insert_elements: String = content_vec
            .iter()
            .map(|c| format!("('{c}_{doc}_{org}','{c}','{doc}','{org}', now())"))
            .collect::<Vec<String>>()
            .join(",");

        // Get the fresh nodes as RecordBatches
        let query = format!(
            r#"
            WITH new_nodes (id, content, doc, org, time) AS (
                VALUES {insert_elements}
            )
            SELECT new_nodes.* 
            FROM new_nodes 
            LEFT JOIN kb.nodes k ON new_nodes.id = k.id
            WHERE k.id IS NULL
            "#
        );

        let batches = self.ctx.sql(&query).await?.collect().await?;

        if !batches.is_empty() {
            for batch in batches {
            let temp_table = "fresh_nodes_temp";
            self.ctx.register_batch(temp_table, batch)?;

            self.ctx
                .sql(&format!("INSERT INTO kb.nodes SELECT * FROM {}", temp_table))
                .await?
                .collect()
                .await?;

            self.ctx.deregister_table(temp_table)?;
            }
        }

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
    kb.unique_node_insert(content_vec.clone(), doc, org).await?;
    kb.unique_edge_insert(content_vec).await?;

    let content_vec: Vec<&str> = vec!["<ORIGIN_doc.md>","# This is a newer header", "This is text", "## This is another header"];
    kb.unique_node_insert(content_vec.clone(), doc, org).await?;
    kb.unique_edge_insert(content_vec).await?;

    let content_vec: Vec<&str> = vec!["<ORIGIN_doc.md>","# This is a header", "This is text", "## This is another header", "This is new stuff", "### A bunch of new","stuff"];
    kb.unique_node_insert(content_vec.clone(), doc, org).await?;
    kb.unique_edge_insert(content_vec).await?;

    let query_res = kb.ctx.sql("SELECT * FROM kb.nodes").await?.collect().await?;
    println!("------Final Nodes-----\n{:?}", query_res);

    let query_res = kb.ctx.sql("SELECT * FROM kb.edges").await?.collect().await?;
    println!("------Final Edges-----\n{:?}", query_res);

    kb.recursive_trace_latest("<ORIGIN_doc.md>").await?;

    let elapsed_time = now.elapsed();
    println!("Running full process took {} milliseconds.", elapsed_time.as_millis());
    
    Ok(())
}