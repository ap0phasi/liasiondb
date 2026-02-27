use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
    Json, Router,
};
use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use tokio::fs;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Represents a content node in the knowledge graph.
/// Nodes are uniquely identified by their content and source filename.
#[derive(Debug, Hash, Eq, Clone, PartialEq, Ord, PartialOrd)]
pub struct Node {
    content: String,
    filename: String,
}

impl Node {
    pub fn new(content: String, filename: String) -> Self {
        Self { content, filename }
    }
}

/// Represents a Structural directed edge between two nodes in the knowledge graph.
/// Edges track the version/timestamp when they were created and can be tagged.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Version number or timestamp for CRDT conflict resolution
    pub version: i32,
    /// Optional tag for categorizing edges
    pub tag: String,
}

impl Edge {
    pub fn new(version: i32, tag: String) -> Self {
        Self { version, tag }
    }
}

/// Ledger file that tracks which nodes have been read.
/// This is a single .ledger file that accumulates node IDs as files are read.
/// When writing, these nodes are used as references.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Ledger {
    /// Node indices that have been read
    pub node_indices: Vec<usize>,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            node_indices: Vec::new(),
        }
    }

    pub fn add_nodes(&mut self, nodes: Vec<usize>) {
        self.node_indices.extend(nodes);
        // Remove duplicates while preserving order
        let mut seen = std::collections::HashSet::new();
        self.node_indices.retain(|&x| seen.insert(x));
    }
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    kb: Arc<RwLock<KnowledgeBase>>,
    /// Directory where files are saved/loaded
    file_dir: String,
}

/// A graph-based CRDT for tracking provenance and relationships in a knowledge base.
///
/// This structure maintains a directed graph where:
/// - Nodes represent content chunks (e.g., paragraphs from markdown files)
/// - Edges represent sequential relationships with version tracking
///
/// The CRDT uses Last-Write-Wins (LWW) semantics based on version numbers.
#[derive(Debug)]
pub struct KnowledgeBase {
    /// Maps node index pairs (from, to) to edges
    edge_table: BTreeMap<(usize, usize), Edge>,
    /// Maps from reference nodes to nodes
    ref_table: BTreeMap<(usize, usize), Edge>,
    /// Ordered set of unique nodes
    node_table: IndexSet<Node>,
}

impl KnowledgeBase {
    /// Creates a new empty knowledge base.
    pub fn new() -> Self {
        Self {
            edge_table: BTreeMap::new(),
            ref_table: BTreeMap::new(),
            node_table: IndexSet::new(),
        }
    }

    /// Inserts a directory node into the knowledge base.
    ///
    /// # Arguments
    /// * `directory_path` - Path of the directory (e.g., "src/components")
    ///
    /// # Returns
    /// The index of the created directory node
    pub fn insert_directory(&mut self, directory_path: &str) -> usize {
        let dir_node = Node::new(format!("DIR: {}", directory_path), "".to_string());
        self.node_table.insert(dir_node.clone());
        self.node_table.get_index_of(&dir_node).unwrap()
    }

    /// Inserts a generic node into the knowledge base.
    ///
    /// # Arguments
    /// * `content` - Content of the node
    /// * `filename` - Source filename for provenance tracking
    ///
    /// # Returns
    /// The index of the created node
    pub fn insert_node(&mut self, content: &str, filename: &str) -> usize {
        let node = Node::new(content.to_string(), filename.to_string());
        self.node_table.insert(node.clone());
        self.node_table.get_index_of(&node).unwrap()
    }

