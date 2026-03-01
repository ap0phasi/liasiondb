use datafusion::prelude::*;
use std::time::Instant;
use std::hash::BuildHasher;
use rapidhash::fast::SeedableState;
use std::collections::BTreeSet;
use tokio::sync::RwLock;

struct KnowledgeBase {
    ctx: SessionContext,
    node_index: RwLock<BTreeSet<u64>>,
    edge_index: RwLock<BTreeSet<(u64, u64)>>,
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

        Ok(Self { 
            ctx,
            node_index: RwLock::new(BTreeSet::new()),
            edge_index: RwLock::new(BTreeSet::new()),
        })
    }

    async fn unique_insert(&self, 
        content_vec: Vec<&str>,
        doc: &str,
        org: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Compute all hashes upfront
        let hasher = SeedableState::fixed();
        let hash_vec: Vec<u64> = content_vec
            .iter()
            .map(|i| hasher.hash_one(format!("{i}_{doc}_{org}")))
            .collect();
        
        // Filter new nodes
        let node_idx = self.node_index.read().await;
        let new_nodes: Vec<(usize, &str, u64)> = content_vec
            .iter()
            .zip(hash_vec.iter())
            .enumerate()
            .filter(|(_, (_, hash))| !node_idx.contains(hash))
            .map(|(i, (content, hash))| (i, *content, *hash))
            .collect();
        drop(node_idx);

        // Filter new edges
        let edge_idx = self.edge_index.read().await;
        let new_edges: Vec<(u64, u64)> = hash_vec
            .windows(2)
            .filter(|w| !edge_idx.contains(&(w[0], w[1])))
            .map(|w| (w[0], w[1]))
            .collect();
        drop(edge_idx);

        // Batch insert new nodes
        if !new_nodes.is_empty() {
            self.batch_insert_nodes(&new_nodes, doc, org).await?;
            
            // Update node index
            let mut node_idx = self.node_index.write().await;
            for (_, _, hash) in &new_nodes {
                node_idx.insert(*hash);
            }
        }

        // Batch insert new edges
        if !new_edges.is_empty() {
            self.batch_insert_edges(&new_edges).await?;
            
            // Update edge index
            let mut edge_idx = self.edge_index.write().await;
            for (o, d) in &new_edges {
                edge_idx.insert((*o, *d));
            }
        }

        Ok(())
    }

    async fn batch_insert_nodes(
        &self,
        nodes: &[(usize, &str, u64)],
        doc: &str,
        org: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let insert_elements: String = nodes
            .iter()
            .map(|(_, content, hash)| format!("('{hash}','{content}','{doc}','{org}', now())"))
            .collect::<Vec<String>>()
            .join(",");

        let query = format!(
            r#"INSERT INTO kb.nodes VALUES {insert_elements}"#
        );

        self.ctx.sql(&query).await?.collect().await?;
        Ok(())
    }

    async fn batch_insert_edges(
        &self,
        edges: &[(u64, u64)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let insert_elements: String = edges
            .iter()
            .map(|(o, d)| format!("('{o}_{d}', '{o}', '{d}', now())"))
            .collect::<Vec<String>>()
            .join(",");

        let query = format!(
            r#"INSERT INTO kb.edges VALUES {insert_elements}"#
        );

        self.ctx.sql(&query).await?.collect().await?;
        Ok(())
    }

    async fn recursive_trace_latest(&self, o_node_str: &str, doc: &str, org: &str) -> Result<(), Box<dyn std::error::Error>> {
        let hasher = SeedableState::fixed();
        let o_node = hasher.hash_one(format!("{o_node_str}_{doc}_{org}"));
        let result = self.ctx.sql(&format!(r#"
            WITH RECURSIVE nodes(node_1, depth) AS (
                SELECT '{o_node}' as node_1, 0 as depth
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