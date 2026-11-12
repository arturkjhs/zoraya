//! `netd`'s accounts (ADR-0262 item 2; N4): INV-0262-1 (one account at its
//! bound is refused while another uses its full bound), INV-0262-2 (account
//! state is freed with the last connection) and INV-0262-3 (the accounts'
//! limits always sum to the service's bound), on the table alone and through
//! the service. The connections' side (pending resolutions, the cache, the
//! release of a charge) is in `conn_tests.rs`.

use super::*;
use crate::accounts::{
    ALL, Accounts, COLUMN, CarveError, GLOBAL, KINDS, Kind, MAX_BUDGET, MAX_CARVED, SESSION,
};
use crate::service::{Action, Service};
use zoraya_abi::net::{Ports, Protocols, ReachRule, Scope, Target as ScopeTarget, rights};
use zoraya_proto_net::{NarrowAccount, Narrowed, status};
use zoraya_protocol::{Below, Data, decode, encode};

fn sums_hold(a: &Accounts) {
    for k in ALL {
        assert_eq!(
            a.total_limit(k),
            GLOBAL[k as usize],
            "{k:?} sums to the global bound"
        );
    }
}

#[test]
fn the_session_starts_with_every_bound_and_never_ends() {
    let mut a = Accounts::new();
    sums_hold(&a);
    assert_eq!(a.live(), 1);
    assert_eq!(a.limit(SESSION, Kind::Connections), 16);
    a.join(SESSION);
    a.leave(SESSION);
    assert!(a.exists(SESSION));
}

#[test]
fn a_carve_takes_its_limits_from_the_caller_and_gives_them_back() {
    let mut a = Accounts::new();
    let c = a.carve(SESSION, 4).unwrap();
    sums_hold(&a);
    assert_eq!(a.limit(c, Kind::Connections), 4);
    assert_eq!(a.limit(c, Kind::Resolutions), 2);
    assert_eq!(a.limit(c, Kind::CacheEntries), 16);
    assert_eq!(a.limit(c, Kind::Listeners), 0);
    assert_eq!(a.limit(SESSION, Kind::Connections), 12);
    // Nothing holds it: it ends at the last hold.
    a.join(c);
    a.leave(c);
    assert!(!a.exists(c));
    assert_eq!(a.limit(SESSION, Kind::Connections), 16);
    sums_hold(&a);
}

#[test]
fn a_budget_of_zero_or_over_the_most_is_invalid_and_changes_nothing() {
    let mut a = Accounts::new();
    for budget in [0, MAX_BUDGET + 1, 16, u32::MAX] {
        assert_eq!(a.carve(SESSION, budget), Err(CarveError::Invalid));
    }
    assert_eq!(a.carve(99, 1), Err(CarveError::Invalid));
    assert_eq!(a.live(), 1);
    sums_hold(&a);
}

#[test]
fn the_column_leaves_the_session_its_half_and_there_are_at_most_four_carves() {
    let mut a = Accounts::new();
    let one = a.carve(SESSION, 4).unwrap();
    let two = a.carve(SESSION, 4).unwrap();
    // Eight connections are the column: a third carve of any size is out.
    assert_eq!(a.carve(SESSION, 1), Err(CarveError::OverQuota));
    assert_eq!(a.limit(SESSION, Kind::Connections), 8);
    sums_hold(&a);
    // Freed, the room is there again.
    a.join(one);
    a.leave(one);
    assert!(a.carve(SESSION, 1).is_ok());
    let _ = two;
    // Four small carves are the most there are, the column's room aside.
    let mut b = Accounts::new();
    for _ in 0..MAX_CARVED {
        assert!(b.carve(SESSION, 1).is_ok());
    }
    assert_eq!(b.carved(), MAX_CARVED);
    assert_eq!(b.carve(SESSION, 1), Err(CarveError::OverQuota), "a fifth");
    sums_hold(&b);
    sums_hold(&a);
    for k in ALL {
        let held: u32 = (1..8).map(|i| a.limit(i, k)).sum();
        assert!(held <= COLUMN[k as usize], "{k:?}: {held}");
    }
}

