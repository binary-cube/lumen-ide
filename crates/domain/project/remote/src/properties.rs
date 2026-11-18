//! A file's or a folder's properties on the server, as a file manager shows them (WinSCP's
//! "Properties"): its Unix permissions ([`Mode`]: read / write / execute for the owner, the
//! group and the others, set-UID, set-GID, sticky — as boxes or as octal), its owner and
//! group ([`Principal`]: a name, an id, or both), its size and date.
//!
//! Several chosen at once are edited together ([`ModeEdit`]): a permission they all share is
//! on or off, one they don't is mixed, left as each has it unless the user sets it.
//!
//! What the servers say is read here too, the same for every protocol: a listing's line
//! (`ls -l`: SFTP's long names, FTP's `LIST` — [`parse_ls_line`]), FTP's `MLST` / `MLSD`
//! facts ([`parse_facts`]), the accounts' files (`/etc/passwd`, `/etc/group` —
//! [`parse_accounts`]).

use std::collections::BTreeMap;

/// Who a permission is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Owner,
    Group,
    Others,
}

impl Class {
    pub const ALL: [Self; 3] = [Self::Owner, Self::Group, Self::Others];

    /// How far its three bits are shifted in a mode.
    fn shift(self) -> u32 {
        match self {
            Self::Owner => 6,
            Self::Group => 3,
            Self::Others => 0,
        }
    }
}

/// What a permission allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Access {
    Read,
    Write,
    Execute,
}

impl Access {
    pub const ALL: [Self; 3] = [Self::Read, Self::Write, Self::Execute];

    fn bit(self) -> u32 {
        match self {
            Self::Read => 0o4,
            Self::Write => 0o2,
            Self::Execute => 0o1,
        }
    }
}

/// The special bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Special {
    /// Runs as its owner.
    SetUid,
    /// Runs as its group; on a folder, what's made in it takes its group.
    SetGid,
    /// On a folder: only a file's owner may delete or rename it.
    Sticky,
}

impl Special {
    pub const ALL: [Self; 3] = [Self::SetUid, Self::SetGid, Self::Sticky];
}

/// One of a mode's twelve bits (one box of the grid).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bit {
    Access(Class, Access),
    Special(Special),
}

impl Bit {
    /// The grid's nine, then the three special ones.
    pub const ALL: [Self; 12] = [
        Self::Access(Class::Owner, Access::Read),
        Self::Access(Class::Owner, Access::Write),
        Self::Access(Class::Owner, Access::Execute),
        Self::Access(Class::Group, Access::Read),
        Self::Access(Class::Group, Access::Write),
        Self::Access(Class::Group, Access::Execute),
        Self::Access(Class::Others, Access::Read),
        Self::Access(Class::Others, Access::Write),
        Self::Access(Class::Others, Access::Execute),
        Self::Special(Special::SetUid),
        Self::Special(Special::SetGid),
        Self::Special(Special::Sticky),
    ];

    pub fn mask(self) -> u32 {
        match self {
            Self::Access(class, access) => access.bit() << class.shift(),
            Self::Special(Special::SetUid) => 0o4000,
            Self::Special(Special::SetGid) => 0o2000,
            Self::Special(Special::Sticky) => 0o1000,
        }
    }
}

/// A file's Unix permissions: the low twelve bits of its mode (its type left out).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Mode(u32);

impl Mode {
    /// The bits a mode keeps.
    pub const MASK: u32 = 0o7777;

