//! `install` and `programs` (ADR-0396, ADR-0397, ADR-0399), and what the shell
//! does for an installed program: the lookup, the check of the bytes loaded,
//! the saved words. The rules are in `zoraya_shell::installed` and
//! `zoraya_settings::apps`, host tested; this file reads and writes the
//! volume and reports.

use super::*;

use zoraya_settings::apps::{self, App, Apps, Word};
use zoraya_settings::sha256::{self, Sha256};
use zoraya_shell::eval::Value;
use zoraya_shell::installed as plan;

/// The most entries, levels and bytes one package may hold.
const MAX_ENTRIES: usize = 2000;
const MAX_DEPTH: usize = 6;
const MAX_BYTES: u64 = 256 << 20;

/// What `install` found while copying.
#[derive(Default)]
struct Copied {
    files: usize,
    bytes: u64,
    entries: usize,
}

/// The program's file and what hashing it needs.
struct Identity<'a> {
    name: &'a str,
    length: u64,
    hasher: Sha256,
}

impl Z {
    /// Why this shell may not run `install` or `programs`, or `None`
    /// (INV-0396-5): an untrusted room, or a shell that does not hold both
    /// `/apps` and `/config`.
    fn apps_refusal(&self) -> Option<String> {
        let holds = |dir: &str| self.root.as_ref().is_some_and(|h| h.covers(dir, true));
        plan::gate(
            self.room.as_ref().is_some_and(|r| r.untrusted),
            holds("/apps"),
            holds("/config"),
        )
        .map(String::from)
    }

    fn read_apps(&self) -> Apps {
        let bytes = self
            .open(apps::RECORD, 0)
            .and_then(|f| read_all(&f))
            .unwrap_or_default();
        apps::read(&bytes)
    }

    fn apps_bytes(&self) -> Vec<u8> {
        self.open(apps::RECORD, 0)
            .and_then(|f| read_all(&f))
            .unwrap_or_default()
    }

    /// The date and the log's time stamp, from the volume's clock (ADR-0226);
    /// empty when there is none.
    fn wall_time(&self) -> (String, String) {
        match zoraya_user::namespace::realtime() {
            Some(now) => {
                let s = zoraya_files::format::utc(now);
                // `YYYY-MM-DD HH:MM:SS UTC`
                let date = s.get(..10).unwrap_or("").to_string();
                let time = s.get(11..19).unwrap_or("00:00:00");
                (date.clone(), format!("{date}T{time}"))
            }
            None => (String::new(), String::from("unknown")),
        }
    }

    /// One line in `/config/apps.log` (ADR-0399 item 5): written with the
    /// record, never for a launch. A log that cannot be written is said,
    /// and the change stands.
    fn apps_log(&self, io: &mut Io, verb: &str, name: &str, detail: &str) {
        let (_, stamp) = self.wall_time();
        let old = self
            .open(apps::LOG, 0)
            .and_then(|f| read_all(&f))
            .unwrap_or_default();
        let new = apps::log_append(&old, &apps::log_line(&stamp, verb, name, detail));
        if let Err(e) = self.publish_file(apps::LOG, &new) {
            io.err.push_str(&format!(
                "zosh: {}: the log could not be written: {}\n",
                apps::LOG,
                one_line(&e)
            ));
        }
    }