    /// Inserts markdown content into the knowledge base.
    ///
    /// The markdown is split by newlines. Each line becomes a node, and sequential 
    /// nodes are connected by edges with the given version.
    /// A file node is created and linked to the parent node, then all content nodes
    /// are linked sequentially starting from the file node.
    ///
    /// # Arguments
    /// * `markdown_content` - Raw markdown text to process
    /// * `filename` - Source filename for provenance tracking
    /// * `parent_idx` - Index of the parent node (e.g., directory node)
    /// * `version` - Version number for CRDT conflict resolution (higher = newer)
    /// * `tag` - Tag to apply to all edges created from this content
    ///
    /// # Returns
    /// The index of the file node created
    pub fn insert_markdown(
        &mut self,
        markdown_content: &str,
        filename: &str,
        parent_idx: usize,
        reference_nodes: Vec<Node>,
        version: i32,
        tag: &str,
    ) -> usize {
        // Create file node and link it to parent
        let file_node = Node::new(format!("FILE: {}", filename), filename.to_string());
        self.node_table.insert(file_node.clone());
        let file_idx = self.node_table.get_index_of(&file_node).unwrap();
        
        // Create structural edge from parent to file
        self.edge_table
            .entry((parent_idx, file_idx))
            .or_insert_with(|| Edge::new(version, tag.to_string()));

        // Split markdown by lines and create nodes
        let content_nodes: Vec<Node> = markdown_content
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(|line| Node::new(line.to_string(), filename.to_string()))
            .collect();

        if content_nodes.is_empty() {
            return file_idx;
        }

        let mut new_node_indices = Vec::new();

        // Insert first content node and link it from file node
        self.node_table.insert(content_nodes[0].clone());
        let first_content_idx = self.node_table.get_index_of(&content_nodes[0]).unwrap();
        new_node_indices.push(first_content_idx);

        // Link file node to first content node
        self.edge_table
            .entry((file_idx, first_content_idx))
            .or_insert_with(|| Edge::new(version, tag.to_string()));

        // Insert remaining nodes and create edges
        for window in content_nodes.windows(2) {
            let from_node = &window[0];
            let to_node = &window[1];

            let is_new = self.node_table.insert(to_node.clone());

            let from_idx = self.node_table.get_index_of(from_node).unwrap();
            let to_idx = self.node_table.get_index_of(to_node).unwrap();
            if is_new {
                new_node_indices.push(to_idx)
            };

            let edge_key = (from_idx, to_idx);

            // Only insert if edge doesn't exist - this preserves divergent paths
            self.edge_table
                .entry(edge_key)
                .or_insert_with(|| Edge::new(version, tag.to_string()));
        }

        // Insert references
        for reference_node in reference_nodes {
            self.node_table.insert(reference_node.clone());

            let from_idx = self.node_table.get_index_of(&reference_node).unwrap();
            for to_idx in new_node_indices.clone().into_iter() {
                let edge_key = (from_idx, to_idx);

                // Only insert if edge doesn't exist - this preserves divergent paths
                self.ref_table
                    .entry(edge_key)
                    .or_insert_with(|| Edge::new(version, tag.to_string()));
            }
        }

        file_idx
    }

    /// Returns an immutable reference to the node table.
    pub fn nodes(&self) -> &IndexSet<Node> {
        &self.node_table
    }

    /// Returns an immutable reference to the edge table.
    pub fn edges(&self) -> &BTreeMap<(usize, usize), Edge> {
        &self.edge_table
    }

    /// Traverses the graph starting from a given node index, following the
    /// edges with the highest version numbers (most recent path).
    ///
    /// Returns a vector of node indices representing the traversal path.
    pub fn traverse_latest_path(&self, start_idx: usize) -> Vec<usize> {
        let mut path = Vec::new();
        let mut current_idx = start_idx;

        loop {
            path.push(current_idx);

            // Find all outgoing edges from current node
            let next_edge = self
                .edge_table
                .range((current_idx, usize::MIN)..(current_idx + 1, usize::MIN))
                .max_by_key(|(_, edge)| edge.version);

            match next_edge {
                Some(((_, to_idx), _)) => {
                    current_idx = *to_idx;
                }
                None => break,
            }
        }

        path
    }

    /// Pretty prints the traversal path starting from a given node.
    pub fn print_latest_path(&self, start_idx: usize) {
        let path = self.traverse_latest_path(start_idx);

        for idx in path {
            println!("{:?}", self.node_table.get_index(idx));
        }
    }

    /// Performs breadth-first search to find all nodes "contaminated" by a given node.
    /// This follows the reference edges forward (from the given node to all nodes it influenced).
    ///
    /// # Arguments
    /// * `start_idx` - The index of the node to start the search from
    ///
    /// # Returns
    /// A vector of node indices that are contaminated (influenced) by the starting node
    pub fn find_contaminated_nodes(&self, start_idx: usize) -> Vec<usize> {
        use std::collections::{HashSet, VecDeque};

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut contaminated = Vec::new();

        queue.push_back(start_idx);
        visited.insert(start_idx);

        while let Some(current_idx) = queue.pop_front() {
            contaminated.push(current_idx);

            // Find all outgoing reference edges from current node
            for ((from_idx, to_idx), _) in self
                .ref_table
                .range((current_idx, usize::MIN)..(current_idx + 1, usize::MIN))
            {
                if *from_idx == current_idx && !visited.contains(to_idx) {
                    visited.insert(*to_idx);
                    queue.push_back(*to_idx);
                }
            }
        }

        contaminated
    }

    /// Performs breadth-first search to find all nodes referenced by a given node.
    /// This follows the reference edges backward (from the given node to all nodes that influenced it).
    ///
    /// # Arguments
    /// * `start_idx` - The index of the node to start the search from
    ///
    /// # Returns
    /// A vector of node indices that are referenced (influenced) the starting node
    pub fn find_referenced_nodes(&self, start_idx: usize) -> Vec<usize> {
        use std::collections::{HashSet, VecDeque};

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut referenced = Vec::new();

        queue.push_back(start_idx);
        visited.insert(start_idx);

        while let Some(current_idx) = queue.pop_front() {
            referenced.push(current_idx);

            // Find all incoming reference edges to current node
            for ((from_idx, to_idx), _) in self.ref_table.iter() {
                if *to_idx == current_idx && !visited.contains(from_idx) {
                    visited.insert(*from_idx);
                    queue.push_back(*from_idx);
                }
            }
        }

        referenced
    }

