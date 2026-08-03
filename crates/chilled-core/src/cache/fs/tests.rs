use super::*;
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn store_fetch_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("a/b/entry");

    assert_eq!(fetch_file(&path), None);
    store_file(&path, b"hello", None);
    assert_eq!(fetch_file(&path), Some(b"hello".to_vec()));
}

#[test]
fn store_pins_mtime() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("entry");
    let mtime = UNIX_EPOCH + Duration::from_secs(784_111_777);

    store_file(&path, b"x", Some(mtime));
    assert_eq!(file_mtime(&path), Some(mtime));
}

#[test]
fn mtime_of_missing_file_is_none() {
    let tmp = tempfile::TempDir::new().unwrap();
    assert_eq!(file_mtime(&tmp.path().join("missing")), None);
}

#[test]
fn writes_are_atomic_and_leave_no_temp_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("sub/entry");

    store_file(&path, b"first", None);
    store_file(&path, b"second", None);
    assert_eq!(fetch_file(&path), Some(b"second".to_vec()));

    // The rename target is the only file left behind.
    let left: Vec<_> = std::fs::read_dir(tmp.path().join("sub"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["entry"]);
}

#[test]
fn concurrent_writers_never_leave_a_partial_body() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = std::sync::Arc::new(tmp.path().join("entry"));
    let big_a = vec![b'a'; 512 * 1024];
    let big_b = vec![b'b'; 512 * 1024];

    let handles: Vec<_> = [big_a.clone(), big_b.clone()]
        .into_iter()
        .map(|body| {
            let path = path.clone();
            std::thread::spawn(move || {
                for _ in 0..8 {
                    store_file(&path, &body, None);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    // Whichever writer won, the body is one of them in full — never a mix.
    let got = fetch_file(&path).unwrap();
    assert!(got == big_a || got == big_b, "torn write observed");
}
