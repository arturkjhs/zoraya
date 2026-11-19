#![no_std]
#![no_main]

//! ADR-0466 from a raw-call program: the file service itself refuses to make a
//! socket node through a connection that does not hold `BIND`, whatever the
//! program says of itself. This program does not use `libposix`: it sends the
//! file protocol's requests, as a program with write on a directory and no
//! library of ours could. It starts the volume's service (`init`'s own call) and
//! takes the connections the shell takes: `ALL | BIND` for a program's own
//! directory (and a line's `/tmp`), and what a `+w` grant is (`ALL` without
//! `PUBLISH`) for any other directory.

use zoraya_proto_file::{open_flags, rights, status};
use zoraya_user::files::{ALL, DIRECTORY, File};
use zoraya_user::println;
use zoraya_user::stack::{find_disk, start_stack};

extern crate alloc;

/// What a `+w` grant of a directory is (`TREE_RIGHTS` of the shell).
const GRANT: u32 = rights::ALL & !rights::PUBLISH;
/// What the own directory's entry is (`own_connection` of the shell).
const OWN: u32 = GRANT | rights::BIND;
const SOCKET: u32 = rights::READ | rights::WRITE;

fn refused(dir: &File, name: &str, why: &str) {
    match dir.make_socket_node(name, SOCKET) {
        Err(status::ACCESS_DENIED) => println!("t_bind: {why}: refused"),
        other => panic!(
            "t_bind: {why}: expected ACCESS_DENIED, got {:?}",
            other.map(|_| ())
        ),
    }
}

fn made(dir: &File, name: &str, why: &str) {
    match dir.make_socket_node(name, SOCKET) {
        Ok(_) => println!("t_bind: {why}: made"),
        Err(s) => panic!("t_bind: {why}: expected a node, got status {s}"),
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let device = find_disk().expect("a block device: is --disk on?");
    let stack = start_stack(device.0, device.1).expect("the stack starts");
    let root = &stack.root;
    // The volume's root holds `BIND`; every directory below is opened with the rights asked.
    root.make_directory("plain").expect("plain");
    root.make_directory("own").expect("own");

    // (a) A directory the program can write (a `+w` grant, or a made directory
    // opened with `ALL`): a create is allowed, a socket node is refused.
    let plain = root.open("plain", DIRECTORY, GRANT).expect("plain");
    plain
        .open("f", open_flags::CREATE | open_flags::EXCLUSIVE, GRANT)
        .expect("a file is made there");
    refused(&plain, "s", "a writable directory");
    let all = root.open("plain", DIRECTORY, ALL).expect("plain, all");
    refused(&all, "s", "a directory with every ordinary right");
    // `BIND` cannot be asked for by a connection that has none.
    assert_eq!(
        plain.clone_as(GRANT | rights::BIND).err(),
        Some(status::ACCESS_DENIED)
    );
    assert_eq!(
        plain.open("", DIRECTORY, rights::BIND).err(),
        Some(status::ACCESS_DENIED)
    );
    println!("t_bind: BIND cannot be asked for through a connection without it");

    // (b) The own directory (and the line's `/tmp`, which is the same
    // connection with the same rights): a node is made, once.
    let own = root.open("own", DIRECTORY, OWN).expect("own");
    made(&own, "s", "the own directory");
    assert_eq!(
        own.make_socket_node("s", SOCKET).err(),
        Some(status::EXISTS),
        "the name is taken"
    );
    // A level below, opened with `BIND`, makes one; opened without, does not.
    own.make_directory("sub").expect("sub");
    let sub_bind = own.open("sub", DIRECTORY, OWN).expect("sub with BIND");
    made(&sub_bind, "s", "a level below, opened with BIND");
    let sub_plain = own.open("sub", DIRECTORY, GRANT).expect("sub without BIND");
    refused(&sub_plain, "t", "a level below, opened without BIND");

    // (c) What is derived from the own directory: a clone or a narrowing that
    // does not ask for `BIND` has none; one that does has it only of a
    // connection that holds it, and a child's `READ | CLONE` has none.
    let copy = own.clone_as(GRANT).expect("clone");
    refused(&copy, "c1", "a clone without BIND");
    let child = own.clone_as(rights::READ | rights::CLONE).expect("child");
    refused(&child, "c2", "the child of an own directory (READ | CLONE)");
    assert_eq!(
        child
            .clone_as(rights::READ | rights::CLONE | rights::BIND)
            .err(),
        Some(status::ACCESS_DENIED)
    );
    assert_eq!(
        copy.clone_as(rights::BIND).err(),
        Some(status::ACCESS_DENIED)
    );
    let narrowed = own.narrow("n", GRANT).expect("narrow");
    refused(&narrowed, "n", "a narrowing without BIND");
    let narrowed = own
        .narrow("n", rights::CONNECT | rights::CLONE)
        .expect("narrow to connect");
    refused(&narrowed, "n", "a +sock narrowing (CONNECT | CLONE)");
    assert_eq!(
        copy.narrow("n", GRANT | rights::BIND).err(),
        Some(status::ACCESS_DENIED)
    );
    // A narrowing that keeps `BIND` makes its one name and no other.
    let one = own.narrow("n", OWN).expect("narrow with BIND");
    refused(&one, "other", "another name than the narrowed one");
    made(&one, "n", "the narrowed name, with BIND");
    println!("t_bind: ok");
    stack.end();
    0
}
