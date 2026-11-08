//! The line editor (ADR-0154, INV-0154-1) on arbitrary keys at a width
//! taken from the first byte: the line stays valid and the cursor inside it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zoraya_shell::editor::{Assist, Completion, Editor, Options};

struct Nothing;

impl Assist for Nothing {
    fn known(&mut self, name: &str) -> bool {
        name.len() % 2 == 0
    }
    fn incomplete(&mut self, text: &str) -> bool {
        text.ends_with('{')
    }
    fn complete(&mut self, line: &str, cursor: usize) -> Completion {
        Completion {
            start: line[..cursor].rfind(' ').map_or(0, |n| n + 1),
            candidates: vec![("alpha".into(), None), ("alps".into(), Some("x".into()))],
            slow: false,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&w, keys)) = data.split_first() else {
        return;
    };
    let mut e = Editor::new(Options {
        width: 1 + usize::from(w) * 2,
        ..Options::default()
    });
    e.remember("make -C src");
    let mut out = String::new();
    e.start("~> ", &mut out);
    for &k in keys {
        out.clear();
        let _ = e.key(k, &mut out, &mut Nothing);
        assert!(e.cursor() <= e.text().chars().count());
    }
});
