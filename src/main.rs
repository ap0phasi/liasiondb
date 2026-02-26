use indexmap::IndexSet;
use markdown;
use std::{collections::BTreeMap, hash::RandomState};

#[derive(Debug, Hash, Eq, Clone, PartialEq, Ord, PartialOrd)]
struct Node {
    content: String,
    filename: String,
}

fn insert_into_table<'a>(
    md_base: &str,
    mut edge_table: BTreeMap<(usize, usize), &'a str>,
    mut node_table: IndexSet<Node>,
    filename: &str,
    id: i32,
) -> (
    BTreeMap<(usize, usize), &'a str>,
    IndexSet<Node, RandomState>,
) {
    let md_result = markdown::to_html(md_base);
    let mut md_vec = md_result
        .split("\n")
        .map(|s| Node {
            content: s.to_string(),
            filename: filename.to_string(),
        })
        .collect::<Vec<Node>>();

    node_table.insert(md_vec[0].clone());
    for i in 1..md_vec.len() {
        node_table.insert(md_vec[i].clone());

        edge_table
            .entry((
                node_table.get_index_of(&md_vec[i - 1]).unwrap(),
                node_table.get_index_of(&md_vec[i]).unwrap(),
            ))
            .or_insert("hi");
    }
    (edge_table, node_table)
}

fn main() {
    let md_base: &str = "# Hi, *Saturn*! 🪐\nThis is some text\n## Another header\nMore text\n## New Header\n More texts";
    // Hashmap tests
    let mut edge_table: BTreeMap<(usize, usize), &str> = BTreeMap::new();
    let mut node_table = IndexSet::new();
    (edge_table, node_table) = insert_into_table(md_base, edge_table, node_table, "test.md", 0);

    println!("{:?}", node_table);
    println!("{:?}", edge_table);

    // Query BTreeMap
    for (k, v) in edge_table.range((1, usize::MIN)..(3, usize::MIN)) {
        println!("{:?}", (k, v))
    }

    println!("{:?}", node_table.get_index(4))
}
