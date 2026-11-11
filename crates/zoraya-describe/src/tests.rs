use super::*;

const CP: &str = "name     cp
summary  copy a file
usage    cp [--force] FROM TO
operand  FROM  read    the file to copy
operand  TO    create  the name of the copy
option   -f --force    replace TO if it exists
exit     0  copied
exit     1  not copied
---
Copies the file FROM to a new file TO. TO must not exist unless --force is given.

EXAMPLES
  cp notes.txt notes.bak
  cp --force draft.txt final.txt

SEE ALSO
  mv, rm
";

#[test]
fn the_example_of_adr_0156_parses() {
    let d = parse(CP.as_bytes()).unwrap();
    assert_eq!(d.what, What::Command);
    assert_eq!(d.name, "cp");
    assert_eq!(d.summary, "copy a file");
    assert_eq!(d.usage, ["cp [--force] FROM TO"]);
    assert_eq!(d.operands.len(), 2);
    assert_eq!(d.operands[0].name, "FROM");
    assert_eq!(d.operands[0].kind, Kind::Read);
    assert_eq!(d.operands[1].kind, Kind::Create);
    assert_eq!(d.operands[1].text, "the name of the copy");
    assert_eq!(d.options[0].short, Some('f'));
    assert_eq!(d.options[0].long, "force");
    assert_eq!(d.options[0].text, "replace TO if it exists");
    assert_eq!(d.options[0].label(), "-f, --force");
    assert_eq!(d.exits.len(), 2);
    assert_eq!(d.cwd, Cwd::Read);
    assert!(!d.question);
    assert_eq!(d.sections.len(), 3);
    assert_eq!(d.sections[0].title, None);
    assert_eq!(d.sections[0].lines.len(), 1);
    assert_eq!(d.sections[1].title.as_deref(), Some("EXAMPLES"));
    assert_eq!(
        d.sections[1].lines,
        [
            "  cp notes.txt notes.bak",
            "  cp --force draft.txt final.txt"
        ]
    );
    assert_eq!(d.sections[2].lines, ["  mv, rm"]);
}

#[test]
fn every_key() {
    let text = "name rm
summary remove files
usage rm [--recursive] PATH...
operand PATH... remove the files to remove
option -r --recursive remove directories and everything in them
option --lines N text how many
when --recursive PATH remove-tree
cwd none
question
program /bin/rm
setting shell.prompt text '{cwd}> ' the prompt
setting shell.size number 0 100000 1000 lines kept
setting shell.colour yes-no yes any colour
setting shell.mode choice fast|slow fast how
setting shell.home path /home where to start
";
    let d = parse(text.as_bytes()).unwrap();
    assert_eq!(d.operands[0].arity, Arity::Many);
    assert_eq!(d.options[1].short, None);
    assert_eq!(
        d.options[1].argument,
        Some(Argument {
            name: "N".into(),
            kind: Kind::Text
        })
    );
    assert_eq!(d.options[1].label(), "--lines N");
    assert_eq!(
        d.whens,
        [When {
            option: 0,
            operand: 0,
            kind: Kind::RemoveTree
        }]
    );
    assert_eq!(d.cwd, Cwd::None);
    assert!(d.question);
    assert_eq!(d.program.as_deref(), Some("/bin/rm"));
    assert_eq!(d.settings[0].default, "{cwd}> ");
    assert_eq!(
        d.settings[1].ty,
        SettingType::Number {
            min: 0,
            max: 100000
        }
    );
    assert_eq!(
        d.settings[3].ty,
        SettingType::Choice(vec!["fast".into(), "slow".into()])
    );
    assert!(d.sections.is_empty());
    let d =
        parse(b"name t\nsummary s\nusage t\noption -d --directory PATH is a directory\n").unwrap();
    assert_eq!(d.options[0].argument, None);
    assert_eq!(d.options[0].text, "PATH is a directory");
}

#[test]
fn operand_arities() {
    for (word, arity) in [
        ("FILE", Arity::One),
        ("[FILE]", Arity::Optional),
        ("FILE...", Arity::Many),
        ("[FILE...]", Arity::Any),
    ] {
        let text = format!("name x\nsummary s\nusage x\noperand {word} read t\n");
        let d = parse(text.as_bytes()).unwrap();
        assert_eq!(d.operands[0].name, "FILE");
        assert_eq!(d.operands[0].arity, arity);
    }
}

