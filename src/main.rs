use indexmap::IndexSet;
use markdown;
use std::collections::BTreeMap;

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct StructEdge {
    from: String,
    to: String,
    filename: String,
}

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct Test {
    a: String,
    b: String,
}

fn insert_into_table(
    md_base: &str,
    mut edge_table: BTreeMap<StructEdge, i32>,
    filename: &str,
    id: i32,
) -> BTreeMap<StructEdge, i32> {
    let md_result = markdown::to_html(md_base);
    let mut md_vec = md_result
        .split("\n")
        .map(|s| s.to_string())
        .collect::<Vec<String>>();
    md_vec.insert(0, format!("<ORIGIN{:?}>", filename));
    md_vec.append(&mut vec![format!("<END{:?}>", filename)]);

    for i in 0..md_vec.len() - 1 {
        edge_table
            .entry(StructEdge {
                from: md_vec[i].to_string(),
                to: md_vec[i + 1].to_string(),
                filename: filename.to_string(),
            })
            .or_insert(id);
    }

    edge_table
}

fn main() {
    let md_base: &str = "# Hi, *Saturn*! 🪐\nThis is some text\n## Another header\nMore text\n## New Header\n More text";
    // Hashmap tests
    let mut edge_table: BTreeMap<StructEdge, i32> = BTreeMap::new();
    edge_table = insert_into_table(md_base, edge_table, "test_file.md", 0);
    let md_new: &str = "# Hi, *Saturn*! 🪐\nThis is some text\n## Another header\nMore text\n## New Header\n More text";
    edge_table = insert_into_table(md_new, edge_table, "test_file.md", 1);
    println!("{:?}", edge_table);

    // Trace Test
    let mut set = IndexSet::new();
    set.insert(Test {
        a: "Apple".to_string(),
        b: "hi".to_string(),
    });
    set.insert(Test {
        a: "Banana".to_string(),
        b: "hi".to_string(),
    });

    println!(
        "{:?}",
        set.get_index_of(&Test {
            a: "Apple".to_string(),
            b: "hi".to_string()
        })
    )
}
