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

/// Represents a directed edge between two nodes in the knowledge graph.
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
    /// Ordered set of unique nodes
    node_table: IndexSet<Node>,
}

impl KnowledgeBase {
    /// Creates a new empty knowledge base.
    pub fn new() -> Self {
        Self {
            edge_table: BTreeMap::new(),
            node_table: IndexSet::new(),
        }
    }

    /// Inserts markdown content into the knowledge base.
    ///
    /// The markdown is converted to HTML and split by newlines. Each line becomes
    /// a node, and sequential nodes are connected by edges with the given version.
    /// Origin and end markers are automatically added to track document boundaries.
    ///
    /// # Arguments
    /// * `markdown_content` - Raw markdown text to process
    /// * `filename` - Source filename for provenance tracking
    /// * `version` - Version number for CRDT conflict resolution (higher = newer)
    /// * `tag` - Tag to apply to all edges created from this content
    pub fn insert_markdown(
        &mut self,
        markdown_content: &str,
        filename: &str,
        version: i32,
        tag: &str,
    ) {
        // Prepend origin tag and append end tag
        let tagged_content = format!(
            "<ORIGIN {}>\n{}\n<END {}>",
            filename, markdown_content, filename
        );

        let html = markdown::to_html(&tagged_content);
        let nodes: Vec<Node> = html
            .split('\n')
            .map(|line| Node::new(line.to_string(), filename.to_string()))
            .collect();

        if nodes.is_empty() {
            return;
        }

        // Insert first node
        self.node_table.insert(nodes[0].clone());

        // Insert remaining nodes and create edges
        for window in nodes.windows(2) {
            let from_node = &window[0];
            let to_node = &window[1];

            self.node_table.insert(to_node.clone());

            let from_idx = self.node_table.get_index_of(from_node).unwrap();
            let to_idx = self.node_table.get_index_of(to_node).unwrap();

            let edge_key = (from_idx, to_idx);

            // Only insert if edge doesn't exist - this preserves divergent paths
            self.edge_table
                .entry(edge_key)
                .or_insert_with(|| Edge::new(version, tag.to_string()));
        }
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

    /// Returns the number of nodes in the knowledge base.
    pub fn node_count(&self) -> usize {
        self.node_table.len()
    }

    /// Returns the number of edges in the knowledge base.
    pub fn edge_count(&self) -> usize {
        self.edge_table.len()
    }

    /// Finds the index of the origin node for a given filename.
    /// Returns None if no origin node is found for that filename.
    pub fn find_origin_node(&self, filename: &str) -> Option<usize> {
        let origin_marker = format!("&lt;ORIGIN {}&gt;", filename);
        self.node_table
            .iter()
            .position(|node| node.content == origin_marker && node.filename == filename)
    }

    /// Traverses from the origin node of a given filename.
    /// Returns None if the origin node cannot be found.
    pub fn traverse_from_origin(&self, filename: &str) -> Option<Vec<usize>> {
        self.find_origin_node(filename)
            .map(|idx| self.traverse_latest_path(idx))
    }

    /// Pretty prints the traversal path starting from the origin node of a filename.
    pub fn print_from_origin(&self, filename: &str) {
        match self.find_origin_node(filename) {
            Some(origin_idx) => {
                println!("Starting traversal from origin node of '{}':", filename);
                self.print_latest_path(origin_idx);
            }
            None => {
                println!("No origin node found for filename '{}'", filename);
            }
        }
    }
}

impl Default for KnowledgeBase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_insertion() {
        let mut kb = KnowledgeBase::new();
        let markdown = "# Header\nContent";

        kb.insert_markdown(markdown, "test.md", 0, "test");

        assert!(kb.node_count() > 0);
    }

    #[test]
    fn test_divergent_paths() {
        let mut kb = KnowledgeBase::new();

        // Insert first version
        kb.insert_markdown("# A\n# B\n# C", "test.md", 0, "path1");

        // Insert divergent version (A -> B -> D instead of A -> B -> C)
        kb.insert_markdown("# A\n# B\n# D", "test.md", 1, "path2");

        // Both edges from B should exist
        assert!(kb.edge_count() >= 3); // A->B, B->C, B->D
    }
}

fn main() {
    let mut kb = KnowledgeBase::new();

    // Insert multiple versions of similar content
    let md1 = "# Hi, *Saturn*! 🪐\nThis is some text\n## Another header\nMore text\n## New Header\n More texts";
    kb.insert_markdown(md1, "test.md", 0, "version-0");

    let md2 = "# Hi, *MARS*! 🪐\nThis is some text\n## Another header\nMore text\n## News Header\n More texts";
    kb.insert_markdown(md2, "test.md", 1, "version-1");

    let md3 = "# Hi, *Saturn*! 🪐\nThis is some new text\n## Another header\nMore text\n## New Header\n More texts";
    kb.insert_markdown(md3, "test.md", 2, "version-2");

    let md4 = "# Hi, *Saturn*! 🪐\nThis is some new text\n## Another header\nMore text\n### Inserted Header\nLots of New Text\n### Another one\nNewNewNew\n## New Header\n More texts";
    kb.insert_markdown(md4, "test.md", 2, "version-2");

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

    // Traverse and print the latest path from origin
    println!("\n=== Latest Path Traversal (from origin) ===");
    kb.print_from_origin("test.md");
}