#[test]
fn a_topic() {
    let d = parse(b"topic keys\nsummary the keys of the line editor\n---\nLeft and Right move.\n\nCTRL KEYS\n  Ctrl-A start\n").unwrap();
    assert_eq!(d.what, What::Topic);
    assert_eq!(d.sections[1].title.as_deref(), Some("CTRL KEYS"));
}

fn refused(text: &str) -> Error {
    parse(text.as_bytes()).expect_err(text)
}

#[test]
fn refusals_name_their_line() {
    let base = "name x\nsummary s\nusage x\n";
    let cases = [
        ("bogus key\n", 4),
        ("operand file read t\n", 4),
        ("operand FILE rd t\n", 4),
        ("operand FILE read\n", 4),
        ("operand FILE... read t\noperand MORE read t\n", 5),
        ("operand [A] read t\noperand B read t\n", 5),
        ("operand A read t\noperand A read u\n", 5),
        ("option -f t\n", 4),
        ("option --Force t\n", 4),
        ("option --help t\n", 4),
        ("option -ab --x t\n", 4),
        ("option -f --force t\noption -f --fine t\n", 5),
        ("when --nope A read\n", 4),
        ("operand A read t\nwhen --x A\n", 5),
        ("cwd maybe\n", 4),
        ("question yes\n", 4),
        ("program bin/x\n", 4),
        ("exit 256 t\n", 4),
        ("exit -1 t\n", 4),
        ("exit 0 a\nexit 0 b\n", 5),
        ("setting x text a t\n", 4),
        ("setting a.b number 5 1 3 t\n", 4),
        ("setting a.b yes-no maybe t\n", 4),
        ("setting a.b path rel t\n", 4),
        ("setting a.b text 'open t\n", 4),
        ("setting a.b choice A|b a t\n", 4),
        ("setting a.b number 0 9 5\n", 4),
        ("name y\n", 4),
        ("summary again\n", 4),
        ("---\nNOT A SECTION\n", 5),
        ("---\nSEE ALSO\nx\nEXAMPLES\n", 7),
        ("---\nEXAMPLES\nEXAMPLES\n", 6),
        ("a\tb\u{7}\n", 4),
        ("x \u{85}\n", 4),
        // Mutation testing: the name rules, one character class at a time.
        ("setting a.b yes-no yes t\nsetting a.b yes-no no t\n", 5),
        ("operand 1 read t\n", 4),
        ("operand FILe read t\n", 4),
        ("option --1 t\n", 4),
        ("option --a_b t\n", 4),
        ("setting A.b yes-no yes t\n", 4),
        ("option -+ --plus t\n", 4),
        ("exit -0 t\n", 4),
        ("---\na\u{7}b\n", 5),
    ];
    for (extra, line) in cases {
        let e = refused(&format!("{base}{extra}"));
        assert_eq!(e.line, line, "{extra:?}: {e}");
    }
    assert_eq!(refused("summary s\nusage x\n").line, 0);
    assert_eq!(refused("name x\nusage x\n").line, 0);
    assert_eq!(refused("name x\nsummary s\n").line, 0);
    assert_eq!(refused("topic t\nsummary s\nusage t\n").line, 0);
    assert_eq!(refused("name X\nsummary s\nusage x\n").line, 1);
    assert_eq!(
        refused("name a-very-long-name-of-a-command\nsummary s\nusage x\n").line,
        1
    );
    let mut bad = b"name x\nsummary s\n".to_vec();
    bad.push(0xff);
    assert_eq!(parse(&bad).unwrap_err().line, 3);
    assert_eq!(parse(&vec![b'a'; MAX_SIZE + 1]).unwrap_err().line, 0);
    let long = format!("name x\nsummary {}\nusage x\n", "s".repeat(MAX_LINE));
    assert_eq!(refused(&long).line, 2);
    let many: String = (0..=MAX_ITEMS).map(|i| format!("exit {i} t\n")).collect();
    assert_eq!(refused(&format!("{base}{many}")).line, 4 + MAX_ITEMS);
}