    /// What already has `name` (ADR-0396 item 1).
    fn name_taken(&mut self, state: &State, name: &str) -> Option<&'static str> {
        if BUILTINS.contains(&name) || LANGUAGE.contains(&name) || FILE_COMMANDS.contains(&name) {
            Some("a builtin")
        } else if matches!(name, "grant" | "revoke" | "remove") {
            Some("a word of programs")
        } else if state.function(name).is_some() {
            Some("a function of the shell")
        } else if TOPICS.contains(&name) {
            Some("a help topic")
        } else if self.lookup_image(name) != Found::Nothing {
            Some("an image command")
        } else if self.read_rooms().get(name).is_some() || name == zoraya_settings::rooms::HOME {
            Some("a room")
        } else if grants::data_names().any(|n| n == name) {
            Some("a program's own directory")
        } else if self.find_on_volume(name).is_some() {
            Some("a program of the path")
        } else {
            None
        }
    }

    /// The words as the record keeps them: `~` for the shell's home, which
    /// the line's expansion made a path of (`+w ~/notes` reaches a builtin as
    /// `+w /home/notes`). The record is read as the person wrote it.
    fn unexpand(&self, words: Vec<Word>) -> Vec<Word> {
        let home = self.home();
        let back = |p: String| grants::shown(&p, &home);
        words
            .into_iter()
            .map(|w| match w {
                Word::Read(p) => Word::Read(back(p)),
                Word::Write(p) => Word::Write(back(p)),
                Word::Own(p) => Word::Own(back(p)),
                other => other,
            })
            .collect()
    }

    /// The check of a word to be saved, with the shell's view of the volume.
    fn check_saved(&self, words: &[Word]) -> Result<(), String> {
        let home = self.home();
        let is_file = |abs: &str| {
            self.open(abs, 0)
                .and_then(|f| f.stat())
                .is_ok_and(|s| s.kind != proto_kind::DIRECTORY)
        };
        let net = |t: &str| match zoraya_shell::netgrant::parse_word(t) {
            Ok(w) if zoraya_shell::netgrant::is_any(&w) => {
                Err(String::from("any is not saved: name the host"))
            }
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        let ctx = apps::Ctx {
            home: &home,
            is_file: &is_file,
            net: &net,
        };
        words.iter().try_for_each(|w| apps::check_word(w, &ctx))
    }

    // --- install ---

    pub(super) fn install_builtin(
        &mut self,
        state: &State,
        d: &Description,
        p: &Parsed<'_>,
        io: &mut Io,
    ) -> i32 {
        let fail = |io: &mut Io, subject: Option<&str>, problem: &str, hints: &[&str]| {
            io.err
                .push_str(&zoraya_command::error("install", subject, problem, hints));
            1
        };
        if let Some(why) = self.apps_refusal() {
            return fail(io, None, &why, &[]);
        }
        let keep = p.flag(d, "keep");
        let Some(package) = p.operand(d, "PACKAGE") else {
            return fail(io, None, "no package named", &["install PACKAGE [WORD...]"]);
        };
        let values: Vec<Value> = io
            .words
            .iter()
            .filter(|v| !(v.literal && (v.text == "--keep" || v.text == "-k")))
            .cloned()
            .collect();
        let at = values
            .iter()
            .position(|v| v.text == package)
            .unwrap_or(values.len());
        let tokens = match plan::literal_tokens(values.get(at + 1..).unwrap_or(&[])) {
            Ok(t) => t,
            Err(e) => return fail(io, None, &e, &[]),
        };
        let words = match apps::parse_words(&tokens) {
            Ok(w) => self.unexpand(w),
            Err(e) => return fail(io, None, &e, &[]),
        };
        if let Err(e) = self.check_saved(&words) {
            return fail(io, None, &e, &[]);
        }
        // The package: a directory with one command.
        let dir_abs = self.abs(package);
        let Ok(dir) = self.open(&dir_abs, OPEN_DIRECTORY) else {
            return fail(io, Some(package), "not a directory", &[]);
        };
        let mut listing: Vec<(String, u32)> = Vec::new();
        if let Err(s) = dir.list(&mut |n, k| listing.push((n.into(), k))) {
            return fail(io, Some(package), &problem(s), &[]);
        }
        let cmds: Vec<&str> = listing
            .iter()
            .filter(|(n, k)| *k == proto_kind::FILE && n.ends_with(".cmd"))
            .map(|(n, _)| n.trim_end_matches(".cmd"))
            .collect();
        let name = match cmds.as_slice() {
            [one] => (*one).to_string(),
            [] => {
                return fail(
                    io,
                    Some(package),
                    "no NAME.cmd: a package has the program, its description NAME.cmd and its files",
                    &[],
                );
            }
            _ => {
                return fail(
                    io,
                    Some(package),
                    "more than one command: a package has one for now (ADR-0396 item 1)",
                    &[],
                );
            }
        };
        let read_part = |file: &str, cap: usize| -> Option<Vec<u8>> {
            let f = self.open(&format!("{dir_abs}/{file}"), 0).ok()?;
            let b = read_all(&f).ok()?;
            (b.len() <= cap).then_some(b)
        };
        let program_head = self
            .open(&format!("{dir_abs}/{name}"), 0)
            .ok()
            .and_then(|f| {
                let mut head = [0u8; 4];
                let n = f.read(0, &mut head).ok()?;
                Some(head[..n].to_vec())
            });
        let cmd = read_part(&format!("{name}.cmd"), apps::MAX_PACKAGE_TEXT);
        let request = read_part("request", apps::MAX_PACKAGE_TEXT);
        let record_bytes = self.apps_bytes();
        let record = apps::read(&record_bytes);
        let existing = record.get(&name).cloned();
        let stray =
            existing.is_none() && self.open(&format!("/apps/{name}"), OPEN_DIRECTORY).is_ok();
        let taken = if existing.is_some() {
            None
        } else {
            self.name_taken(state, &name)
        };
        let checked = apps::check_package(
            &apps::Package {
                name: &name,
                program_head: program_head.as_deref(),
                description: cmd.as_deref(),
                request: request.as_deref(),
            },
            &|_| taken,
            stray,
        );
        let checked = match checked {
            Ok(c) => c,
            Err(r) => return fail(io, None, &r.text(&name), &[]),
        };
        let cmd = cmd.unwrap_or_default();
        // Copy beside the old, then put it in place: a package that fails
        // half way leaves the old one.
        let apps_dir = match self.open("/apps", OPEN_DIRECTORY) {
            Ok(d) => d,
            Err(_) => match self
                .open("/", OPEN_DIRECTORY)
                .and_then(|r| r.make_directory("apps"))
                .and_then(|()| self.open("/apps", OPEN_DIRECTORY))
            {
                Ok(d) => d,
                Err(s) => return fail(io, Some("/apps"), &problem(s), &[]),
            },
        };
        let building = format!(".new-{name}");
        remove_tree(&apps_dir, &building);
        let outcome = (|| -> Result<(Copied, sha256::Digest), String> {
            apps_dir.make_directory(&building).map_err(problem)?;
            let to = apps_dir
                .open(&building, OPEN_DIRECTORY, ALL)
                .map_err(problem)?;
            let from = self.open(&dir_abs, OPEN_DIRECTORY).map_err(problem)?;
            let length = from
                .open(&name, 0, file_rights::READ)
                .and_then(|f| f.stat())
                .map_err(problem)?
                .size;
            let mut hasher = Sha256::new();
            hasher.update(&length.to_le_bytes());
            let mut id = Identity {
                name: &name,
                length,
                hasher,
            };
            let mut copied = Copied::default();
            self.copy_tree(&from, &to, 0, &mut copied, Some(&mut id), &cmd)?;
            if id.length != copied_program(&to, &name)? {
                return Err(String::from("the program changed while it was copied"));
            }
            let digest = id.hasher.finish();
            Ok((copied, digest))
        })();
        let (copied, digest) = match outcome {
            Ok(x) => x,
            Err(e) => {
                remove_tree(&apps_dir, &building);
                return fail(io, Some(package), &e, &[]);
            }
        };
        // The old package is put aside, not removed, until the new one is in
        // place: a failed rename puts it back (ADR-0394 item 4).
        let aside = format!(".old-{name}");
        // A stop between the two renames of an earlier upgrade left the old
        // package aside and none in place: put it back first.
        if existing.is_some()
            && self.open(&format!("/apps/{name}"), OPEN_DIRECTORY).is_err()
            && self.open(&format!("/apps/{aside}"), OPEN_DIRECTORY).is_ok()
        {
            let _ = apps_dir.rename(&aside, &apps_dir, &name);
        }
        remove_tree(&apps_dir, &aside);
        if existing.is_some()
            && let Err(s) = apps_dir.rename(&name, &apps_dir, &aside)
        {
            remove_tree(&apps_dir, &building);
            return fail(io, Some(&name), &problem(s), &[]);
        }
        if let Err(s) = apps_dir.rename(&building, &apps_dir, &name) {
            if existing.is_some() {
                let _ = apps_dir.rename(&aside, &apps_dir, &name);
            }
            remove_tree(&apps_dir, &building);
            return fail(io, Some(&name), &problem(s), &[]);
        }
        remove_tree(&apps_dir, &aside);
        // The record.
        let (date, _) = self.wall_time();
        let mut saved = words.clone();
        let mut dropped: Vec<Word> = Vec::new();
        let mut kept: Vec<Word> = Vec::new();
        if let Some(old) = &existing {
            if keep {
                for w in &old.words {
                    if !saved.contains(w) {
                        saved.insert(0, w.clone());
                    }
                }
                kept = old.words.clone();
            } else {
                dropped = old.words.clone();
            }
        }
        saved.truncate(apps::MAX_WORDS);
        let app = App {
            name: name.clone(),
            hash: digest,
            installed: date,
            words: saved,
        };
        let bytes = match apps::put(&record_bytes, &app) {
            Ok(b) => b,
            Err(_) => {
                return fail(
                    io,
                    Some(&name),
                    "the record is full: programs remove one",
                    &[],
                );
            }
        };
        if let Err(e) = self.publish_file(apps::RECORD, &bytes) {
            return fail(io, Some(apps::RECORD), &one_line(&e), &[]);
        }
        let kinds: Vec<&str> = app.words.iter().map(Word::token).collect();
        self.apps_log(
            io,
            "install",
            &name,
            format!("sha256:{} {}", app.short_hash(), kinds.join(" ")).trim_end(),
        );
        // What the person is told (ADR-0396 item 3, ADR-0397 item 8).
        let home = self.home();
        let size = copied.bytes.div_ceil(1024);
        io.out.push_str(&format!(
            "{name} (sha256 {}): {} file{}, {size} KiB, command {name}\n",
            app.short_hash(),
            copied.files,
            if copied.files == 1 { "" } else { "s" },
        ));
        if let Some(old) = &existing {
            io.out
                .push_str(&format!("replaces {name} (sha256 {})\n", old.short_hash()));
        }
        if !checked.asked.is_empty() {
            let asked: Vec<String> = checked.asked.iter().map(Word::words).collect();
            io.out
                .push_str(&format!("it asks for:   {}\n", asked.join("    ")));
        }
        if !dropped.is_empty() {
            let d: Vec<String> = dropped.iter().map(Word::words).collect();
            io.out.push_str(&format!(
                "dropped the saved grants of the old package: {} (--keep keeps them)\n",
                d.join(", ")
            ));
        }
        if !kept.is_empty() {
            let k: Vec<String> = kept.iter().map(Word::words).collect();
            io.out
                .push_str(&format!("kept the saved grants: {}\n", k.join(", ")));
        }
        if app.words.is_empty() {
            io.out.push_str("installed with no grants.");
            if checked.asked.is_empty() {
                io.out.push('\n');
            } else {
                let asked: Vec<String> = checked.asked.iter().map(Word::words).collect();
                io.out.push_str(&format!(
                    " to give what it asks:\n  install {package} {}\n",
                    asked.join(" ")
                ));
            }
        } else {
            io.out.push_str(&format!("{name} installed with:\n"));
            for w in &app.words {
                io.out.push_str(&format!("  {}\n", apps::meaning(w, &home)));
            }
        }
        let _ = state;
        0
    }

    /// Copies the entries of `from` into `to`, hashing the program file.
    fn copy_tree(
        &self,
        from: &File,
        to: &File,
        depth: usize,
        copied: &mut Copied,
        mut id: Option<&mut Identity<'_>>,
        cmd: &[u8],
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("more than {MAX_DEPTH} levels of directories"));
        }
        let mut names: Vec<(String, u32)> = Vec::new();
        from.list(&mut |n, k| names.push((n.into(), k)))
            .map_err(problem)?;
        names.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        for (name, kind) in names {
            copied.entries += 1;
            if copied.entries > MAX_ENTRIES {
                return Err(format!("more than {MAX_ENTRIES} entries"));
            }
            match kind {
                proto_kind::DIRECTORY => {
                    to.make_directory(&name).map_err(problem)?;
                    let sub_from = from.open(&name, OPEN_DIRECTORY, ALL).map_err(problem)?;
                    let sub_to = to.open(&name, OPEN_DIRECTORY, ALL).map_err(problem)?;
                    self.copy_tree(&sub_from, &sub_to, depth + 1, copied, None, cmd)?;
                }
                proto_kind::FILE => {
                    let src = from.open(&name, 0, file_rights::READ).map_err(problem)?;
                    // The description is what was read for the check, so
                    // that the hash covers the bytes the lint saw.
                    let is_cmd = depth == 0
                        && id
                            .as_deref()
                            .is_some_and(|i| format!("{}.cmd", i.name) == name);
                    if is_cmd {
                        write_new(to, &name, cmd)?;
                        copied.bytes += cmd.len() as u64;
                    } else {
                        let hasher = match id.as_deref_mut() {
                            Some(i) if depth == 0 && i.name == name => Some(&mut i.hasher),
                            _ => None,
                        };
                        let n = copy_file(&src, to, &name, hasher, copied.bytes)?;
                        copied.bytes += n;
                    }
                    if copied.bytes > MAX_BYTES {
                        return Err(String::from("the package is larger than 256 MiB"));
                    }
                    copied.files += 1;
                    // After the program, the description's length and bytes.
                    if depth == 0
                        && let Some(i) = id.as_deref_mut()
                        && i.name == name
                    {
                        i.hasher.update(&(cmd.len() as u64).to_le_bytes());
                        i.hasher.update(cmd);
                    }
                }
                _ => {
                    return Err(format!(
                        "{name}: a link in a package (a package holds files and directories)"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The length of the program as it was written.
fn copied_program(to: &File, name: &str) -> Result<u64, String> {
    to.open(name, 0, file_rights::READ)
        .and_then(|f| f.stat())
        .map(|s| s.size)
        .map_err(problem)
}

/// A new file `name` in `dir` with `bytes`, published whole.
fn write_new(dir: &File, name: &str, bytes: &[u8]) -> Result<(), String> {
    let file = dir.create_unnamed().map_err(problem)?;
    let mut at = 0u64;
    for chunk in bytes.chunks(zoraya_proto_file::MAX_INLINE) {
        file.write(at, 0, chunk).map_err(problem)?;
        at += chunk.len() as u64;
    }
    file.publish(dir, name).map_err(problem)
}

/// Copies `src` to `name` in `dir`, feeding `hasher` the bytes written.
fn copy_file(
    src: &File,
    dir: &File,
    name: &str,
    mut hasher: Option<&mut Sha256>,
    _so_far: u64,
) -> Result<u64, String> {
    let file = dir.create_unnamed().map_err(problem)?;
    let mut buf = alloc::vec![0u8; zoraya_proto_file::MAX_INLINE];
    let mut at = 0u64;
    loop {
        let n = src.read(at, &mut buf).map_err(problem)?;
        if n == 0 {
            break;
        }
        file.write(at, 0, &buf[..n]).map_err(problem)?;
        if let Some(h) = hasher.as_deref_mut() {
            h.update(&buf[..n]);
        }
        at += n as u64;
        if at > MAX_BYTES {
            return Err(String::from("a file larger than 256 MiB"));
        }
    }
    file.publish(dir, name).map_err(problem)?;
    Ok(at)
}

// --- Looking at what is installed, and the check of a launch ---

impl Z {
    /// Whether the files of `app` are the ones installed: the program's
    /// bytes and its description, hashed as the record's identity (ADR-0396
    /// item 4). `None` when they cannot be read.
    fn installed_files(&self, app: &App) -> Option<(sha256::Digest, Vec<u8>)> {
        let name = &app.name;
        let cmd = self
            .open(&format!("/apps/{name}/{name}.cmd"), 0)
            .and_then(|f| read_all(&f))
            .ok()
            .filter(|b| b.len() <= apps::MAX_PACKAGE_TEXT)?;
        let file = self.open(&format!("/apps/{name}/{name}"), 0).ok()?;
        let length = file.stat().ok()?.size;
        let mut h = Sha256::new();
        h.update(&length.to_le_bytes());
        let mut buf = alloc::vec![0u8; zoraya_proto_file::MAX_INLINE];
        let mut at = 0u64;
        while at < length {
            let n = file.read(at, &mut buf).ok()?;
            if n == 0 {
                return None;
            }
            h.update(&buf[..n.min((length - at) as usize)]);
            at += n as u64;
        }
        h.update(&(cmd.len() as u64).to_le_bytes());
        h.update(&cmd);
        Some((h.finish(), cmd))
    }

    /// The description of an installed program, read for `help`,
    /// completion and usage (ADR-0396 items 5 and 6): only while the files
    /// are the installed ones, and as the volume reading of it (every kind
    /// `text`). Checked once a session for a record, and again whenever the
    /// record changes.
    pub(super) fn installed_description(&mut self, name: &str) -> Option<Rc<Description>> {
        let app = self.read_apps().get(name).cloned()?;
        if let Some((hash, d)) = self.installed_cache.get(name)
            && *hash == app.hash
        {
            return Some(d.clone());
        }
        let (hash, cmd) = self.installed_files(&app)?;
        if hash != app.hash {
            return None;
        }
        let d = zoraya_describe::parse(&cmd).ok()?;
        if d.name != app.name {
            return None;
        }
        let d = Rc::new(zoraya_describe::for_volume(d));
        self.installed_cache
            .insert(name.into(), (app.hash, d.clone()));
        Some(d)
    }

    /// The page of `help NAME` for an installed program, or why there is
    /// none (`changed since installed`).
    pub(super) fn installed_page(
        &mut self,
        name: &str,
    ) -> Option<Result<(Rc<Description>, String), String>> {
        if !apps::is_app_name(name) {
            return None;
        }
        let app = self.read_apps().get(name).cloned()?;
        let first = apps::help_head(&app);
        // Always checked here: a page is shown only for the installed files.
        self.installed_cache.remove(name);
        Some(match self.installed_description(name) {
            Some(d) => Ok((d, first)),
            None => Err(format!(
                "{name} is an installed program, but its files changed since installed"
            )),
        })
    }

    /// The names of the installed programs, with a summary from the cache
    /// when the description was read this session.
    pub(super) fn installed_listing(&mut self) -> Vec<(String, String)> {
        self.read_apps()
            .apps
            .iter()
            .map(|a| {
                let summary = self
                    .installed_cache
                    .get(&a.name)
                    .filter(|(h, _)| *h == a.hash)
                    .map(|(_, d)| one_line(&tamed(d.summary.as_bytes())))
                    .unwrap_or_else(|| String::from("an installed program"));
                (a.name.clone(), summary)
            })
            .collect()
    }

    /// The saved words of `name` as a launch applies them, or `None` when
    /// the name is not an installed one this lookup found.
    pub(super) fn saved(&self, name: &str) -> Option<plan::Applied> {
        let app = self.installed_now.get(name)?;
        let path =
            |abs: &str, write: bool| self.root.as_ref().is_some_and(|h| h.covers(abs, write));
        let home = self.home();
        let room_nets = self.room.as_ref().map(|r| r.nets.as_slice());
        Some(plan::apply(
            name,
            &app.words,
            &plan::Held {
                home: &home,
                room: self.room.as_ref().map(|r| r.name.as_str()),
                untrusted: self.room.as_ref().is_some_and(|r| r.untrusted),
                room_nets,
                path: &path,
                network: self.net.is_some(),
            },
        ))
    }

    /// An installed program's plan beyond the line's: its package read
    /// only (when this shell holds it) and the saved `+r` and `+w` words.
    pub(super) fn add_saved(&self, name: &str, plan_out: &mut Vec<grants::Planned>) {
        if !self.installed_now.contains_key(name) {
            return;
        }
        let package = format!("/apps/{name}");
        if self
            .root
            .as_ref()
            .is_some_and(|h| h.covers(&package, false))
        {
            plan_out.push(grants::Planned {
                path: package,
                access: grants::Access::Read,
                reason: String::from("its package"),
            });
        }
        if let Some(s) = self.saved(name) {
            plan_out.extend(s.planned);
        }
    }

    /// Finds `name` among the installed programs for a lookup: its program
    /// file's path, remembered with its record until the next lookup.
    pub(super) fn installed_find(&mut self, name: &str) -> Option<String> {
        if !apps::is_app_name(name) || self.root.is_none() {
            return None;
        }
        let app = self.read_apps().get(name).cloned()?;
        let path = format!("/apps/{name}/{name}");
        self.open(&path, 0).ok()?;
        self.installed_now.insert(name.into(), app);
        Some(path)
    }

    /// The jobs' programs, for `programs`' counts and `--kill`.
    fn app_instances(&self) -> Vec<apps::Instance> {
        self.jobs
            .iter()
            .flat_map(|e| {
                e.payload
                    .elements
                    .iter()
                    .filter(|(n, _, _)| self.live_apps.contains(n))
                    .map(|(n, _, _)| apps::Instance {
                        job: e.number,
                        program: n.clone(),
                        here: true,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

// --- programs ---

impl Z {
    pub(super) fn programs_builtin(&mut self, d: &Description, p: &Parsed<'_>, io: &mut Io) -> i32 {
        let fail = |io: &mut Io, subject: Option<&str>, problem: &str, hints: &[&str]| {
            io.err
                .push_str(&zoraya_command::error("programs", subject, problem, hints));
            1
        };
        if let Some(why) = self.apps_refusal() {
            return fail(io, None, &why, &[]);
        }
        self.refresh_states();
        let names: Vec<String> = self
            .read_apps()
            .apps
            .iter()
            .map(|a| a.name.clone())
            .collect();
        self.live_apps = names
            .into_iter()
            .filter(|n| {
                Host::lookup(self, n) == Found::Program && self.installed_now.contains_key(n)
            })
            .collect();
        let (all, kill, data, log) = (
            p.flag(d, "all"),
            p.flag(d, "kill"),
            p.flag(d, "data"),
            p.flag(d, "log"),
        );
        let flag_words = ["--all", "--kill", "--data", "--log"];
        let values: Vec<Value> = io
            .words
            .iter()
            .filter(|v| !(v.literal && flag_words.contains(&v.text.as_str())))
            .cloned()
            .collect();
        let record_bytes = self.apps_bytes();
        let record = apps::read(&record_bytes);
        for pr in &record.problems {
            io.err.push_str(&format!(
                "zosh: {} line {}: {}\n",
                apps::RECORD,
                pr.line,
                pr.problem
            ));
        }
        let home = self.home();
        let one_instances = |z: &Z, name: &str| apps::instances_of(&z.app_instances(), name);
        if log {
            let name = values.first().map(|v| v.text.as_str());
            let text = self
                .open(apps::LOG, 0)
                .and_then(|f| read_all(&f))
                .unwrap_or_default();
            let view = apps::log_view(&text, name);
            io.out.push_str(if view.is_empty() {
                "nothing logged\n"
            } else {
                &view
            });
            return 0;
        }
        let Some(first) = values.first().map(|v| v.text.clone()) else {
            // The table.
            let instances = self.app_instances();
            let running = |n: &str| instances.iter().filter(|i| i.program == n).count();
            let changed: Vec<String> = record
                .apps
                .iter()
                .filter(|a| self.installed_files(a).is_none_or(|(h, _)| h != a.hash))
                .map(|a| a.name.clone())
                .collect();
            io.out.push_str(&apps::table(&record, &running, &|n| {
                changed.iter().any(|c| c == n)
            }));
            return 0;
        };
        match first.as_str() {
            "grant" | "revoke" | "remove" => {}
            name => return self.programs_page(&record, name, &home, io),
        }
        let Some(name) = values.get(1).map(|v| v.text.clone()) else {
            return fail(io, None, &format!("{first} needs a program name"), &[]);
        };
        let rest = &values[2..];
        match first.as_str() {
            "grant" => {
                let Some(app) = record.get(&name).cloned() else {
                    return fail(io, Some(&name), "not an installed program", &[]);
                };
                let tokens = match plan::literal_tokens(rest) {
                    Ok(t) => t,
                    Err(e) => return fail(io, None, &e, &[]),
                };
                let words = match apps::parse_words(&tokens) {
                    Ok(w) if !w.is_empty() => self.unexpand(w),
                    Ok(_) => return fail(io, None, "no word to save", &[]),
                    Err(e) => return fail(io, None, &e, &[]),
                };
                if let Err(e) = self.check_saved(&words) {
                    return fail(io, None, &e, &[]);
                }
                let mut app = app;
                for w in &words {
                    if !app.words.contains(w) {
                        app.words.push(w.clone());
                    }
                }
                let bytes = match apps::put(&record_bytes, &app) {
                    Ok(b) => b,
                    Err(_) => {
                        return fail(
                            io,
                            Some(&name),
                            &format!("more than {} saved words", apps::MAX_WORDS),
                            &[],
                        );
                    }
                };
                if let Err(e) = self.publish_file(apps::RECORD, &bytes) {
                    return fail(io, Some(apps::RECORD), &one_line(&e), &[]);
                }
                let shown: Vec<String> = words.iter().map(Word::words).collect();
                self.apps_log(io, "grant", &name, &shown.join(" "));
                io.out.push_str(&format!("{name} gets, saved:\n"));
                for w in &words {
                    io.out.push_str(&format!("  {}\n", apps::meaning(w, &home)));
                }
                0
            }
            "revoke" => {
                if record.get(&name).is_none() {
                    return fail(io, Some(&name), "not an installed program", &[]);
                }
                let tokens = match plan::literal_tokens(rest) {
                    Ok(t) => t,
                    Err(e) => return fail(io, None, &e, &[]),
                };
                let result = if all {
                    if !tokens.is_empty() {
                        return fail(io, None, "--all takes no words", &[]);
                    }
                    apps::revoke_all(&record_bytes, &name)
                } else if tokens.is_empty() && kill {
                    // `programs revoke --kill NAME`: end the copies; the record
                    // is untouched.
                    Ok((record_bytes.clone(), Vec::new()))
                } else {
                    match apps::parse_words(&tokens) {
                        Ok(w) if !w.is_empty() => {
                            apps::revoke(&record_bytes, &name, &self.unexpand(w))
                        }
                        Ok(_) => return fail(io, None, "no word to revoke (or --all)", &[]),
                        Err(e) => return fail(io, None, &e, &[]),
                    }
                };
                let (bytes, gone) = match result {
                    Ok(x) => x,
                    Err(apps::Refusal::NoSuchApp) => {
                        return fail(io, Some(&name), "not an installed program", &[]);
                    }
                    Err(apps::Refusal::NotSaved(w)) => {
                        return fail(
                            io,
                            Some(&name),
                            &format!("{w} is not saved for it"),
                            &["programs NAME shows what is"],
                        );
                    }
                    Err(_) => return fail(io, Some(&name), "the record cannot be changed", &[]),
                };
                if !gone.is_empty() {
                    if let Err(e) = self.publish_file(apps::RECORD, &bytes) {
                        return fail(io, Some(apps::RECORD), &one_line(&e), &[]);
                    }
                    let shown: Vec<String> = gone.iter().map(Word::words).collect();
                    self.apps_log(io, "revoke", &name, &shown.join(" "));
                }
                let (jobs_here, _) = one_instances(self, &name);
                let copies = self
                    .app_instances()
                    .iter()
                    .filter(|i| i.program == name)
                    .count();
                let shown: Vec<String> = gone.iter().map(Word::words).collect();
                let removed = if shown.is_empty() {
                    String::from("nothing removed")
                } else {
                    format!("{} removed", shown.join(", "))
                };
                if copies == 0 {
                    io.out
                        .push_str(&format!("{name}: {removed}; no copy running.\n"));
                } else if kill {
                    io.out.push_str(&format!(
                        "{name}: {removed}; ending {copies} running cop{} ({} job{}).\n",
                        if copies == 1 { "y" } else { "ies" },
                        jobs_here.len(),
                        if jobs_here.len() == 1 { "" } else { "s" },
                    ));
                    show(&core::mem::take(&mut io.out));
                    for n in jobs_here {
                        let job = self.jobs.get(n).map(|e| e.payload.job);
                        if let Some(job) = job {
                            let _ = job_kill(job);
                            show(&self.jobs.take_notices());
                            self.wait_for(Some(n), clock_monotonic() + 2_000_000_000);
                        }
                    }
                    io.out.push_str(
                        "programs of other rooms' shells are not reachable from here and are not touched.\n",
                    );
                } else {
                    io.out.push_str(&format!(
                        "{name}: {removed}; {copies} running cop{} keep{} access until {} exit{}; programs revoke --kill {name} stops {}.\n",
                        if copies == 1 { "y" } else { "ies" },
                        if copies == 1 { "s" } else { "" },
                        if copies == 1 { "it" } else { "they" },
                        if copies == 1 { "s" } else { "" },
                        if copies == 1 { "it" } else { "them" },
                    ));
                }
                0
            }
            _ => self.programs_remove(&record, &record_bytes, &name, data, io),
        }
    }

    /// `programs NAME`: the page (ADR-0399 item 1).
    fn programs_page(&mut self, record: &Apps, name: &str, home: &str, io: &mut Io) -> i32 {
        let Some(app) = record.get(name).cloned() else {
            io.err.push_str(&zoraya_command::error(
                "programs",
                Some(name),
                "not an installed program",
                &["programs lists them"],
            ));
            return 1;
        };
        io.out.push_str(&apps::help_head(&app));
        io.out.push('\n');
        let files = self.installed_files(&app);
        let changed = files.as_ref().is_none_or(|(h, _)| *h != app.hash);
        match self.installed_description(name) {
            Some(d) => io
                .out
                .push_str(&format!("  {}\n", one_line(&tamed(d.summary.as_bytes())))),
            None => io.out.push_str(
                "  changed since installed: its files are not the ones installed, and it does not start\n",
            ),
        }
        let _ = changed;
        // The files of the package.
        if let Ok(dir) = self.open(&format!("/apps/{name}"), OPEN_DIRECTORY) {
            let mut list: Vec<(String, u32)> = Vec::new();
            let _ = dir.list(&mut |n, k| list.push((n.into(), k)));
            list.sort();
            io.out.push_str("files:\n");
            for (n, k) in list {
                let size = dir
                    .open(
                        &n,
                        if k == proto_kind::DIRECTORY {
                            OPEN_DIRECTORY
                        } else {
                            0
                        },
                        file_rights::READ,
                    )
                    .and_then(|f| f.stat())
                    .map(|s| s.size)
                    .unwrap_or(0);
                let shown = tamed(n.as_bytes());
                if k == proto_kind::DIRECTORY {
                    io.out.push_str(&format!("  {shown}/\n"));
                } else {
                    io.out.push_str(&format!("  {shown}  {} bytes\n", size));
                }
            }
        }
        if app.words.is_empty() {
            io.out.push_str("saved grants: none\n");
        } else {
            io.out.push_str("saved grants:\n");
            for w in &app.words {
                io.out.push_str(&format!(
                    "  {}  {} (saved)\n",
                    w.words(),
                    apps::meaning(w, home)
                ));
            }
        }
        let asked = self
            .open(&format!("/apps/{name}/request"), 0)
            .and_then(|f| read_all(&f))
            .map(|b| apps::request_words(&b))
            .unwrap_or_default();
        let missing: Vec<String> = asked
            .iter()
            .filter(|w| !app.words.contains(w))
            .map(Word::words)
            .collect();
        if !missing.is_empty() {
            io.out
                .push_str(&format!("asked, not given: {}\n", missing.join("  ")));
        }
        let jobs: Vec<u32> = apps::instances_of(&self.app_instances(), name).0;
        if jobs.is_empty() {
            io.out.push_str("running: none\n");
        } else {
            io.out.push_str("running:\n");
            for n in jobs {
                if let Some(e) = self.jobs.get(n) {
                    io.out.push_str(&format!(
                        "  [{n}] {}\n",
                        one_line(&tamed(e.line.as_bytes()))
                    ));
                }
            }
        }
        0
    }

    /// `programs remove NAME [--data]` (ADR-0396 item 9, ADR-0399 item 1).
    fn programs_remove(
        &mut self,
        record: &Apps,
        record_bytes: &[u8],
        name: &str,
        data: bool,
        io: &mut Io,
    ) -> i32 {
        let fail = |io: &mut Io, problem: &str, hints: &[&str]| {
            io.err.push_str(&zoraya_command::error(
                "programs",
                Some(name),
                problem,
                hints,
            ));
            1
        };
        let stray = self.open(&format!("/apps/{name}"), OPEN_DIRECTORY).is_ok();
        if record.get(name).is_none() && !(stray && apps::is_app_name(name)) {
            return fail(io, "not an installed program", &[]);
        }
        let copies = apps::instances_of(&self.app_instances(), name).0.len();
        if copies > 0 {
            return fail(
                io,
                &format!("{copies} job{} run it", if copies == 1 { "" } else { "s" }),
                &[&format!("programs revoke --kill {name} ends them")],
            );
        }
        // Its own files, in every room, only when asked (and asked again).
        let mut rooms: Vec<String> = Vec::new();
        if data && let Ok(dir) = self.open("/data", OPEN_DIRECTORY) {
            let mut list: Vec<(String, u32)> = Vec::new();
            let _ = dir.list(&mut |n, k| list.push((n.into(), k)));
            for (room, k) in list {
                if k == proto_kind::DIRECTORY
                    && self
                        .open(&format!("/data/{room}/{name}"), OPEN_DIRECTORY)
                        .is_ok()
                {
                    rooms.push(room);
                }
            }
        }
        if !rooms.is_empty() {
            show(&format!(
                "delete the files of {name} in {} room{}? (y/n) ",
                rooms.len(),
                if rooms.len() == 1 { "" } else { "s" }
            ));
            let key = self.wait_key();
            show("\n");
            if key != b'y' && key != b'Y' {
                io.err.push_str("programs: nothing removed\n");
                return 1;
            }
        }
        if let Ok(apps_dir) = self.open("/apps", OPEN_DIRECTORY) {
            remove_tree(&apps_dir, name);
        }
        for room in &rooms {
            if let Ok(dir) = self.open(&format!("/data/{room}"), OPEN_DIRECTORY) {
                remove_tree(&dir, name);
            }
        }
        if record.get(name).is_some() {
            match apps::remove(record_bytes, name) {
                Ok(bytes) => {
                    if let Err(e) = self.publish_file(apps::RECORD, &bytes) {
                        return fail(io, &one_line(&e), &[]);
                    }
                }
                Err(_) => return fail(io, "the record cannot be changed", &[]),
            }
        }
        self.installed_cache.remove(name);
        self.apps_log(
            io,
            "remove",
            name,
            if rooms.is_empty() { "" } else { "--data" },
        );
        io.out.push_str(&format!(
            "{name} removed{}\n",
            if data {
                if rooms.is_empty() {
                    ""
                } else {
                    " with its files"
                }
            } else {
                "; its own files under /data are kept"
            }
        ));
        0
    }
}
