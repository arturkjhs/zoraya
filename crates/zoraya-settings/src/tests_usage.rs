use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::Cell;

use crate::rooms::Limits;
use crate::usage::*;

const MIB: u64 = 1 << 20;

fn agent() -> Reading {
    Reading {
        name: "agent-1".into(),
        running: true,
        untrusted: true,
        mem_used: 58 * MIB,
        mem_limit: Some(64 * MIB),
        mem_refused: 3,
        procs_used: 14,
        procs_limit: Some(64),
        procs_refused: 0,
        cpu_permille: Some(250),
        cpu_pct: Some(25),
        disk_limit: Some(2 << 30),
        conns_limit: Some(16),
        tmp_limit: Some(16 * MIB),
    }
}

#[test]
fn top_the_room_words_the_card_and_grants_show_the_same_limits_and_use() {
    // INV-0267-1 and INV-0266-3: one reading, three renderings.
    let r = agent();
    let row = top_table(core::slice::from_ref(&r));
    let words = room_words(&r);
    let card = r.card_lines();
    // The same use in all three.
    assert!(row.contains("58M of 64M"), "{row}");
    assert!(words.contains("--mem 64M (58M)"), "{words}");
    assert!(card.contains(&String::from("mem 58M of 64M")), "{card:?}");
    assert!(row.contains("14 of 64") && words.contains("--procs 64 (14)"));
    assert!(card.contains(&String::from("procs 14 of 64")));
    assert!(row.contains("25% of 25%") && card.contains(&String::from("cpu 25% of 25%")));
    // The limit words are the line's own: `grants` prints `Limits::words()`.
    let line = Limits {
        mem: Some(64 * MIB),
        cpu: Some(25),
        procs: Some(64),
        disk: Some(2 << 30),
        conns: Some(16),
        tmp: Some(16 * MIB),
    };
    assert_eq!(r.limits(), line);
    let grants = alloc::format!("limits {}", line.words());
    assert_eq!(r.limits_line(), grants);
    assert!(card.contains(&grants));
    assert!(row.contains("(refused 3 mem)"), "{row}");
    // The notice: what refused, and the line that raises it twofold, the
    // words of the shell's hint (`quota_hint`).
    assert!(card.contains(&String::from("(refused 3 mem)")));
    assert_eq!(
        r.raise_line().as_deref(),
        Some("raise: room set agent-1 --mem 128M")
    );
    let hint = crate::rooms::quota_hint("agent-1", crate::rooms::Refused::Memory, 64 * MIB, true);
    assert!(hint.contains("room set agent-1 --mem 128M"), "{hint}");
    let mut p = agent();
    p.mem_refused = 0;
    p.procs_refused = 1;
    assert_eq!(
        p.raise_line().as_deref(),
        Some("raise: room set agent-1 --procs 128")
    );
    p.procs_refused = 0;
    assert_eq!(p.raise_line(), None);
}

#[test]
fn a_room_without_limits_shows_its_use_alone_and_no_limits_line() {
    let r = Reading {
        name: "home".into(),
        running: true,
        mem_used: 41 * MIB,
        procs_used: 12,
        ..Reading::default()
    };
    let cells = r.cells();
    assert_eq!(cells[0], "41M");
    assert_eq!(cells[2], "12");
    assert_eq!(cells[3], "-");
    assert_eq!(r.limits_line(), "");
    assert_eq!(room_words(&r), "");
    assert!(r.pills().is_empty());
    assert_eq!(r.trailer(), "");
}

#[test]
fn a_pill_shows_at_ninety_percent_and_not_below() {
    let mut r = agent();
    r.cpu_pct = Some(0);
    r.mem_used = 64 * MIB * 89 / 100;
    assert!(r.pills().is_empty(), "89%: {:?}", r.pills());
    r.mem_used = 64 * MIB * 90 / 100 + 1;
    assert_eq!(r.pills(), vec![String::from("mem 90%")]);
    r.mem_used = 64 * MIB;
    r.procs_used = 64;
    assert_eq!(
        r.pills(),
        vec![String::from("mem 100%"), String::from("procs 100%")]
    );
    // CPU: 24% of a 25% share is 96%.
    r.mem_used = 0;
    r.procs_used = 0;
    r.cpu_pct = Some(24);
    assert_eq!(r.pills(), vec![String::from("cpu 96%")]);
    // A room that does not run shows none, whatever it last used.
    r.running = false;
    r.cpu_pct = Some(25);
    assert!(r.pills().is_empty());
}