#[test]
fn values_quote_and_unquote() {
    for value in [
        "plain",
        "",
        "two words",
        "it's",
        "a\"b",
        "back\\slash",
        "#hash",
        "{cwd}> ",
    ] {
        let quoted = quote_value(value);
        let (back, rest) = split_value(&quoted).unwrap();
        assert_eq!(back, value, "{quoted}");
        assert_eq!(rest, "");
    }
    assert_eq!(quote_value("plain"), "plain");
    assert_eq!(quote_value("two words"), "'two words'");
    assert_eq!(split_value("'a b' rest"), Ok(("a b".into(), "rest")));
    assert!(split_value("'a'b").is_err());
    assert!(split_value("a'b").is_err());
    assert!(split_value("\"\\n\"").is_err());
    assert!(split_value("").is_err());
}

#[test]
fn setting_types_check_their_bounds() {
    let n = SettingType::Number { min: 20, max: 500 };
    assert!(n.check("20").is_ok());
    assert!(n.check("500").is_ok());
    assert_eq!(
        n.check("19").unwrap_err(),
        "19 is not a number from 20 to 500"
    );
    assert!(n.check("501").is_err());
    assert!(n.check("").is_err());
    assert!(n.check("1e3").is_err());
    assert!(n.check("99999999999999999999").is_err());
    assert_eq!(
        SettingType::YesNo.check("maybe").unwrap_err(),
        "maybe is not yes or no"
    );
    assert!(SettingType::YesNo.check("yes").is_ok());
    assert!(SettingType::Path.check("/home").is_ok());
    assert!(SettingType::Path.check("home").is_err());
    let c = SettingType::Choice(vec!["a".into(), "b".into()]);
    assert!(c.check("b").is_ok());
    assert_eq!(c.check("c").unwrap_err(), "c is not one of a, b");
    assert!(SettingType::Text.check("").is_ok());
    assert_eq!(parse_number("-5"), Some(-5));
    assert_eq!(parse_number("-"), None);
}

/// ADR-0229: the `address` and `prefix` types.
#[test]
fn addresses_and_prefixes_are_plain_dotted_decimal() {
    assert_eq!(parse_address("10.0.2.15"), Some([10, 0, 2, 15]));
    assert_eq!(parse_address("255.255.255.255"), Some([255; 4]));
    for bad in [
        "",
        "10.0.2",
        "10.0.2.15.1",
        "10.0.2.256",
        "10.0.2.015",
        "10.0.2.-1",
        "10.0.2.+1",
        " 10.0.2.15",
        "10.0.2.15 ",
        "10..2.15",
        "a.b.c.d",
        "1000.0.0.1",
        "0x0a.0.0.1",
        "10.0.2.15/24",
    ] {
        assert_eq!(parse_address(bad), None, "{bad:?}");
    }
    assert_eq!(parse_prefix("10.0.2.15/24"), Some(([10, 0, 2, 15], 24)));
    assert_eq!(parse_prefix("0.0.0.0/0"), Some(([0; 4], 0)));
    for bad in [
        "10.0.2.15",
        "10.0.2.15/",
        "10.0.2.15/33",
        "10.0.2.15/024",
        "10.0.2.15/-1",
        "/24",
        "10.0.2/24",
        "10.0.2.15/2/4",
    ] {
        assert_eq!(parse_prefix(bad), None, "{bad:?}");
    }
    assert!(SettingType::Address.check("10.0.2.2").is_ok());
    assert!(SettingType::Address.check("").is_ok());
    assert_eq!(
        SettingType::Address.check("10.0.2").unwrap_err(),
        "10.0.2 is not an address"
    );
    assert!(SettingType::Prefix.check("10.0.2.15/24").is_ok());
    assert!(SettingType::Prefix.check("").is_ok());
    assert!(SettingType::Prefix.check("10.0.2.15").is_err());
    let list = parse(
        b"name x\nsummary s\nusage x\ncwd none\nsetting net.dns list of address 0 4 '' dns servers\nsetting net.ip prefix '' the address\n---\nbody\n",
    )
    .unwrap();
    assert_eq!(list.settings[0].ty.word(), "list of address 0 4");
    assert_eq!(list.settings[1].ty.word(), "prefix");
    assert!(list.settings[0].ty.check("10.0.2.3 1.1.1.1").is_ok());
    assert!(list.settings[0].ty.check("10.0.2.3 1.1.1").is_err());
}

