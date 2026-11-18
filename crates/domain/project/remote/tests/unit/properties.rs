use super::*;

const OWNER_X: Bit = Bit::Access(Class::Owner, Access::Execute);
const GROUP_W: Bit = Bit::Access(Class::Group, Access::Write);

#[test]
fn octal_goes_both_ways_with_three_or_four_digits() {
    assert_eq!(Mode::parse_octal("755").map(Mode::octal).as_deref(), Some("0755"));
    assert_eq!(Mode::parse_octal("4755").map(Mode::bits), Some(0o4755));
    assert_eq!(Mode::parse_octal(" 0644 ").map(Mode::bits), Some(0o644));
    for bad in ["", "75", "07555", "0758", "rwx", "-755"] {
        assert_eq!(Mode::parse_octal(bad), None, "{bad}");
    }
}

#[test]
fn a_mode_drops_the_type_bits() {
    // A folder's mode as SFTP gives it (S_IFDIR | 0755).
    assert_eq!(Mode::new(0o40755).bits(), 0o755);
}

#[test]
fn symbolic_letters_include_the_special_bits() {
    for (text, bits) in [
        ("rwxr-xr-x", 0o755),
        ("rw-r--r--", 0o644),
        ("rwsr-sr-t", 0o7755),
        ("rwSr-Sr-T", 0o7644),
        ("---------", 0),
    ] {
        let mode = Mode::parse_symbolic(text).expect(text);
        assert_eq!(mode.bits(), bits, "{text}");
        assert_eq!(mode.symbolic(), text);
    }
    assert_eq!(Mode::parse_symbolic("rwxr-xr-"), None);
    assert_eq!(Mode::parse_symbolic("rwzr-xr-x"), None);
}

#[test]
fn folders_get_x_where_they_are_readable() {
    assert_eq!(Mode::new(0o644).with_x_where_readable().bits(), 0o755);
    assert_eq!(Mode::new(0o640).with_x_where_readable().bits(), 0o750);
    assert_eq!(Mode::new(0o200).with_x_where_readable().bits(), 0o200);
}

#[test]
fn several_files_share_some_bits_and_mix_the_others() {
    let edit = ModeEdit::of([Mode::new(0o644), Mode::new(0o664)]);
    assert_eq!(edit.state(Bit::Access(Class::Owner, Access::Read)), Tri::On);
    assert_eq!(edit.state(OWNER_X), Tri::Off);
    assert_eq!(edit.state(GROUP_W), Tri::Mixed);
    // Mixed: no octal.
    assert_eq!(edit.octal(), None);
    // Applied, each keeps its own mixed bit.
    assert_eq!(edit.apply(Mode::new(0o644)).bits(), 0o644);
    assert_eq!(edit.apply(Mode::new(0o664)).bits(), 0o664);
    assert!(!edit.is_changed());
}

#[test]
fn a_mixed_box_cycles_back_to_mixed_and_a_plain_one_never_does() {
    let mut edit = ModeEdit::of([Mode::new(0o644), Mode::new(0o664)]);
    edit.toggle(GROUP_W);
    assert_eq!(edit.state(GROUP_W), Tri::On);
    assert!(edit.is_changed());
    edit.toggle(GROUP_W);
    assert_eq!(edit.state(GROUP_W), Tri::Off);
    edit.toggle(GROUP_W);
    assert_eq!(edit.state(GROUP_W), Tri::Mixed);
    assert!(!edit.is_changed());
    edit.toggle(OWNER_X);
    edit.toggle(OWNER_X);
    assert_eq!(edit.state(OWNER_X), Tri::Off);
}

#[test]
fn typed_octal_sets_every_box_and_a_bad_one_changes_nothing() {
    let mut edit = ModeEdit::of([Mode::new(0o644), Mode::new(0o600)]);
    assert!(!edit.set_octal("08"));
    assert_eq!(edit.octal(), None);
    assert!(edit.set_octal("2775"));
    assert_eq!(edit.octal().as_deref(), Some("2775"));
    assert_eq!(edit.state(Bit::Special(Special::SetGid)), Tri::On);
    assert_eq!(edit.apply(Mode::new(0o600)).bits(), 0o2775);
}

