use super::*;

#[test]
fn round_trip_and_version_check() {
    let file = CachedFile {
        path: "app/User.php".into(),
        len: 120,
        modified: 42,
        hash: hash(b"<?php class User {}"),
        symbols: Arc::new(FileSymbols {
            language: "php".into(),
            namespace: Some("App".into()),
            uses: vec![UseImport { alias: "Model".into(), fqn: "Illuminate\\Model".into() }],
            symbols: vec![Symbol {
                name: "User".into(),
                kind: SymbolKind::Class,
                fqn: "App\\User".into(),
                container: None,
                parents: Box::new(["Illuminate\\Model".into()]),
                range: 6..40,
                offset: 12,
                line: 0,
                detail: None,
                is_static: false,
                visibility: Default::default(),
                note: None,
                types: Some("t TValue\nx Model<TValue>\n".into()),
            }],
        }),
        packed: None,
        block: None,
    };
    let (bytes, blocks) = encode_with_blocks(7, std::slice::from_ref(&file));
    let mut pool = Pool::default();
    let decoded = decode(7, &bytes, &mut pool).unwrap();
    let block = blocks[0];
    let raw: Arc<[u8]> =
        Arc::from(&bytes[block.at as usize..(block.at + u64::from(block.len)) as usize]);
    let told = CachedFile { block: Some(block), packed: Some(raw), ..file };
    assert_eq!(decoded, vec![told], "where it is, its block's bytes kept");
    // Written again packed: the same bytes, not packed anew.
    let again = CachedFile { symbols: Arc::default(), ..decoded[0].clone() };
    assert_eq!(encode_with_blocks(7, std::slice::from_ref(&again)).0, bytes);
    // The repeated texts are shared: `Illuminate\Model` (an import and a parent) once.
    let symbols = &decoded[0].symbols;
    assert!(Arc::ptr_eq(&symbols.uses[0].fqn, &symbols.symbols[0].parents[0]));
    assert_eq!(decode(8, &bytes, &mut Pool::default()), None, "another version of the extractors");
    assert_eq!(
        decode(7, &bytes[..bytes.len() - 3], &mut Pool::default()),
        None,
        "a truncated file"
    );
}

/// What a project's symbol cache is made of (sizes per field, repetitions, `vendor`), for
/// memory work: `INDEX_STATS=<project>/<project dir>/index/symbols.bin cargo test -p indexer
/// index_stats -- --ignored --nocapture`.
#[test]
#[ignore = "an analysis tool, run by hand"]
fn index_stats() {
    use std::collections::HashSet;
    let Ok(path) = std::env::var("INDEX_STATS") else { return };
    let bytes = std::fs::read(path).unwrap();
    let extractors = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let files = decode(extractors, &bytes, &mut Pool::default()).unwrap();
    let (mut symbols, mut vendor_files, mut vendor_symbols, mut private_vendor) = (0, 0, 0, 0);
    let mut field = [0usize; 6]; // name, fqn, container, parents, detail, uses
    let mut distinct: [HashSet<String>; 3] = Default::default(); // fqn, container, detail
    for f in &files {
        let vendor = f.path.starts_with("vendor/") || f.path.contains("node_modules/");
        vendor_files += usize::from(vendor);
        field[5] += f.symbols.uses.iter().map(|u| u.alias.len() + u.fqn.len()).sum::<usize>();
        for s in &f.symbols.symbols {
            symbols += 1;
            vendor_symbols += usize::from(vendor);
            private_vendor +=
                usize::from(vendor && s.visibility == code_intelligence::Visibility::Private);
            field[0] += s.name.len();
            field[1] += s.fqn.len();
            field[2] += s.container.as_ref().map_or(0, |c| c.len());
            field[3] += s.parents.iter().map(|p| p.len()).sum::<usize>();
            field[4] += s.detail.as_ref().map_or(0, |d| d.len());
            distinct[0].insert(s.fqn.to_string());
            if let Some(c) = &s.container {
                distinct[2 - 1].insert(c.to_string());
            }
            if let Some(d) = &s.detail {
                distinct[2].insert(d.to_string());
            }
        }
    }
    eprintln!(
        "files {} (vendor {vendor_files}), symbols {symbols} (vendor {vendor_symbols}, private in vendor {private_vendor})",
        files.len()
    );
    eprintln!(
        "bytes: name {} fqn {} container {} parents {} detail {} uses {}",
        field[0], field[1], field[2], field[3], field[4], field[5]
    );
    eprintln!(
        "distinct: fqn {} container {} detail {}",
        distinct[0].len(),
        distinct[1].len(),
        distinct[2].len()
    );
    eprintln!("size_of Symbol {}", std::mem::size_of::<code_intelligence::Symbol>());
}

