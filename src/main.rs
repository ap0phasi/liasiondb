use markdown;
use std::collections::HashMap;

#[derive(Debug, Hash, Eq, PartialEq)]
struct Link {
    from: String,
    to: String,
    filename: String,
}

fn insert_into_table(
    md_base: &str,
    mut edge_table: HashMap<Link, i32>,
    id: i32,
) -> HashMap<Link, i32> {
    let md_result = markdown::to_html(md_base);
    let mut md_vec = md_result.split("\n").collect::<Vec<&str>>();
    md_vec.insert(0, "<ORIGIN>");
    md_vec.append(&mut vec!["<END>"]);

    for i in 0..md_vec.len() - 1 {
        edge_table
            .entry(Link {
                from: md_vec[i].to_string(),
                to: md_vec[i + 1].to_string(),
                filename: "hello.md".to_string(),
            })
            .or_insert(id);
    }

    edge_table
}

fn main() {
    let md_base: &str = "# Hi, *Saturn*! 🪐\nThis is some text\n## Another header\nMore text\n## New Header\n More text";
    // Hashmap tests
    let mut edge_table: HashMap<Link, i32> = HashMap::new();
    edge_table = insert_into_table(md_base, edge_table, 0);
    println!("{:?}", edge_table);
    let md_new: &str = "# Hi, *Saturn*! 🪐\nThis is some text\nThis is some more text that is new\n## Another header\nMore text\n## New Header\n More text ## A Newer Header\nBlah Blah";
    edge_table = insert_into_table(md_new, edge_table, 1);
    println!("{:?}", edge_table);
}