#[test]
fn nothing_chosen_is_all_off() {
    assert_eq!(ModeEdit::of([]).octal().as_deref(), Some("0000"));
}

#[test]
fn principals_show_their_name_and_id() {
    assert_eq!(Principal::named("www-data", Some(33)).to_string(), "www-data [33]");
    assert_eq!(Principal::named("ftp", None).to_string(), "ftp");
    assert_eq!(Principal::id(1000).to_string(), "[1000]");
    let known = Principal::id(33).completed(&Principal::named("www-data", None));
    assert_eq!(known, Principal::named("www-data", Some(33)));
    assert!(Principal::id(33).same(&Principal::named("x", Some(33))));
    assert!(!Principal::id(33).same(&Principal::named("x", None)));
}

#[test]
fn ls_lines_give_mode_owner_group_and_name() {
    let line = "drwxr-sr-x    2 www-data www-data     4096 Nov  6 10:00 my folder";
    let ls = parse_ls_line(line).expect("parsed");
    assert!(ls.is_dir);
    assert_eq!(ls.mode.bits(), 0o2755);
    assert_eq!(ls.owner, Principal::named("www-data", None));
    assert_eq!(ls.name, "my folder");
    // `ls -n`: ids; an ACL's `+` after the letters.
    let ls = parse_ls_line("-rw-r--r--+ 1 1000 33 12 Jan  1  2025 a.txt").expect("parsed");
    assert_eq!((ls.owner, ls.group), (Principal::id(1000), Principal::id(33)));
    assert_eq!(ls.mode.bits(), 0o644);
    // DOS listings, lines without a group.
    assert_eq!(parse_ls_line("11-06-25  10:00AM       <DIR>          public"), None);
    assert_eq!(parse_ls_line("-rw-r--r-- 1 user 12 Jan  1  2025 a.txt"), None);
}

#[test]
fn mlst_facts_name_owners_however_the_server_does() {
    let (facts, name) =
        parse_facts(" type=file;size=12;UNIX.mode=0644;UNIX.owner=33;UNIX.ownername=www-data;UNIX.group=33;UNIX.groupname=www-data; index.php\r\n")
            .expect("parsed");
    assert_eq!(name, "index.php");
    let (mode, owner, group) = facts_properties(&facts);
    assert_eq!(mode, Some(Mode::new(0o644)));
    assert_eq!(owner, Some(Principal::named("www-data", Some(33))));
    assert_eq!(group, Some(Principal::named("www-data", Some(33))));
    // Pure-FTPd: ids only.
    let (facts, _) =
        parse_facts("type=dir;UNIX.mode=755;UNIX.uid=1000;UNIX.gid=1000; app").unwrap();
    let (mode, owner, _) = facts_properties(&facts);
    assert_eq!(mode, Some(Mode::new(0o755)));
    assert_eq!(owner, Some(Principal::id(1000)));
    // No Unix facts (a Windows server).
    let (facts, _) = parse_facts("type=file;size=3; a").unwrap();
    assert_eq!(facts_properties(&facts), (None, None, None));
}

#[test]
fn accounts_are_read_from_passwd_and_merged_with_what_files_have() {
    let users = parse_accounts(
        "# comment\nroot:x:0:0:root:/root:/bin/sh\nwww-data:x:33:33::/var/www:/usr/sbin/nologin\nbroken\n",
    );
    assert_eq!(users, [Principal::named("root", Some(0)), Principal::named("www-data", Some(33))]);
    let accounts = Accounts { users, groups: Vec::new() }
        .with_seen([&Principal::id(33), &Principal::id(1001)], [&Principal::named("staff", None)]);
    assert_eq!(accounts.users.len(), 3);
    assert!(accounts.users.contains(&Principal::id(1001)));
    assert_eq!(accounts.groups, [Principal::named("staff", None)]);
}