    /// Returns the number of nodes in the knowledge base.
    pub fn node_count(&self) -> usize {
        self.node_table.len()
    }

    /// Returns the number of edges in the knowledge base.
    pub fn edge_count(&self) -> usize {
        self.edge_table.len()
    }

    /// Lists all unique filenames in the knowledge base.
    pub fn list_files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .node_table
            .iter()
            .filter(|node| node.content.starts_with("FILE: "))
            .map(|node| node.content.strip_prefix("FILE: ").unwrap().to_string())
            .collect();
        files.sort();
        files.dedup();
        files
    }

    /// Reconstructs a markdown file from the knowledge base by traversing from a file node.
    /// Returns both the markdown content and the node indices that composed it.
    pub fn read_file(&self, filename: &str) -> Option<(String, Vec<usize>)> {
        // Find the file node
        let file_node = Node::new(format!("FILE: {}", filename), filename.to_string());
        let file_idx = self.node_table.get_index_of(&file_node)?;

        // Traverse from the file node to get all content
        let path = self.traverse_latest_path(file_idx);
        
        // Skip the first node (FILE node itself) and collect content
        let mut node_indices = Vec::new();
        let mut markdown_parts = Vec::new();
        
        for idx in path.iter().skip(1) {
            if let Some(node) = self.node_table.get_index(*idx) {
                let content = &node.content;
                markdown_parts.push(content.clone());
                node_indices.push(*idx);
            }
        }

        // Join markdown lines back together
        let markdown = markdown_parts.join("\n");
        Some((markdown, node_indices))
    }
}

impl Default for KnowledgeBase {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HTTP Handlers
// ============================================================================

/// Health check endpoint
async fn health() -> &'static str {
    "OK"
}

/// Clear the ledger file
async fn clear_ledger(State(state): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let ledger_path = format!("{}/.ledger", state.file_dir);
    
    // Write empty ledger
    let ledger = Ledger::new();
    let ledger_json = serde_json::to_string_pretty(&ledger).unwrap();
    fs::write(&ledger_path, ledger_json).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(serde_json::json!({
        "status": "ledger cleared"
    })))
}

/// Lists all files in the knowledge base
async fn list_files(State(state): State<AppState>) -> Json<Vec<String>> {
    let kb = state.kb.read().unwrap();
    Json(kb.list_files())
}

/// Reads a file from the knowledge base and saves it with a .ledger file
async fn read_file(
    State(state): State<AppState>,
    Path(filepath): Path<String>,
) -> Result<String, StatusCode> {
    let content: String;
    let node_indices: Vec<usize>;
    
    {
        let kb = state.kb.read().unwrap();
        match kb.read_file(&filepath) {
            Some(result) => {
                content = result.0;
                node_indices = result.1;
            },
            None => return Err(StatusCode::NOT_FOUND),
        }
    }

    // Save file to disk
    let file_path = format!("{}/{}", state.file_dir, filepath);
    
    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        fs::create_dir_all(parent).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    
    fs::write(&file_path, &content).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Update the shared .ledger file
    let ledger_path = format!("{}/.ledger", state.file_dir);
    let mut ledger = if let Ok(ledger_content) = fs::read_to_string(&ledger_path).await {
        serde_json::from_str::<Ledger>(&ledger_content).unwrap_or_else(|_| Ledger::new())
    } else {
        Ledger::new()
    };
    
    ledger.add_nodes(node_indices);
    
    let ledger_json = serde_json::to_string_pretty(&ledger).unwrap();
    fs::write(&ledger_path, ledger_json).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(content)
}

/// Request body for writing a file
#[derive(Deserialize)]
struct WriteFileRequest {
    content: String,
}