/// A small deterministic generator, so that a failure repeats.
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

const PIECES: [&str; 24] = [
    "name ",
    "topic ",
    "summary ",
    "usage ",
    "operand ",
    "option ",
    "when ",
    "cwd ",
    "question",
    "program ",
    "exit ",
    "setting ",
    "---",
    "\n",
    " ",
    "FILE",
    "[FILE...]",
    "-f",
    "--force",
    "read",
    "'",
    "EXAMPLES",
    "number 0 9",
    "\u{e9}",
];

/// INV-0156-1: any bytes give a description or an error, never a panic, and
/// an error's line is inside the input.
#[test]
fn the_parser_is_total() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for round in 0..20_000 {
        let mut input: Vec<u8> = Vec::new();
        if round % 3 == 0 {
            // Mutations of a valid description.
            input.extend_from_slice(CP.as_bytes());
            for _ in 0..1 + rng.below(4) {
                let at = rng.below(input.len());
                match rng.below(3) {
                    0 => input[at] = rng.next() as u8,
                    1 => {
                        input.remove(at);
                    }
                    _ => {
                        let p = PIECES[rng.below(PIECES.len())];
                        input.splice(at..at, p.bytes());
                    }
                }
            }
        } else if round % 3 == 1 {
            for _ in 0..rng.below(40) {
                input.extend_from_slice(PIECES[rng.below(PIECES.len())].as_bytes());
            }
        } else {
            for _ in 0..rng.below(200) {
                input.push(rng.next() as u8);
            }
        }
        let lines = 1 + input.iter().filter(|b| **b == b'\n').count();
        match parse(&input) {
            Ok(d) => {
                assert!(!d.name.is_empty());
                assert!(
                    d.whens
                        .iter()
                        .all(|w| w.option < d.options.len() && w.operand < d.operands.len())
                );
            }
            Err(e) => assert!(e.line <= lines, "{e} of {lines}"),
        }
    }
}

fn header(extra: &str) -> Description {
    let text = format!("name x\nsummary s\nusage x\n{extra}");
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

fn problem(extra: &str) -> &'static str {
    refused(&format!("name x\nsummary s\nusage x\n{extra}")).problem
}

/// ADR-0224 item 2: `needs net-status`.
#[test]
fn needs_net_status_is_one_known_word_given_once() {
    assert!(header("needs net-status\n---\n").needs_net_status);
    assert!(!header("---\n").needs_net_status);
    assert_eq!(problem("needs\n---\n"), "needs takes net-status");
    assert_eq!(problem("needs net\n---\n"), "needs takes net-status");
    assert_eq!(
        problem("needs net-status now\n---\n"),
        "needs takes net-status"
    );
    assert_eq!(
        problem("needs net-status\nneeds net-status\n---\n"),
        "needs net-status is given once"
    );
    let topic = refused("topic t\nsummary s\nneeds net-status\n---\n");
    assert_eq!(topic.problem, "a topic has only a summary and a body");
}

/// Mutation testing: the bounds are inclusive, a description of
/// `MAX_SIZE` bytes and a line of `MAX_LINE` are accepted.
#[test]
fn the_largest_description_and_line_are_accepted() {
    let mut text = String::from("name x\nsummary s\nusage x\n---\n");
    while text.len() < MAX_SIZE {
        let room = MAX_SIZE - text.len();
        let n = room.min(1000);
        if n == 1 {
            // A last line of its newline only.
            text.push('\n');
        } else {
            text.push_str(&"a".repeat(n - 1));
            text.push('\n');
        }
    }
    assert_eq!(text.len(), MAX_SIZE);
    assert!(parse(text.as_bytes()).is_ok());
    let mut over = text.clone();
    over.push('\n');
    assert_eq!(refused(&over).line, 0);
    let summary = format!("summary {}", "s".repeat(MAX_LINE - 8));
    assert_eq!(summary.len(), MAX_LINE);
    let d = parse(format!("name x\n{summary}\nusage x\n").as_bytes()).expect("a full line");
    assert_eq!(d.summary.len(), MAX_LINE - 8);
}