    /// From a mode as a server gives it (its type's bits dropped).
    pub fn new(bits: u32) -> Self {
        Self(bits & Self::MASK)
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    pub fn has(self, bit: Bit) -> bool {
        self.0 & bit.mask() != 0
    }

    #[must_use]
    pub fn with(self, bit: Bit, on: bool) -> Self {
        if on { Self(self.0 | bit.mask()) } else { Self(self.0 & !bit.mask()) }
    }

    /// Four octal digits (`0755`, `2775`), as `chmod` takes them.
    pub fn octal(self) -> String {
        format!("{:04o}", self.0)
    }

    /// Three or four octal digits (`755`, `0644`, `4755`); anything else: `None`.
    pub fn parse_octal(text: &str) -> Option<Self> {
        let text = text.trim();
        let valid =
            (3..=4).contains(&text.len()) && text.bytes().all(|b| (b'0'..=b'7').contains(&b));
        valid.then(|| u32::from_str_radix(text, 8).ok()).flatten().map(Self::new)
    }

    /// As `ls -l` writes it after the type (`rwxr-sr-t`).
    pub fn symbolic(self) -> String {
        let mut out = String::with_capacity(9);
        for class in Class::ALL {
            for access in Access::ALL {
                let on = self.has(Bit::Access(class, access));
                let special = match (class, access) {
                    (Class::Owner, Access::Execute) => Some((Special::SetUid, 's')),
                    (Class::Group, Access::Execute) => Some((Special::SetGid, 's')),
                    (Class::Others, Access::Execute) => Some((Special::Sticky, 't')),
                    _ => None,
                };
                let c = match special {
                    Some((s, c)) if self.has(Bit::Special(s)) => {
                        if on {
                            c
                        } else {
                            c.to_ascii_uppercase()
                        }
                    }
                    _ if !on => '-',
                    _ => match access {
                        Access::Read => 'r',
                        Access::Write => 'w',
                        Access::Execute => 'x',
                    },
                };
                out.push(c);
            }
        }
        out
    }

    /// From `ls -l`'s nine letters (`rwxr-xr-x`, `rwsr-Sr-T`); anything else: `None`.
    pub fn parse_symbolic(text: &str) -> Option<Self> {
        let letters: Vec<char> = text.chars().take(9).collect();
        if letters.len() != 9 {
            return None;
        }
        let mut mode = Self::default();
        for (i, c) in letters.into_iter().enumerate() {
            let class = Class::ALL[i / 3];
            let access = Access::ALL[i % 3];
            let expected = ['r', 'w', 'x'][i % 3];
            let special = [Special::SetUid, Special::SetGid, Special::Sticky][i / 3];
            let special_letter = if class == Class::Others { 't' } else { 's' };
            match c {
                '-' => {}
                c if c == expected => mode = mode.with(Bit::Access(class, access), true),
                c if access == Access::Execute && c == special_letter => {
                    mode = mode.with(Bit::Access(class, access), true);
                    mode = mode.with(Bit::Special(special), true);
                }
                c if access == Access::Execute && c == special_letter.to_ascii_uppercase() => {
                    mode = mode.with(Bit::Special(special), true);
                }
                _ => return None,
            }
        }
        Some(mode)
    }

    /// On a folder, "X": executable (searchable) by whoever may read it — what applying a
    /// file's permissions to its folders recursively needs (`0644` → `0755` on folders).
    #[must_use]
    pub fn with_x_where_readable(self) -> Self {
        Class::ALL.into_iter().fold(self, |mode, class| {
            if mode.has(Bit::Access(class, Access::Read)) {
                mode.with(Bit::Access(class, Access::Execute), true)
            } else {
                mode
            }
        })
    }
}

/// A box's state: what all chosen have, or mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    Off,
    On,
    Mixed,
}

/// The permissions of one or several files, edited together: each bit on, off, or mixed (the
/// files differ, each keeps its own unless the bit is set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeEdit {
    on: u32,
    off: u32,
    /// The bits mixed at first (they may go back to mixed).
    mixed_at_first: u32,
    first: (u32, u32),
}

impl ModeEdit {
    /// What `modes` share (empty: all off).
    pub fn of(modes: impl IntoIterator<Item = Mode>) -> Self {
        let (mut on, mut off) = (Mode::MASK, Mode::MASK);
        let mut any = false;
        for mode in modes {
            any = true;
            on &= mode.0;
            off &= !mode.0 & Mode::MASK;
        }
        if !any {
            (on, off) = (0, Mode::MASK);
        }
        let mixed = Mode::MASK & !(on | off);
        Self { on, off, mixed_at_first: mixed, first: (on, off) }
    }

    pub fn state(&self, bit: Bit) -> Tri {
        let mask = bit.mask();
        if self.on & mask != 0 {
            Tri::On
        } else if self.off & mask != 0 {
            Tri::Off
        } else {
            Tri::Mixed
        }
    }

    /// A click on a box: on → off → (mixed, if it was at first) → on.
    pub fn toggle(&mut self, bit: Bit) {
        let mask = bit.mask();
        let next = match self.state(bit) {
            Tri::On => Tri::Off,
            Tri::Off if self.mixed_at_first & mask != 0 => Tri::Mixed,
            Tri::Off | Tri::Mixed => Tri::On,
        };
        self.set(bit, next);
    }

    pub fn set(&mut self, bit: Bit, state: Tri) {
        let mask = bit.mask();
        self.on &= !mask;
        self.off &= !mask;
        match state {
            Tri::On => self.on |= mask,
            Tri::Off => self.off |= mask,
            Tri::Mixed => {}
        }
    }

    /// The octal digits when no bit is mixed.
    pub fn octal(&self) -> Option<String> {
        self.whole().map(Mode::octal)
    }

