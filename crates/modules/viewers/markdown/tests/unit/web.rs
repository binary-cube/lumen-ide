use std::io::{Read, Write};
use std::net::TcpListener;

use super::*;

/// A server on this machine answering every request with `body` as `kind`.
fn serve(kind: &'static str, body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("its address");
    // The test's server lives as long as the test process: a thread of its own.
    #[allow(clippy::disallowed_methods)]
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
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
    format!("http://{address}/image")
}

fn png() -> Vec<u8> {
    let mut pixmap = tiny_skia::Pixmap::new(30, 20).expect("pixmap");
    pixmap.fill(tiny_skia::Color::WHITE);
    pixmap.encode_png().expect("png")
}

fn wait(images: &mut WebImages, url: &str) -> Fetch {
    let started = std::time::Instant::now();
    loop {
        images.poll();
        let f = images.get(url);
        if f != Fetch::Pending {
            return f;
        }
        assert!(started.elapsed().as_secs() < 20, "downloaded meanwhile");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn web_images_are_downloaded_kept_and_refused() {
    let root = std::env::temp_dir().join(format!("viewer-markdown-web-{}", std::process::id()));
    files::places::isolate(root.clone());
    let Some(cache) = cache() else { panic!("a cache folder") };
    assert!(cache.dir().starts_with(&root), "never the user's own folder");
    crate::settings::set_web_images(true);
    let url = serve("image/png", png());
    let mut images = WebImages::default();
    assert_eq!(images.get(&url), Fetch::Pending);
    let Fetch::Ready(path) = wait(&mut images, &url) else { panic!("downloaded") };
    assert!(path.starts_with(cache.dir()) && path.extension().is_some_and(|e| e == "png"));
    assert!(downloaded(&path));
    // Kept: another preview finds it on disk.
    assert_eq!(WebImages::default().get(&url), Fetch::Ready(path));
    // Not an image: refused.
    let html = serve("text/html", b"<html></html>".to_vec());
    assert!(matches!(wait(&mut images, &html), Fetch::Failed(_)));
    // Off: nothing asked.
    crate::settings::set_web_images(false);
    assert!(matches!(WebImages::default().get("http://127.0.0.1:9/never"), Fetch::Failed(_)));
    crate::settings::set_web_images(true);
    cache.clear();
    let _ = std::fs::remove_dir_all(&root);
}
