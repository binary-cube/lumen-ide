use super::*;

#[test]
fn only_plain_http_urls() {
    assert!(allowed("https://example.com/a.png"));
    assert!(allowed("http://example.com:8080/a.png"));
    assert!(!allowed("https://user:pass@example.com/a.png"));
    assert!(!allowed("file:///etc/passwd"));
    assert!(!allowed("ftp://example.com/a.png"));
    assert!(!allowed("https:///nohost"));
    assert_eq!(fetch("file:///x", Limits::default(), &|_| true), Err(Error::Refused));
}

#[test]
fn the_cache_keeps_files_by_url_and_its_cap() {
    let dir = std::env::temp_dir().join(format!("web-fetch-tests-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = DiskCache::new(dir.clone(), 10);
    let a = cache.put("https://a/x.png", "png", b"123456").expect("kept");
    assert_eq!(cache.get("https://a/x.png"), Some(a.clone()));
    assert_eq!(cache.get("https://b/y.png"), None);
    // Over the cap: the oldest goes.
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    std::fs::File::options().write(true).open(&a).expect("open").set_modified(old).expect("time");
    cache.put("https://b/y.png", "png", b"123456").expect("kept");
    assert_eq!(cache.get("https://a/x.png"), None);
    assert!(cache.get("https://b/y.png").is_some());
    assert!(cache.size() <= 10);
    cache.clear();
    assert_eq!(cache.size(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server on this machine answering every request with `body` as `kind`.
fn serve(kind: &'static str, body: Vec<u8>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("its address");
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{address}/file")
}

#[test]
fn fetches_with_its_limits() {
    let images = |k: &str| k.starts_with("image/");
    let url = serve("image/png; charset=binary", vec![1, 2, 3]);
    assert_eq!(
        fetch(&url, Limits::default(), &images),
        Ok(Fetched { bytes: vec![1, 2, 3], kind: "image/png".into() })
    );
    let page = serve("text/html", b"<html>".to_vec());
    assert_eq!(fetch(&page, Limits::default(), &images), Err(Error::Kind("text/html".into())));
    let big = serve("image/png", vec![0; 100]);
    let small = Limits { max_bytes: 10, ..Limits::default() };
    assert_eq!(fetch(&big, small, &images), Err(Error::TooLarge));
    assert!(matches!(
        fetch(
            "http://127.0.0.1:9/x",
            Limits { timeout: std::time::Duration::from_secs(2), ..Limits::default() },
            &images
        ),
        Err(Error::Offline(_))
    ));
}