#[test]
fn inv_0262_1_one_account_at_its_bound_is_refused_while_another_uses_all_of_its() {
    let mut a = Accounts::new();
    let x = a.carve(SESSION, 2).unwrap();
    let y = a.carve(SESSION, 4).unwrap();
    a.join(x);
    a.join(y);
    for _ in 0..2 {
        assert!(a.charge(x, Kind::Connections).is_ok());
    }
    assert!(a.charge(x, Kind::Connections).is_err());
    assert_eq!(a.refusals(x, Kind::Connections), 1);
    // The refusal changed nothing of anyone else, and y gets all four.
    for _ in 0..4 {
        assert!(a.charge(y, Kind::Connections).is_ok());
    }
    assert!(a.charge(y, Kind::Connections).is_err());
    assert_eq!(a.refusals(y, Kind::Connections), 1);
    assert_eq!(a.refusals(SESSION, Kind::Connections), 0);
    for _ in 0..10 {
        assert!(a.charge(SESSION, Kind::Connections).is_ok());
    }
    assert_eq!(a.used(x, Kind::Connections), 2);
    sums_hold(&a);
    // A released charge is room again, for that account alone.
    a.release(x, Kind::Connections);
    assert!(a.charge(x, Kind::Connections).is_ok());
    assert!(a.charge(y, Kind::Connections).is_err());
}

#[test]
fn inv_0262_2_an_account_ends_with_its_last_connection_and_its_last_charge() {
    let mut a = Accounts::new();
    let c = a.carve(SESSION, 3).unwrap();
    a.join(c);
    a.join(c);
    assert!(a.charge(c, Kind::Connections).is_ok());
    a.leave(c);
    a.leave(c);
    // No scope connection left, but a TCP connection still holds it.
    assert!(a.exists(c));
    assert_eq!(a.limit(SESSION, Kind::Connections), 13);
    a.release(c, Kind::Connections);
    assert!(!a.exists(c));
    assert_eq!(a.live(), 1);
    for k in ALL {
        assert_eq!(a.limit(SESSION, k), GLOBAL[k as usize]);
        assert_eq!(a.used(SESSION, k), 0);
    }
}

#[test]
fn a_carve_from_a_carved_account_stays_inside_it_and_the_parent_outlives_it() {
    let mut a = Accounts::new();
    let p = a.carve(SESSION, 4).unwrap();
    a.join(p);
    let k = a.carve(p, 2).unwrap();
    a.join(k);
    sums_hold(&a);
    assert_eq!(a.limit(p, Kind::Connections), 2);
    assert_eq!(
        a.carve(p, 3),
        Err(CarveError::OverQuota),
        "more than it has"
    );
    // The parent has no connection left but its child lives.
    a.leave(p);
    assert!(a.exists(p));
    a.leave(k);
    assert!(!a.exists(k) && !a.exists(p));
    assert_eq!(a.limit(SESSION, Kind::Connections), 16);
    sums_hold(&a);
}

#[test]
fn a_carve_never_takes_what_the_parent_already_uses() {
    let mut a = Accounts::new();
    for _ in 0..9 {
        a.charge(SESSION, Kind::Connections).unwrap();
    }
    // 9 in use, 7 free: a carve of 4 would leave the session 12, over its
    // use? No: 12 >= 9, it is allowed; one more charge each time.
    assert!(a.carve(SESSION, 4).is_ok());
    for _ in 0..3 {
        a.charge(SESSION, Kind::Connections).unwrap();
    }
    assert!(a.charge(SESSION, Kind::Connections).is_err());
    // Now the session's limit equals its use: nothing can be carved.
    assert_eq!(a.carve(SESSION, 1), Err(CarveError::OverQuota));
    sums_hold(&a);
}