    /// The mode, when no bit is mixed.
    pub fn whole(&self) -> Option<Mode> {
        (self.on | self.off == Mode::MASK).then_some(Mode(self.on))
    }

    /// Typed octal digits set every bit; `false`: not a mode (nothing changed).
    pub fn set_octal(&mut self, text: &str) -> bool {
        let Some(mode) = Mode::parse_octal(text) else { return false };
        self.on = mode.0;
        self.off = !mode.0 & Mode::MASK;
        true
    }

    /// `mode` with what's set here (the mixed bits as it has them).
    pub fn apply(&self, mode: Mode) -> Mode {
        Mode((mode.0 | self.on) & !self.off & Mode::MASK)
    }

    /// Something was changed since it was made.
    pub fn is_changed(&self) -> bool {
        (self.on, self.off) != self.first
    }
}

/// A user or a group on the server: its name, its id, or both.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Principal {
    pub name: Option<String>,
    pub id: Option<u32>,
}

impl Principal {
    pub fn named(name: impl Into<String>, id: Option<u32>) -> Self {
        Self { name: Some(name.into()), id }
    }

    pub fn id(id: u32) -> Self {
        Self { name: None, id: Some(id) }
    }

    /// From a listing's word: an id when it's a number, else a name.
    pub fn from_word(word: &str) -> Self {
        match word.parse() {
            Ok(id) => Self::id(id),
            Err(_) => Self::named(word, None),
        }
    }

    /// The same one: the same id, or (without ids) the same name.
    pub fn same(&self, other: &Self) -> bool {
        match (self.id, other.id) {
            (Some(a), Some(b)) => a == b,
            _ => self.name.is_some() && self.name == other.name,
        }
    }

    /// What `other` knows that this doesn't (a name to an id, an id to a name).
    #[must_use]
    pub fn completed(mut self, other: &Self) -> Self {
        if self.name.is_none() {
            self.name.clone_from(&other.name);
        }
        if self.id.is_none() {
            self.id = other.id;
        }
        self
    }
}

impl std::fmt::Display for Principal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.name, self.id) {
            (Some(name), Some(id)) => write!(f, "{name} [{id}]"),
            (Some(name), None) => f.write_str(name),
            (None, Some(id)) => write!(f, "[{id}]"),
            (None, None) => f.write_str("?"),
        }
    }
}

/// What the server says of a file or a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    /// Relative to the project's root, with `/` (`""`: the root).
    pub path: String,
    pub is_dir: bool,
    /// Bytes (a folder's own entry: what the server says, not its contents').
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: i64,
    /// `None`: the server doesn't say (a Windows FTP server).
    pub mode: Option<Mode>,
    pub owner: Option<Principal>,
    pub group: Option<Principal>,
}

impl Properties {
    /// Only what any listing says (no permissions, no owner).
    pub fn of(entry: &crate::RemoteEntry) -> Self {
        Self {
            path: entry.path.clone(),
            is_dir: entry.is_dir,
            size: entry.size,
            modified: entry.modified,
            mode: None,
            owner: None,
            group: None,
        }
    }

    /// The names (or ids) a listing's line gives, added to what's known.
    #[must_use]
    pub fn with_listed(mut self, owner: &Principal, group: &Principal) -> Self {
        self.owner = Some(self.owner.take().map_or_else(|| owner.clone(), |o| o.completed(owner)));
        self.group = Some(self.group.take().map_or_else(|| group.clone(), |g| g.completed(group)));
        self
    }
}

/// The users and groups a server has, to choose an owner or a group from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Accounts {
    pub users: Vec<Principal>,
    pub groups: Vec<Principal>,
}

impl Accounts {
    /// `seen` added (those the files have, which the lists may lack); sorted by name.
    pub fn with_seen<'a>(
        mut self,
        users: impl IntoIterator<Item = &'a Principal>,
        groups: impl IntoIterator<Item = &'a Principal>,
    ) -> Self {
        fn merge<'a>(list: &mut Vec<Principal>, seen: impl IntoIterator<Item = &'a Principal>) {
            for p in seen {
                match list.iter_mut().find(|known| known.same(p)) {
                    Some(known) => *known = known.clone().completed(p),
                    None => list.push(p.clone()),
                }
            }
            list.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
        }
        merge(&mut self.users, users);
        merge(&mut self.groups, groups);
        self
    }

    /// `props` with its owner's and group's names (or ids) as the accounts know them.
    #[must_use]
    pub fn naming(&self, mut props: Properties) -> Properties {
        let complete = |p: Option<Principal>, list: &[Principal]| {
            p.map(|p| match list.iter().find(|known| known.same(&p)) {
                Some(known) => p.completed(known),
                None => p,
            })
        };
        props.owner = complete(props.owner, &self.users);
        props.group = complete(props.group, &self.groups);
        props
    }
}