fn cached(path: &str, class: &str, members: &[&str]) -> CachedFile {
    let symbol = |name: &str, kind, container: Option<&str>| Symbol {
        name: name.into(),
        kind,
        fqn: name.into(),
        container: container.map(Into::into),
        parents: Box::default(),
        range: 0..1,
        offset: 0,
        line: 0,
        detail: Some("(): void".into()),
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    };
    let mut symbols = vec![symbol(class, SymbolKind::Class, None)];
    symbols.extend(members.iter().map(|m| symbol(m, SymbolKind::Method, Some(class))));
    CachedFile {
        path: path.into(),
        len: 1,
        modified: 1,
        hash: 1,
        symbols: Arc::new(FileSymbols {
            language: "php".into(),
            symbols,
            ..FileSymbols::default()
        }),
        packed: None,
        block: None,
    }
}

/// One file's symbols read alone from the cache on disk (plan 74): its block only, where the
/// whole file's decoding said it is; the same symbols as decoding everything gives.
#[test]
fn one_file_s_block_reads_alone() {
    let files =
        [cached("vendor/A.php", "A", &["one", "two"]), cached("vendor/B.php", "B", &["three"])];
    let (bytes, blocks) = encode_with_blocks(3, &files);
    let dir = std::env::temp_dir().join(format!("codec-blocks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("symbols.bin");
    std::fs::write(&path, &bytes).unwrap();
    let decoded = decode(3, &bytes, &mut Pool::default()).unwrap();
    assert_eq!(decoded.iter().map(|f| f.block.unwrap()).collect::<Vec<_>>(), blocks);
    let block = read_block(&path, blocks[1]).unwrap();
    let alone = unpack(&block, &mut Pool::default()).unwrap();
    assert_eq!(alone, *files[1].symbols, "B's symbols, nothing of A's");
    assert!(unpack(&block[..block.len() - 1], &mut Pool::default()).is_none(), "cut short");
    let _ = std::fs::remove_dir_all(dir);
}

/// How long the cache takes to read whole (the start) and one block alone (a library file's
/// members, first asked for), on a real project's cache: `BLOCK_TIMES=<project>/<project
/// dir>/index/symbols.bin cargo test -p indexer --release block_times -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by hand"]
#[allow(clippy::disallowed_methods)] // Measured by the test, not the code.
fn block_times() {
    let Ok(path) = std::env::var("BLOCK_TIMES") else { return };
    let bytes = std::fs::read(&path).unwrap();
    let extractors = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let started = std::time::Instant::now();
    let files = decode(extractors, &bytes, &mut Pool::default()).expect("this format's cache");
    let whole = started.elapsed();
    let mut times: Vec<(std::time::Duration, u32, usize)> = files
        .iter()
        .map(|f| {
            let block = f.block.unwrap();
            let at = block.at as usize;
            let raw = &bytes[at..at + block.len as usize];
            let started = std::time::Instant::now();
            let symbols = unpack(raw, &mut Pool::default()).unwrap();
            (started.elapsed(), block.len, symbols.symbols.len())
        })
        .collect();
    times.sort_by_key(|t| t.0);
    let total: std::time::Duration = times.iter().map(|t| t.0).sum();
    let packed: u64 = times.iter().map(|t| u64::from(t.1)).sum();
    let (worst, worst_len, worst_symbols) = times[times.len() - 1];
    let median = times[times.len() / 2].0;
    eprintln!("\n{} files, {} bytes, {packed} bytes in blocks", files.len(), bytes.len());
    eprintln!("the whole cache read: {whole:?}");
    eprintln!("a block alone: median {median:?}, mean {:?}", total / times.len() as u32);
    eprintln!("the worst: {worst:?} ({worst_len} bytes, {worst_symbols} symbols)");
}