#[test]
fn random_runs_keep_the_sums_the_column_and_the_counts() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut a = Accounts::new();
    // Each account's scope connections and charges, as the loop holds them.
    let mut handles = [0u32; 16];
    let mut charges = vec![[0u32; KINDS]; 16];
    for _ in 0..4000 {
        let id = (next() % 8) as usize;
        match next() % 6 {
            0 => {
                let from = if a.exists(id) { id } else { SESSION };
                if let Ok(c) = a.carve(from, (next() % 6) as u32) {
                    assert!(c < 16);
                }
            }
            1 if a.exists(id) => {
                a.join(id);
                handles[id] += 1;
            }
            2 if a.exists(id) && handles[id] > 0 => {
                a.leave(id);
                handles[id] -= 1;
            }
            3 if a.exists(id) => {
                let k = ALL[(next() % KINDS as u64) as usize];
                if a.charge(id, k).is_ok() {
                    charges[id][k as usize] += 1;
                }
            }
            4 if a.exists(id) => {
                let k = ALL[(next() % KINDS as u64) as usize];
                if charges[id][k as usize] > 0 {
                    a.release(id, k);
                    charges[id][k as usize] -= 1;
                }
            }
            _ => {}
        }
        sums_hold(&a);
        for i in 0..16 {
            if !a.exists(i) {
                assert!(i != SESSION);
                handles[i] = 0;
                charges[i] = [0; KINDS];
                continue;
            }
            for k in ALL {
                assert!(a.used(i, k) <= a.limit(i, k), "{i} {k:?}");
                assert_eq!(a.used(i, k), charges[i][k as usize]);
            }
        }
        assert!(a.carved() <= MAX_CARVED);
        for k in ALL {
            let held: u32 = (1..16).map(|i| a.limit(i, k)).sum();
            assert!(held <= COLUMN[k as usize], "{k:?}");
        }
    }
    // Everything let go: only the session is left.
    for i in 0..16 {
        while a.exists(i) && handles[i] > 0 {
            a.leave(i);
            handles[i] -= 1;
        }
        for k in ALL {
            while a.exists(i) && charges[i][k as usize] > 0 {
                a.release(i, k);
                charges[i][k as usize] -= 1;
            }
        }
    }
    assert_eq!(a.live(), 1);
    sums_hold(&a);
}

fn narrow_account(
    service: &mut Service,
    net: &mut Net,
    from: usize,
    budget: u32,
    wanted: u32,
    scope: &[u8],
) -> Result<usize, u32> {
    let mut out = vec![0u8; 512];
    let n = encode(
        &NarrowAccount {
            rights: wanted,
            budget: Below(budget),
            scope: Data(scope),
        },
        5,
        &mut out,
    )
    .unwrap();
    match service.handle(from, &out[..n], 1, &mut net.client, 0) {
        Action::Narrowed { id, .. } => Ok(id),
        Action::Reply(r) => Err(decode::<Narrowed>(&r, 0).unwrap().1.status.0),
        other => panic!("{other:?}"),
    }
}

fn connect_to(service: &mut Service, net: &mut Net, id: usize) -> Action {
    let mut out = vec![0u8; 256];
    let n = encode(
        &zoraya_proto_net::Connect {
            name: Data(b""),
            address: u32::from_be_bytes([10, 0, 2, 50]),
            port: Below(80),
        },
        6,
        &mut out,
    )
    .unwrap();
    service.handle(id, &out[..n], 3, &mut net.client, 0)
}

fn reach_scope() -> Vec<u8> {
    let mut out = vec![0u8; 128];
    let r = ReachRule {
        target: ScopeTarget::Any,
        ports: Ports::ANY,
        protocols: Protocols::TCP,
    };
    let n = zoraya_abi::net::encode(&[r], &[], &mut out).unwrap();
    out.truncate(n);
    out
}