#[test]
fn shared_values_are_kept_and_differing_ones_are_not() {
    assert_eq!(shared([1, 1, 1]), Some(1));
    assert_eq!(shared([1, 2]), None);
    assert_eq!(shared(Vec::<u8>::new()), None);
}

#[test]
fn mlst_facts_make_a_whole_entry_with_its_time() {
    assert_eq!(facts_time("19700101000000"), Some(0));
    assert_eq!(facts_time("20251106103000.123"), Some(1_762_425_000));
    assert_eq!(facts_time("2025110610"), None);
    let (facts, _) =
        parse_facts("type=dir;modify=20000101000000;UNIX.mode=0750; /var/www/app").unwrap();
    let entry = facts_entry("app", &facts).unwrap();
    assert!(entry.is_dir);
    assert_eq!((entry.modified, entry.mode), (946_684_800, Some(Mode::new(0o750))));
    let (facts, _) = parse_facts("size=1; a").unwrap();
    assert_eq!(facts_entry("a", &facts), None);
}

#[test]
fn the_accounts_name_an_owner_known_by_its_id() {
    let accounts = Accounts {
        users: vec![Principal::named("www-data", Some(33))],
        groups: vec![Principal::named("staff", Some(50))],
    };
    let props = Properties {
        path: "a".into(),
        is_dir: false,
        size: 0,
        modified: 0,
        mode: None,
        owner: Some(Principal::id(33)),
        group: Some(Principal::id(7)),
    };
    let named = accounts.naming(props);
    assert_eq!(named.owner, Some(Principal::named("www-data", Some(33))));
    assert_eq!(named.group, Some(Principal::id(7)), "unknown: kept as it is");
}

#[test]
fn a_change_sets_only_what_differs_and_keeps_mixed_bits() {
    let edit = |modes: &[u32]| ModeEdit::of(modes.iter().map(|m| Mode::new(*m)));
    let mut mixed = edit(&[0o644, 0o664]);
    mixed.toggle(Bit::Access(Class::Others, Access::Read));
    let change = PropertiesChange {
        mode: Some(mixed),
        owner: None,
        group: None,
        recursive: false,
        x_on_folders: false,
    };
    assert!(change.needs_modes());
    // Others' read off, the group's write as each had it.
    assert_eq!(change.for_file(false, Some(Mode::new(0o664))).mode, Some(Mode::new(0o660)));
    // Mixed bits and an unknown mode: nothing to base it on.
    assert_eq!(change.for_file(false, None).mode, None);
    let mut whole = edit(&[0o644]);
    assert!(whole.set_octal("0640"));
    let change = PropertiesChange { mode: Some(whole), x_on_folders: true, ..change };
    assert!(!change.needs_modes());
    assert_eq!(change.for_file(false, None).mode, Some(Mode::new(0o640)));
    assert_eq!(change.for_file(true, None).mode, Some(Mode::new(0o750)));
    // Already so: nothing set.
    assert!(change.for_file(false, Some(Mode::new(0o640))).is_empty());
}

#[test]
fn a_digest_is_found_in_a_server_answer_by_its_length() {
    let sum = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  /var/www/a.txt";
    assert_eq!(Algo::Sha256.digest_in(sum).map(|h| h.len()), Some(64));
    assert_eq!(Algo::Md5.digest_in(sum), None);
    let ftp = "213 SHA-1 0-11 2AAE6C35C94FCFB415DBE95F408B9CE91EE846ED a.txt";
    assert_eq!(
        Algo::Sha1.digest_in(ftp).as_deref(),
        Some("2aae6c35c94fcfb415dbe95f408b9ce91ee846ed")
    );
    assert_eq!(Algo::from_id("SHA-512"), Some(Algo::Sha512));
    assert_eq!(Algo::from_id("md5"), Some(Algo::Md5));
    assert_eq!(Algo::ALL[0], Algo::Sha512);
}
