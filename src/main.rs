use indexmap::IndexSet;
use markdown;
use std::{collections::BTreeMap, hash::RandomState};

#[derive(Debug, Hash, Eq, Clone, PartialEq, Ord, PartialOrd)]
struct Node {
    content: String,
    filename: String,
}

#[derive(Debug)]
struct Edge {
    num: i32,
    tag: String,
}

fn insert_into_table<'a>(
    md_base: &str,
    mut edge_table: BTreeMap<(usize, usize), Edge>,
    mut node_table: IndexSet<Node>,
    filename: &str,
    id: i32,
) -> (BTreeMap<(usize, usize), Edge>, IndexSet<Node, RandomState>) {
    let md_result = markdown::to_html(md_base);
    let md_vec = md_result
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
            .or_insert(Edge {
                num: id,
                tag: "hello".to_string(),
            });
    }
    (edge_table, node_table)
}

fn main() {
    // Hashmap tests
    let mut edge_table: BTreeMap<(usize, usize), Edge> = BTreeMap::new();
    let mut node_table = IndexSet::new();

    let md_base: &str = "# Hi, *Saturn*! 🪐\nThis is some text\n## Another header\nMore text\n## New Header\n More texts";
    (edge_table, node_table) = insert_into_table(md_base, edge_table, node_table, "test.md", 0);
    let md_base: &str = "# Hi, *Saturn*! 🪐\nThis is some text\n## Another header\nMore text\n## News Header\n More texts";
    (edge_table, node_table) = insert_into_table(md_base, edge_table, node_table, "test.md", 1);
    let md_base: &str = "# Hi, *Saturn*! 🪐\nThis is some new text\n## Another header\nMore text\n## New Header\n More texts";
    (edge_table, node_table) = insert_into_table(md_base, edge_table, node_table, "test.md", 2);

    println!("{:?}", node_table);
    println!("{:?}", edge_table);

    println!("{:?}", node_table.get_index(0));

    let mut use_idx = 0;
    let mut proceed = true;
    while proceed {
        proceed = false;
        let mut max_value = -1000;
        for (k, v) in edge_table.range((use_idx, usize::MIN)..(use_idx + 1, usize::MIN)) {
            proceed = true;
            if v.num > max_value {
                max_value = v.num;
                use_idx = k.1;
            }
        }
        println!("{:?}", node_table.get_index(use_idx));
    }
}