/// What's set on one file: its mode, its owner's id, its group's id (each `None`: left as
/// it is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SetProperties {
    pub mode: Option<Mode>,
    pub owner: Option<u32>,
    pub group: Option<u32>,
}

impl SetProperties {
    pub fn is_empty(&self) -> bool {
        self.mode.is_none() && self.owner.is_none() && self.group.is_none()
    }
}

/// What the user changed in the dialog, applied to the chosen (and, recursively, to
/// what's in the chosen folders).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertiesChange {
    /// The permissions edited (`None`: not touched); mixed bits stay as each file has them.
    pub mode: Option<ModeEdit>,
    pub owner: Option<u32>,
    pub group: Option<u32>,
    /// Into the chosen folders, all the way down.
    pub recursive: bool,
    /// Folders get X wherever they're readable (`0644` → `0755` on folders).
    pub x_on_folders: bool,
}

impl PropertiesChange {
    /// What's set on a file that has `current` (its mode; `None`: unknown) — nothing when
    /// it already is so.
    pub fn for_file(&self, is_dir: bool, current: Option<Mode>) -> SetProperties {
        let mode = self.mode.and_then(|edit| {
            let base = current.or_else(|| edit.whole())?;
            let mut mode = edit.apply(base);
            if is_dir && self.x_on_folders {
                mode = mode.with_x_where_readable();
            }
            (Some(mode) != current).then_some(mode)
        });
        SetProperties { mode, owner: self.owner, group: self.group }
    }

    /// The files' current modes are needed (some bits are mixed: each keeps its own).
    pub fn needs_modes(&self) -> bool {
        self.mode.is_some_and(|edit| edit.whole().is_none())
    }
}

/// A hash of a file's bytes: computed on the server where it can, else here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Algo {
    Sha512,
    Sha384,
    Sha256,
    Sha224,
    Sha1,
    Md5,
}

impl Algo {
    /// As they're offered (the strongest first).
    pub const ALL: [Self; 6] =
        [Self::Sha512, Self::Sha384, Self::Sha256, Self::Sha224, Self::Sha1, Self::Md5];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sha512 => "SHA-512",
            Self::Sha384 => "SHA-384",
            Self::Sha256 => "SHA-256",
            Self::Sha224 => "SHA-224",
            Self::Sha1 => "SHA-1",
            Self::Md5 => "MD5",
        }
    }

    /// Its id in the settings (`sha-256`), and back.
    pub fn id(self) -> &'static str {
        match self {
            Self::Sha512 => "sha-512",
            Self::Sha384 => "sha-384",
            Self::Sha256 => "sha-256",
            Self::Sha224 => "sha-224",
            Self::Sha1 => "sha-1",
            Self::Md5 => "md5",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|a| a.id().eq_ignore_ascii_case(id) || a.label().eq_ignore_ascii_case(id))
    }

    /// How many hex digits its digest has.
    pub fn hex_len(self) -> usize {
        match self {
            Self::Sha512 => 128,
            Self::Sha384 => 96,
            Self::Sha256 => 64,
            Self::Sha224 => 56,
            Self::Sha1 => 40,
            Self::Md5 => 32,
        }
    }

    /// The digest found in a server's answer (`sha256sum`'s line, an FTP reply): the first
    /// word of hex digits of the right length, lower-cased.
    pub fn digest_in(self, answer: &str) -> Option<String> {
        answer
            .split(|c: char| !c.is_ascii_hexdigit())
            .find(|w| w.len() == self.hex_len())
            .map(str::to_ascii_lowercase)
    }
}

/// A file's digest and where it was computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    pub algo: Algo,
    /// Lower-case hex.
    pub hex: String,
    /// The server computed it (else it was downloaded and computed here).
    pub on_server: bool,
}

/// What a session can change and compute on the server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub chmod: bool,
    pub chown: bool,
    pub chgrp: bool,
    /// The hashes the server is known to compute itself (the others: tried, else the file
    /// is downloaded).
    pub server_hashes: Vec<Algo>,
}

/// A listing's line (`ls -l`), the parts that matter here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LsLine {
    pub is_dir: bool,
    pub mode: Mode,
    pub owner: Principal,
    pub group: Principal,
    /// What follows the date (a link's ` -> target` kept).
    pub name: String,
}

