use datafusion::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = SessionContext::new();

    ctx.sql("CREATE SCHEMA kb").await.unwrap().collect().await.unwrap();

    ctx.sql(r#"
        CREATE TABLE kb.nodes (
            id VARCHAR,
            content VARCHAR,
            doc VARCHAR,
            org VARCHAR,
            time TIMESTAMP
        )
        "#).await.unwrap().collect().await.unwrap();

    ctx.sql(r#"
        CREATE TABLE kb.edges (
            id VARCHAR,
            o_id VARCHAR,
            d_id VARCHAR,
            time TIMESTAMP
        )
        "#).await.unwrap().collect().await.unwrap();

    let content_vec: Vec<&str> = vec!["# This is a header", "This is text", "## This is anotehr header"];
    let doc = "doc.md";
    let org = "myorg";
    let insert_elements: String = content_vec.iter().map(|c| format!("(md5('{c}_{doc}_{org}'),'{c}','{doc}','{org}', now())")).collect::<Vec<String>>().join(",");
    let insert_query: String = format!("INSERT INTO kb.nodes VALUES {insert_elements}");
    println!("{}",insert_query);
    ctx.sql(&insert_query).await.unwrap().collect().await.unwrap();

    let content_vec: Vec<&str> = vec!["# This is a newer header", "This is text", "## This is anotehr header"];
    let insert_elements: String = content_vec.iter().map(|c| format!("(md5('{c}_{doc}_{org}'),'{c}','{doc}','{org}', now())")).collect::<Vec<String>>().join(",");
    
    // Get the fresh nodes as RecordBatches
    let query = format!(r#"
    WITH new_nodes (id, content, doc, org, time) AS (
        VALUES {insert_elements}
    )
    SELECT new_nodes.* 
    FROM new_nodes 
    LEFT JOIN kb.nodes k ON new_nodes.id = k.id
    WHERE k.id IS NULL
    "#);
    
    let batches = ctx.sql(&query).await?.collect().await?;
    for batch in batches{
        let temp_table = "fresh_nodes_temp";
        ctx.register_batch(temp_table, batch)?;
        
        ctx.sql(&format!("INSERT INTO kb.nodes SELECT * FROM {}", temp_table))
            .await?
            .collect()
            .await?;
        
        ctx.deregister_table(temp_table)?;
    }
    
    let query_res = ctx.sql("SELECT * FROM kb.nodes").await?.collect().await?;
    println!("------Final Result-----\n{:?}", query_res);
    Ok(())
}