#[test]
fn a_limit_of_zero_and_huge_numbers_do_not_overflow() {
    let r = Reading {
        name: "x".into(),
        running: true,
        mem_used: u64::MAX,
        mem_limit: Some(0),
        procs_used: u64::MAX,
        procs_limit: Some(u64::MAX),
        cpu_permille: Some(u64::MAX),
        cpu_pct: Some(u64::MAX),
        ..Reading::default()
    };
    // Zero limit: no pill (nothing can be used of nothing), no panic.
    let _ = r.pills();
    let _ = r.card_lines();
    let _ = top_table(&[r]);
}

#[test]
fn cpu_share_is_between_two_readings() {
    assert_eq!(cpu_share(0, 250_000_000, 1_000_000_000), Some(25));
    assert_eq!(cpu_share(5, 5, 1_000_000_000), Some(0));
    assert_eq!(cpu_share(9, 5, 1_000_000_000), None);
    assert_eq!(cpu_share(0, 5, 0), None);
    assert_eq!(cpu_share(0, u64::MAX, 1), Some(100));
}

#[test]
fn the_wire_round_trips_and_every_other_text_is_refused() {
    let mut a = agent();
    let b = Reading {
        name: "home".into(),
        running: true,
        mem_used: 7,
        ..Reading::default()
    };
    a.cpu_pct = None;
    let rows = vec![a.clone(), b.clone()];
    let text = to_wire(&rows, 4096);
    assert_eq!(from_wire(&text), Some(rows));
    // A longer line, a missing field, a bad name, a repeated name: none.
    for bad in [
        "agent-1 3 1 2 3 4 5 6 7 8 9 10 11 12\n",
        "agent-1 3 1 2 3 4 5 6 7 8 9 10\n",
        "Agent-1 3 1 2 3 4 5 6 7 8 9 10 11\n",
        "agent-1 9 1 2 3 4 5 6 7 8 9 10 11\n",
        "agent-1 3 x 2 3 4 5 6 7 8 9 10 11\n",
        "agent-1 3 1 2 3 4 5 6 7 8 9 10 11\nagent-1 3 1 2 3 4 5 6 7 8 9 10 11\n",
    ] {
        assert_eq!(from_wire(bad), None, "{bad:?}");
    }
    // Only as many as fit, whole lines.
    let one = a.to_wire().len() + 1;
    assert_eq!(to_wire(&[a.clone(), b], one).lines().count(), 1);
    assert_eq!(to_wire(&[a], one - 1), "");
}

#[test]
fn random_text_never_panics() {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let alphabet = b"0123456789 -\nabcdefghijklmnopqrstuvwxyz";
    for _ in 0..3000 {
        let n = (next() % 120) as usize;
        let s: Vec<u8> = (0..n)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
            .collect();
        if let Ok(t) = core::str::from_utf8(&s) {
            let _ = from_wire(t);
        }
    }
}

#[test]
fn usage_is_read_at_most_once_a_second_under_watch() {
    // INV-0267-2: a fake clock that ticks 10 ms, a counting keeper.
    let clock = Cell::new(0u64);
    let reads = Cell::new(0u32);
    let shown = Cell::new(0u32);
    let turns = Cell::new(0u32);
    let times: core::cell::RefCell<Vec<u64>> = core::cell::RefCell::new(Vec::new());
    watch(
        Throttle::new(),
        || clock.get(),
        |ns| clock.set(clock.get() + ns.min(10_000_000)),
        || {
            reads.set(reads.get() + 1);
            times.borrow_mut().push(clock.get());
            Some(vec![agent()])
        },
        |_| shown.set(shown.get() + 1),
        || {
            turns.set(turns.get() + 1);
            clock.get() >= 5_500_000_000
        },
    );
    assert!(turns.get() > 100, "the loop turned {} times", turns.get());
    assert_eq!(reads.get(), shown.get());
    let t = times.borrow();
    assert!(t.len() >= 5, "{t:?}");
    for w in t.windows(2) {
        assert!(w[1] - w[0] >= MIN_INTERVAL_NS, "{t:?}");
    }
    assert!(reads.get() <= 6);
}

#[test]
fn watch_ends_when_the_keeper_is_gone() {
    let mut reads = 0;
    watch(
        Throttle::new(),
        || 0,
        |_| {},
        || {
            reads += 1;
            None
        },
        |_| panic!("nothing to show"),
        || false,
    );
    assert_eq!(reads, 1);
}

