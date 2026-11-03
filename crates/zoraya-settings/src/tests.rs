use super::*;

const ZOSH: &str = "name zosh
summary the shell
usage zosh
setting shell.prompt text '{cwd}> ' the prompt
setting shell.colour yes-no yes colour
setting shell.history-size number 0 100000 1000 lines kept
setting shell.suggestions yes-no yes suggestions
setting shell.mode choice fast|slow fast how
setting console.columns number 20 500 80 width
";

fn declared() -> Vec<Setting> {
    zoraya_describe::parse(ZOSH.as_bytes()).unwrap().settings
}

#[test]
fn defaults_when_empty() {
    let v = read(b"", &declared());
    assert_eq!(v.get("shell.prompt"), Some("{cwd}> "));
    assert_eq!(v.get("console.columns"), Some("80"));
    assert!(v.problems.is_empty());
}

#[test]
fn values_comments_and_problems() {
    let text = "# my settings\n\nshell.prompt = 'zoraya {cwd}$ '\nshell.colour = maybe\nshell.colur = no\n\
                gui.theme = dark\njunk\nshell.history-size = 5\nshell.history-size = 6\nshell.mode=slow\n";
    let v = read(text.as_bytes(), &declared());
    assert_eq!(v.get("shell.prompt"), Some("zoraya {cwd}$ "));
    assert_eq!(
        v.get("shell.colour"),
        Some("yes"),
        "a bad value keeps the default"
    );
    assert_eq!(v.get("shell.history-size"), Some("5"));
    assert_eq!(v.get("shell.mode"), Some("slow"));
    let p: Vec<(usize, &str)> = v
        .problems
        .iter()
        .map(|p| (p.line, p.problem.as_str()))
        .collect();
    assert_eq!(
        p,
        [
            (4, "shell.colour: maybe is not yes or no"),
            (5, "shell.colur: no such setting"),
            (7, "not NAME = VALUE"),
            (9, "shell.history-size: set twice; the first line counts"),
        ]
    );
}

#[test]
fn the_examples_of_adr_0157() {
    let d = declared();
    let file = set(b"", &d, "shell.prompt", "zoraya {cwd}$ ").unwrap();
    assert_eq!(file, b"shell.prompt = 'zoraya {cwd}$ '\n");
    assert_eq!(
        set(&file, &d, "shell.colour", "maybe"),
        Err(Refusal::Value("maybe is not yes or no".into()))
    );
    assert_eq!(
        set(&file, &d, "shell.colur", "no"),
        Err(Refusal::Unknown(vec!["shell.colour".into()]))
    );
    assert_eq!(
        set(&file, &d, "shell.sugestions", "yes"),
        Err(Refusal::Unknown(vec!["shell.suggestions".into()]))
    );
    assert_eq!(reset(&file, "shell.prompt"), b"");
}

#[test]
fn reset_keeps_the_rest() {
    let text = b"# a\nshell.colour = no\nshell.prompt = x\n";
    assert_eq!(reset(text, "shell.colour"), b"# a\nshell.prompt = x\n");
    assert_eq!(reset(text, "shell.prompt"), b"# a\nshell.colour = no\n");
    assert_eq!(reset(b"shell.prompt = x", "shell.prompt"), b"");
    assert_eq!(reset(b"# only", "shell.prompt"), b"# only");
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: [&str; 14] = [
    "shell.prompt",
    "shell.colour",
    " = ",
    "=",
    "'",
    "\"",
    "yes",
    "no",
    "# c",
    "\n",
    "\r",
    "x y",
    "console.columns",
    "\u{e9}",
];

fn random_file(rng: &mut Rng) -> Vec<u8> {
    let mut f = Vec::new();
    for _ in 0..rng.below(30) {
        if rng.below(5) == 0 {
            f.push(rng.next() as u8);
        } else {
            f.extend_from_slice(PIECES[rng.below(PIECES.len())].as_bytes());
        }
    }
    f
}

/// INV-0157-1: after `set`, reading gives back the value, and every other
/// line is unchanged.
#[test]
fn set_changes_one_line() {
    let d = declared();
    let mut rng = Rng(7);
    let values = ["yes", "no"];
    for _ in 0..5_000 {
        let file = random_file(&mut rng);
        let value = values[rng.below(2)];
        let Ok(new) = set(&file, &d, "shell.colour", value) else {
            continue;
        };
        let v = read(&new, &d);
        let set_twice = v
            .problems
            .iter()
            .any(|p| p.problem.starts_with("shell.colour: set twice"));
        if !set_twice {
            assert_eq!(
                v.get("shell.colour"),
                Some(value),
                "{:?}",
                String::from_utf8_lossy(&new)
            );
        }
        let old: Vec<&[u8]> = file.split(|b| *b == b'\n').collect();
        let changed: Vec<&[u8]> = new.split(|b| *b == b'\n').collect();
        let differing = old.iter().zip(&changed).filter(|(a, b)| a != b).count();
        assert!(
            differing <= 1,
            "{:?} -> {:?}",
            String::from_utf8_lossy(&file),
            String::from_utf8_lossy(&new)
        );
        assert!(changed.len() <= old.len() + 2);
    }
}

/// INV-0157-2: an undeclared name or a bad value changes nothing.
#[test]
fn refusals_change_nothing() {
    let d = declared();
    for (name, value) in [
        ("shell.nope", "x"),
        ("shell.colour", "maybe"),
        ("shell.colour", ""),
        ("shell.history-size", "-1"),
        ("shell.history-size", "100001"),
        ("console.columns", "19"),
        ("console.columns", "501"),
        ("shell.mode", "medium"),
        ("shell.prompt", "a\nb"),
    ] {
        assert!(set(b"# x\n", &d, name, value).is_err(), "{name} {value}");
    }
    for (name, value) in [
        ("shell.history-size", "0"),
        ("shell.history-size", "100000"),
        ("console.columns", "20"),
        ("console.columns", "500"),
        ("shell.prompt", ""),
        ("shell.prompt", "it's \"x\""),
    ] {
        let f = set(b"", &d, name, value).unwrap();
        assert_eq!(read(&f, &d).get(name), Some(value), "{name} {value}");
    }
}

/// INV-0157-3: any bytes are read without a panic; each problem names a
/// line inside the file, once.
#[test]
fn any_file_is_read() {
    let d = declared();
    let mut rng = Rng(99);
    for _ in 0..10_000 {
        let file = random_file(&mut rng);
        let lines = 1 + file.iter().filter(|b| **b == b'\n').count();
        let v = read(&file, &d);
        let mut seen = Vec::new();
        for p in &v.problems {
            assert!(p.line >= 1 && p.line <= lines);
            assert!(!seen.contains(&p.line), "one problem per line");
            seen.push(p.line);
        }
        for s in &d {
            let value = v.get(&s.name).unwrap();
            assert!(s.ty.check(value).is_ok());
        }
    }
    let big = vec![b'#'; MAX_SIZE + 10];
    assert_eq!(read(&big, &d).problems.len(), 1);
}