/// Mutation testing: what other crates ask of a kind and a setting type.
#[test]
fn kinds_and_types_say_themselves() {
    assert_eq!(Kind::Read.shown(), "read");
    assert_eq!(Kind::Create.shown(), "created");
    assert!(!Kind::Text.is_path());
    assert!(Kind::Read.is_path());
    assert_eq!(
        SettingType::Number {
            min: 0,
            max: 100000
        }
        .word(),
        "number 0 100000"
    );
    assert_eq!(SettingType::YesNo.word(), "yes-no");
    assert_eq!(
        parse_number("999999999999999999"),
        Some(999_999_999_999_999_999)
    );
    assert_eq!(
        format!(
            "{}",
            Error {
                line: 4,
                problem: "x"
            }
        ),
        "line 4: x"
    );
    assert_eq!(
        format!(
            "{}",
            Error {
                line: 0,
                problem: "x"
            }
        ),
        "x"
    );
}

/// Mutation testing: escapes inside double quotes, and quoting a value
/// with both kinds of quote.
#[test]
fn escapes_quote_and_unquote() {
    assert_eq!(split_value("\"a\\\"b\""), Ok(("a\"b".into(), "")));
    assert_eq!(split_value("\"a\"b"), Err("text follows a closing quote"));
    let value = "it's a\"b\\c";
    let quoted = quote_value(value);
    assert_eq!(split_value(&quoted), Ok((value.into(), "")));
    assert_eq!(quote_value("it's \"x\""), "\"it's \\\"x\\\"\"");
}

/// Mutation testing: what each name may hold.
#[test]
fn names_take_their_characters() {
    let d = parse(b"name a-b2\nsummary s\nusage x\n").expect("hyphen and digit");
    assert_eq!(d.name, "a-b2");
    assert_eq!(refused("name a_b\nsummary s\nusage x\n").line, 1);
    let d = header("operand A_B read t\noperand FILE2 read t\noption --dry-run t\n");
    assert_eq!(d.operands[0].name, "A_B");
    assert_eq!(d.operands[1].name, "FILE2");
    assert_eq!(d.options[0].long, "dry-run");
}

/// Mutation testing: the setting and exit rules give their own problem.
#[test]
fn settings_and_exits_are_checked() {
    assert_eq!(
        problem("setting a.b number 5 1 3 t\n"),
        "MIN is larger than MAX"
    );
    let d = header("setting a.b number 3 3 3 t\n");
    assert_eq!(d.settings[0].ty, SettingType::Number { min: 3, max: 3 });
    assert_eq!(
        problem("setting a.b yes-no yes t\nsetting a.b yes-no no t\n"),
        "this setting appears twice"
    );
    assert_eq!(problem("exit -0 t\n"), "an exit code is 0 to 255");
    assert_eq!(header("cwd read\n").cwd, Cwd::Read);
    let d = parse(b"name x\nsummary a\tb\nusage x\n").expect("a tab is text");
    assert_eq!(d.summary, "a\tb");
}

/// Mutation testing: every key a command has and a topic has not.
#[test]
fn a_topic_has_no_command_keys() {
    for key in [
        "option -f --force t\n",
        "when --force A read\n",
        "cwd read\n",
        "question\n",
        "program /bin/x\n",
        "exit 0 ok\n",
        "setting a.b text x t\n",
    ] {
        let e = refused(&format!("topic t\nsummary s\n{key}"));
        assert_eq!(
            (e.line, e.problem),
            (0, "a topic has only a summary and a body"),
            "{key:?}"
        );
    }
}

/// Mutation testing: `when` names a short option by its one letter.
#[test]
fn a_when_names_a_short_option_by_its_letter() {
    let base = "option -f --force t\noperand A text t\n";
    let d = header(&format!("{base}when -f A read\n"));
    assert_eq!(
        d.whens,
        [When {
            option: 0,
            operand: 0,
            kind: Kind::Read
        }]
    );
    assert_eq!(
        problem(&format!("{base}when -fx A read\n")),
        "when names an option the description lacks"
    );
}