/// Writes a file to the knowledge base, using .ledger file for reference nodes
async fn write_file(
    State(state): State<AppState>,
    Path(filepath): Path<String>,
    Json(payload): Json<WriteFileRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Read the shared .ledger file to get reference nodes
    let ledger_path = format!("{}/.ledger", state.file_dir);
    let reference_nodes = if let Ok(ledger_content) = fs::read_to_string(&ledger_path).await {
        match serde_json::from_str::<Ledger>(&ledger_content) {
            Ok(ledger) => {
                // Convert node indices to actual nodes
                let kb = state.kb.read().unwrap();
                ledger
                    .node_indices
                    .iter()
                    .filter_map(|idx| kb.nodes().get_index(*idx).cloned())
                    .collect()
            }
            Err(_) => {
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    } else {
        Vec::new()
    };

    // Get or create directory parent node
    let file_idx: usize;
    {
        let mut kb = state.kb.write().unwrap();
        
        // Extract directory path from filepath
        let dir_path = std::path::Path::new(&filepath)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        
        let parent_idx = if dir_path.is_empty() {
            kb.insert_directory(".")
        } else {
            kb.insert_directory(dir_path)
        };

        // Get current highest version
        let version = kb.edge_count() as i32;
        
        // Insert the markdown
        file_idx = kb.insert_markdown(
            &payload.content,
            &filepath,
            parent_idx,
            reference_nodes,
            version,
            &format!("version-{}", version),
        );
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "file_idx": file_idx,
    })))
}

// ============================================================================
// Main Application
// ============================================================================

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "liasiondb=debug,tower_http=debug,axum=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Create knowledge base and populate with example data
    let mut kb = KnowledgeBase::new();
    
    // Create a directory node
    let docs_dir_idx = kb.insert_directory("docs");
    
    // Insert example content
    let md1 = "# Example Document\n\nThis is some example content.";
    kb.insert_markdown(
        md1,
        "example.md",
        docs_dir_idx,
        vec![],
        0,
        "version-0",
    );

    // Set up shared state
    let file_dir = std::env::var("FILE_DIR").unwrap_or_else(|_| "./files".to_string());
    fs::create_dir_all(&file_dir)
        .await
        .expect("Failed to create file directory");

    let state = AppState {
        kb: Arc::new(RwLock::new(kb)),
        file_dir,
    };

    // Build router
    use axum::routing::MethodRouter;
    let app = Router::new()
        .route("/health", get(health))
        .route("/ledger", delete(clear_ledger))
        .route("/files", get(list_files))
        .route("/files/*path", MethodRouter::new().get(read_file).post(write_file))
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    
    tracing::info!("Server listening on {}", listener.local_addr().unwrap());
    
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
fn old_main() {
    let mut kb = KnowledgeBase::new();

    // Create a directory node
    let docs_dir_idx = kb.insert_directory("docs");

    // Insert multiple versions of similar content under the directory
    let md1 = "# Hi, *Saturn*! 🪐\nThis is some text\n## Another header\nMore text\n## New Header\n More texts";
    kb.insert_markdown(
        md1,
        "test.md",
        docs_dir_idx,
        vec![Node::new(
            "it came to me in a dream".to_string(),
            "".to_string(),
        )],
        0,
        "version-0",
    );

    let md2 = "# Hi, *Saturn*! 🪐\nThis is some better text\n## Another header\nMore text\n## New Header\n More texts";
    kb.insert_markdown(
        md2,
        "test.md",
        docs_dir_idx,
        vec![Node::new(
            "I actually read this I swear".to_string(),
            "".to_string(),
        )],
        1,
        "version-1",
    );

    // Display the knowledge base state
    println!("=== Knowledge Base State ===");
    println!("\nNodes ({} total):", kb.node_count());
    for (idx, node) in kb.nodes().iter().enumerate() {
        println!("  [{}] {:?}", idx, node);
    }

    println!("\nEdges ({} total):", kb.edge_count());
    for ((from, to), edge) in kb.edges() {
        println!(
            "  {} -> {} (v{}, tag: {})",
            from, to, edge.version, edge.tag
        );
    }

    // Traverse and print the latest path from directory
    println!("\n=== Latest Path Traversal (from directory) ===");
    kb.print_latest_path(docs_dir_idx);

    println!("\n=== Reference Table ===");
    println!("{:?}", kb.ref_table);

    // Demonstrate BFS: Find all nodes contaminated by "it came to me in a dream"
    println!("\n=== Nodes Contaminated by 'it came to me in a dream' ===");
    let dream_idx = kb
        .nodes()
        .iter()
        .position(|n| n.content == "it came to me in a dream")
        .unwrap();
    let contaminated = kb.find_contaminated_nodes(dream_idx);
    println!("Reference node index: {}", dream_idx);
    println!("Contaminated nodes ({} total):", contaminated.len());
    for idx in &contaminated {
        println!("  [{}] {:?}", idx, kb.nodes().get_index(*idx));
    }

    // Demonstrate BFS: Find all nodes referenced by a content node
    println!("\n=== Nodes Referenced by '<p>This is some better text</p>' ===");
    let content_idx = kb
        .nodes()
        .iter()
        .position(|n| n.content == "<p>This is some better text</p>")
        .unwrap();
    let referenced = kb.find_referenced_nodes(content_idx);
    println!("Content node index: {}", content_idx);
    println!("Referenced nodes ({} total):", referenced.len());
    for idx in &referenced {
        println!("  [{}] {:?}", idx, kb.nodes().get_index(*idx));
    }
}