#[test]
fn a_narrow_with_a_budget_makes_an_account_and_a_plain_narrow_joins_the_callers() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let root = service.add(Connection::root()).unwrap();
    assert_eq!(service.accounts().live(), 1);
    let empty = Scope::EMPTY.as_bytes();
    let a = narrow_account(&mut service, &mut net, root, 2, rights::REACH, empty).unwrap();
    let acct = service.connection(a).unwrap().account();
    assert_ne!(acct, SESSION);
    assert_eq!(service.accounts().limit(acct, Kind::Connections), 2);
    // Bad budgets are refused before anything is carved.
    for budget in [0, 5, 16] {
        assert_eq!(
            narrow_account(&mut service, &mut net, root, budget, rights::REACH, empty),
            Err(status::INVALID)
        );
    }
    assert_eq!(service.accounts().live(), 2);
    // The account ends with its connection.
    service.remove(a);
    assert_eq!(service.accounts().live(), 1);
    sums_hold(service.accounts());
    // A connection without the right to narrow cannot carve.
    let weak = service
        .add(Connection::new(rights::REACH, empty).unwrap())
        .unwrap();
    assert_eq!(
        narrow_account(&mut service, &mut net, weak, 1, rights::REACH, empty),
        Err(status::NOT_GRANTED)
    );
    // Past the column: OVER_QUOTA, and nothing changed.
    let b1 = narrow_account(&mut service, &mut net, root, 4, rights::REACH, empty).unwrap();
    let b2 = narrow_account(&mut service, &mut net, root, 4, rights::REACH, empty).unwrap();
    assert_eq!(
        narrow_account(&mut service, &mut net, root, 1, rights::REACH, empty),
        Err(status::OVER_QUOTA)
    );
    assert_eq!(service.accounts().carved(), 2);
    service.remove(b1);
    service.remove(b2);
    assert_eq!(service.accounts().carved(), 0);
    sums_hold(service.accounts());
}

#[test]
fn a_narrow_without_a_budget_joins_its_parents_account() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let scope = reach_scope();
    let root = service.add(Connection::root()).unwrap();
    let acct_conn = narrow_account(
        &mut service,
        &mut net,
        root,
        2,
        rights::REACH | rights::NARROW,
        &scope,
    )
    .unwrap();
    let acct = service.connection(acct_conn).unwrap().account();
    // `narrow` from it joins the same account.
    let mut out = vec![0u8; 512];
    let n = encode(
        &zoraya_proto_net::Narrow {
            rights: rights::REACH,
            scope: Data(&scope),
        },
        3,
        &mut out,
    )
    .unwrap();
    let Action::Narrowed { id: child, .. } =
        service.handle(acct_conn, &out[..n], 1, &mut net.client, 0)
    else {
        panic!("narrowed");
    };
    assert_eq!(service.connection(child).unwrap().account(), acct);
    assert_eq!(service.accounts().carved(), 1);
    // It ends only with both.
    service.remove(acct_conn);
    assert!(service.accounts().exists(acct));
    service.remove(child);
    assert!(!service.accounts().exists(acct));
}

#[test]
fn inv_0262_1_connect_at_the_bound_is_over_quota_while_another_account_connects() {
    let mut net = Net::new(static_config());
    let mut service = Service::new();
    let scope = reach_scope();
    let root = service
        .add(Connection::new(rights::ALL, &scope).unwrap())
        .unwrap();
    let x = narrow_account(&mut service, &mut net, root, 2, rights::REACH, &scope).unwrap();
    let y = narrow_account(&mut service, &mut net, root, 4, rights::REACH, &scope).unwrap();
    let over = |a: &Action| match a {
        Action::Connect { over_quota, .. } => *over_quota,
        other => panic!("{other:?}"),
    };
    let account_of = |a: &Action| match a {
        Action::Connect { account, .. } => *account,
        other => panic!("{other:?}"),
    };
    let first = connect_to(&mut service, &mut net, x);
    let second = connect_to(&mut service, &mut net, x);
    assert!(!over(&first) && !over(&second));
    assert!(
        over(&connect_to(&mut service, &mut net, x)),
        "the third of two"
    );
    // The other account and the session still connect, all they may.
    for _ in 0..4 {
        assert!(!over(&connect_to(&mut service, &mut net, y)));
    }
    assert!(over(&connect_to(&mut service, &mut net, y)));
    for _ in 0..10 {
        assert!(!over(&connect_to(&mut service, &mut net, root)));
    }
    sums_hold(service.accounts());
    // A refused connect took no charge; an ended one gives its charge back.
    let acct = account_of(&first);
    assert_eq!(service.accounts().used(acct, Kind::Connections), 2);
    assert_eq!(service.accounts().refusals(acct, Kind::Connections), 1);
    service.connection_ended(acct);
    assert!(!over(&connect_to(&mut service, &mut net, x)));
    assert_eq!(service.accounts().used(acct, Kind::Connections), 2);
}