/// Mutation testing: `MAX_SIZE` leaves room for a long page (the largest
/// shipped description is under 1 KiB; one of 4 KiB is accepted).
#[test]
fn a_long_page_fits() {
    let mut text = String::from("name x\nsummary s\nusage x\n---\n");
    while text.len() < 4096 {
        text.push_str(&"a".repeat(99));
        text.push('\n');
    }
    assert!(parse(text.as_bytes()).is_ok());
}

/// ADR-0171's declarations: bounds, `unique`, and their refusals.
#[test]
fn list_settings_are_declared() {
    let head = "name t\nsummary s\nusage t\n";
    let one = |line: &str| parse(std::format!("{head}{line}\n").as_bytes());
    let d = one("setting a.b list of number 0 9 2 3 '1 2' two or three digits").unwrap();
    let SettingType::List {
        min, max, unique, ..
    } = &d.settings[0].ty
    else {
        panic!()
    };
    assert_eq!((*min, *max, *unique), (2, 3, false));
    assert_eq!(d.settings[0].default, "1 2");
    assert!(d.settings[0].ty.check("1 2 3").is_ok());
    assert!(d.settings[0].ty.check("1 2 3 4").is_err());
    assert!(d.settings[0].ty.check("1 x").is_err());
    let d = one("setting a.b list of path '' paths").unwrap();
    assert!(matches!(
        d.settings[0].ty,
        SettingType::List {
            min: 0,
            max: MAX_LIST,
            ..
        }
    ));
    for bad in [
        "setting a.b list text 'x' t",
        "setting a.b list of 'x' t",
        "setting a.b list of text 3 2 'x' t",
        "setting a.b list of text 0 65 'x' t",
        "setting a.b list of choice x|y 1 2 unique 'x x' t",
        "setting a.b list of choice x|y 1 2 'z' t",
    ] {
        assert!(one(bad).is_err(), "{bad}");
    }
    assert_eq!(
        list_items("a 'b c' \"d\""),
        Ok(std::vec!["a".into(), "b c".into(), "d".into()])
    );
    assert_eq!(list_text(&["a".into(), "b c".into()]), "a 'b c'");
    assert_eq!(list_items(""), Ok(std::vec![]));
}

#[test]
fn a_list_repeats_its_items_unless_it_is_unique() {
    let head = "name t\nsummary s\nusage t\n";
    let d = parse(std::format!("{head}setting a.b list of text 0 3 'x x' t\n").as_bytes()).unwrap();
    assert!(d.settings[0].ty.check("x x").is_ok());
}

#[test]
fn list_bounds_may_be_equal_and_reach_both_ends() {
    let head = "name t\nsummary s\nusage t\n";
    let one = |line: &str| parse(std::format!("{head}{line}\n").as_bytes());
    for good in [
        "setting a.b list of text 2 2 'x y' t",
        "setting a.b list of text 0 2 'x' t",
        "setting a.b list of text 0 64 'x' t",
    ] {
        assert!(one(good).is_ok(), "{good}");
    }
}

#[test]
fn a_list_with_bad_bounds_names_the_bounds() {
    let head = "name t\nsummary s\nusage t\n";
    let one = |line: &str| parse(std::format!("{head}{line}\n").as_bytes());
    for bad in [
        "setting a.b list of text 3 2 'x' t",
        "setting a.b list of text -1 3 'x' t",
    ] {
        let e = one(bad).unwrap_err();
        assert_eq!(
            e.problem, "a list's MIN and MAX are counts from 0 to 64, MIN first",
            "{bad}"
        );
    }
}

/// A choice's words may be numbers (`gui.scale`, ADR-0169), not mixed
/// case, punctuation or empty.
#[test]
fn choices_of_words_and_numbers() {
    let head = "name t\nsummary s\nusage t\n";
    let one = |line: &str| parse(std::format!("{head}{line}\n").as_bytes());
    let d = one("setting a.b choice auto|1|2 auto scale").unwrap();
    assert!(d.settings[0].ty.check("2").is_ok());
    assert!(d.settings[0].ty.check("3").is_err());
    for bad in [
        "choice Auto|1 1 t",
        "choice a||b a t",
        "choice a|b. a t",
        "choice 1.5|2 2 t",
    ] {
        assert!(one(&std::format!("setting a.b {bad}")).is_err(), "{bad}");
    }
}
