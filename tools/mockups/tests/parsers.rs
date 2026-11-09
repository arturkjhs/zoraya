//! ADR-0161: "a host test parses every file in `docs/research/desktop/mockups/`
//! with both parsers and compares the token sets": the tool's parser
//! (`zoraya_mockups::tokens`) and the system's (`zoraya_tokens::parse`).

use std::collections::BTreeMap;

use zoraya_tokens::parse::{Set, lines};

#[test]
fn both_parsers_read_every_direction_file_alike() {
    let root = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/research/desktop/mockups"
    );
    let mut files = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path().join("tokens.txt");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        files += 1;
        let tool = zoraya_mockups::tokens::Tokens::parse(&text).unwrap();
        let mut plain = BTreeMap::new();
        let mut light = BTreeMap::new();
        let mut dark = BTreeMap::new();
        for line in lines(&text) {
            let (set, name, value) = line.unwrap_or_else(|n| panic!("{path:?} line {n}"));
            let map = match set {
                Set::Plain => &mut plain,
                Set::Light => &mut light,
                Set::Dark => &mut dark,
            };
            map.insert(name.to_string(), value.to_string());
        }
        assert_eq!(plain, tool.plain, "{path:?}");
        assert_eq!(light, tool.light, "{path:?}");
        assert_eq!(dark, tool.dark, "{path:?}");
    }
    assert!(files >= 1);
}