#[test]
fn the_throttle_survives_a_clock_that_goes_back() {
    let mut t = Throttle::new();
    assert!(t.due(5_000_000_000));
    assert!(!t.due(5_500_000_000));
    assert!(t.due(6_000_000_000));
    assert!(t.due(1)); // back in time: one reading, start again
    assert!(!t.due(2));
    assert_eq!(t.wait(500_000_001), MIN_INTERVAL_NS - 500_000_000);
}

#[test]
fn a_reading_the_caller_took_counts_against_the_first_turn() {
    // `top --watch` reads once to check its room, then watches: the first
    // turn must not read again at once.
    let clock = Cell::new(1_000_000_000u64);
    let reads = Cell::new(0u32);
    let mut throttle = Throttle::new();
    assert!(throttle.due(clock.get()));
    watch(
        throttle,
        || clock.get(),
        |ns| clock.set(clock.get() + ns.min(100_000_000)),
        || {
            reads.set(reads.get() + 1);
            assert!(clock.get() >= 2_000_000_000, "read at {}", clock.get());
            Some(vec![agent()])
        },
        |_| {},
        || clock.get() >= 2_050_000_000,
    );
    assert_eq!(reads.get(), 1);
}

#[test]
fn rows_round_trip_and_refuse_every_other_text() {
    let a = agent();
    let rows = vec![
        RoomRow {
            name: "agent-1".into(),
            untrusted: true,
            limits: a.limits(),
            reading: Some(a),
        },
        RoomRow {
            name: "project".into(),
            untrusted: false,
            limits: Limits {
                mem: Some(512 * MIB),
                ..Limits::default()
            },
            reading: None,
        },
        RoomRow {
            name: "plain".into(),
            untrusted: false,
            limits: Limits::default(),
            reading: None,
        },
    ];
    let text = rows_to_wire(&rows, 6144);
    assert_eq!(rows_from_wire(&text), Some(rows.clone()));
    for bad in [
        "agent-1\t2\t\t-\n",
        "Agent\t0\t\t-\n",
        "x\t0\t--mem\t-\n",
        "x\t0\t--mem 1\t-\n",
        "x\t0\t--tmp 1M\t-\n",
        "x\t0\t\t-\textra\n",
        "x\t0\t\tx 1 2\n",
        "x\t0\t\t-\nx\t0\t\t-\n",
    ] {
        assert_eq!(rows_from_wire(bad), None, "{bad:?}");
    }
    let one = rows[0].to_wire().len() + 1;
    assert_eq!(rows_to_wire(&rows, one).lines().count(), 1);
}

#[test]
fn a_limit_is_changed_in_the_table_as_room_set_would() {
    use crate::rooms::with_limit;
    let table = b"# mine\nproject --colour 1 --mem 64M +w ~/p\nagent-1 --untrusted --mem 89M --cpu 25% --procs 32 --disk 1G --conns 16 --tmp 16M +w ~/s\n";
    let (new, old) = with_limit(table, "project", "--mem", "128M").unwrap();
    assert_eq!(old.limits.mem, Some(64 * MIB));
    assert_eq!(
        new,
        b"# mine\nproject --colour 1 --mem 128M +w ~/p\nagent-1 --untrusted --mem 89M --cpu 25% --procs 32 --disk 1G --conns 16 --tmp 16M +w ~/s\n"
    );
    // The old line put back restores every byte.
    assert_eq!(crate::rooms::replace(&new, &old).unwrap(), table);
    // `none` removes a trusted word; an untrusted room cannot be emptied.
    assert!(with_limit(table, "project", "--mem", "none").is_ok());
    assert!(with_limit(table, "agent-1", "--mem", "none").is_err());
    // A bad value, a word of the other trust, a room that is not there.
    assert!(with_limit(table, "project", "--mem", "12").is_err());
    assert!(with_limit(table, "project", "--tmp", "1M").is_err());
    assert!(with_limit(table, "nope", "--mem", "1M").is_err());
    assert!(with_limit(table, "project", "--colour", "2").is_err());
    // An untrusted line that lacks a word is not completed here.
    let thin = b"a --untrusted --mem 8M +w ~/s\n";
    assert!(with_limit(thin, "a", "--mem", "9M").is_err());
}
