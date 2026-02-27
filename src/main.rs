use indexmap::IndexSet;
use std::collections::BTreeMap;

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
    /// The markdown is converted to HTML and split by newlines. Each line becomes
    /// a node, and sequential nodes are connected by edges with the given version.
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

        let html = markdown::to_html(markdown_content);
        let content_nodes: Vec<Node> = html
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
}

impl Default for KnowledgeBase {
    fn default() -> Self {
        Self::new()
    }
}

fn main() {
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