/// `drwxr-sr-x  2 www-data www-data 4096 Nov  6 10:00 public` (names or ids, `ls -n`). A
/// line without the group (`ls -o`) or of another shape (DOS): `None`.
pub fn parse_ls_line(line: &str) -> Option<LsLine> {
    let mut words = Vec::with_capacity(9);
    let mut rest = line.trim_start();
    // The eight words before the name, each with where the next begins.
    while words.len() < 8 {
        let end = rest.find(char::is_whitespace)?;
        words.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    let kind = words[0].chars().next()?;
    if !"-dlcbps".contains(kind) {
        return None;
    }
    let mode = Mode::parse_symbolic(words[0].get(1..)?)?;
    // The link count, the size: numbers (a line without its group would put the size third).
    if words[1].parse::<u64>().is_err() || words[4].parse::<u64>().is_err() || rest.is_empty() {
        return None;
    }
    Some(LsLine {
        is_dir: kind == 'd',
        mode,
        owner: Principal::from_word(words[2]),
        group: Principal::from_word(words[3]),
        name: rest.to_owned(),
    })
}

/// An `MLST` / `MLSD` line's facts (`type=file;size=12;UNIX.mode=0644;UNIX.owner=33; a.txt`):
/// the facts by lower-cased name, and the name.
pub fn parse_facts(line: &str) -> Option<(BTreeMap<String, String>, String)> {
    let line = line.strip_prefix(' ').unwrap_or(line).trim_end_matches(['\r', '\n']);
    let (facts, name) = line.split_once(' ')?;
    let facts = facts
        .split(';')
        .filter_map(|f| f.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Some((facts, name.to_owned()))
}

/// The permissions, owner and group the facts give (servers name them differently: ProFTPD
/// `UNIX.owner` = the id and `UNIX.ownername`, Pure-FTPd `UNIX.uid`, some the name in
/// `UNIX.owner`).
pub fn facts_properties(
    facts: &BTreeMap<String, String>,
) -> (Option<Mode>, Option<Principal>, Option<Principal>) {
    let mode = facts.get("unix.mode").and_then(|m| u32::from_str_radix(m, 8).ok()).map(Mode::new);
    let who = |id_keys: &[&str], name_keys: &[&str]| {
        let word = id_keys.iter().find_map(|k| facts.get(*k));
        let mut p = word.map(|w| Principal::from_word(w)).unwrap_or_default();
        if p.name.is_none() {
            p.name = name_keys.iter().find_map(|k| facts.get(*k)).cloned();
        }
        (p != Principal::default()).then_some(p)
    };
    let owner = who(&["unix.owner", "unix.uid"], &["unix.ownername"]);
    let group = who(&["unix.group", "unix.gid"], &["unix.groupname"]);
    (mode, owner, group)
}

/// What `MLST` says of a file as a whole (`None`: the facts don't say its type).
pub fn facts_entry(path: &str, facts: &BTreeMap<String, String>) -> Option<Properties> {
    let is_dir = match facts.get("type")?.to_ascii_lowercase().as_str() {
        "dir" | "cdir" | "pdir" => true,
        // A file, a link (`OS.unix=slink:…`), a device: told as a file.
        _ => false,
    };
    let (mode, owner, group) = facts_properties(facts);
    Some(Properties {
        path: path.to_owned(),
        is_dir,
        size: facts.get("size").and_then(|s| s.parse().ok()).unwrap_or(0),
        modified: facts.get("modify").and_then(|m| facts_time(m)).unwrap_or(0),
        mode,
        owner,
        group,
    })
}

/// An `MLST` time (`YYYYMMDDHHMMSS[.sss]`, UTC) in seconds since the Unix epoch.
pub fn facts_time(text: &str) -> Option<i64> {
    let digits = text.split('.').next()?;
    if digits.len() != 14 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| digits[r].parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(4..6)?, n(6..8)?);
    let (h, min, s) = (n(8..10)?, n(10..12)?, n(12..14)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + min * 60 + s)
}

/// `/etc/passwd`'s or `/etc/group`'s lines (`name:x:id:…`) as accounts.
pub fn parse_accounts(text: &str) -> Vec<Principal> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut parts = l.split(':');
            let name = parts.next()?.trim();
            let id = parts.nth(1)?.trim().parse().ok()?;
            (!name.is_empty()).then(|| Principal::named(name, Some(id)))
        })
        .collect()
}

/// What a value is across several files: the same for all, or not.
pub fn shared<T: PartialEq + Clone>(values: impl IntoIterator<Item = T>) -> Option<T> {
    let mut values = values.into_iter();
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

#[cfg(test)]
#[path = "../tests/unit/properties.rs"]
mod tests;
