//! `files::walk` / `parallel_map`, shared by both CLIs.

use momentstays_core::files::{parallel_map, walk, Found};
use std::fs;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ms-files-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn walk_recurses_filters_and_sorts_by_components() {
    let d = dir("walk");
    for f in [
        "b.jpg",
        "a/z.HEIC",
        "a-b/c.jpeg",
        "notes.txt",
        "a/deeper/x.heif",
    ] {
        let p = d.join(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }
    let got: Vec<String> = walk(&d)
        .iter()
        .map(|f| match f {
            Found::File(p) => p
                .strip_prefix(&d)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
            Found::Unreadable(p, e) => panic!("unreadable {}: {e}", p.display()),
        })
        .collect();
    // Component order: "a" < "a-b" < "b.jpg", and a's files before its subdir's.
    assert_eq!(got, ["a/deeper/x.heif", "a/z.HEIC", "a-b/c.jpeg", "b.jpg"]);
}

#[cfg(unix)]
#[test]
fn walk_skips_symlinks() {
    let d = dir("links");
    fs::write(d.join("real.jpg"), b"x").unwrap();
    std::os::unix::fs::symlink(d.join("real.jpg"), d.join("link.jpg")).unwrap();
    std::os::unix::fs::symlink(&d, d.join("loop")).unwrap();
    assert_eq!(walk(&d), vec![Found::File(d.join("real.jpg"))]);
}

#[cfg(unix)]
#[test]
fn unreadable_subdirectory_is_reported_not_fatal() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir("perm");
    fs::write(d.join("ok.jpg"), b"x").unwrap();
    let locked = d.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("hidden.jpg"), b"x").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&locked).is_ok(); // true when running as root
    let found = walk(&d);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        eprintln!("skipped: permissions are not enforced for this user");
        return;
    }
    assert!(found.contains(&Found::File(d.join("ok.jpg"))));
    assert!(found
        .iter()
        .any(|f| matches!(f, Found::Unreadable(p, _) if *p == locked)));
}

#[test]
fn parallel_map_keeps_order() {
    let out = parallel_map(1000, 8, |i| i * 2);
    assert_eq!(out, (0..1000).map(|i| i * 2).collect::<Vec<_>>());
    assert_eq!(parallel_map(0, 8, |i| i), Vec::<usize>::new());
    assert_eq!(parallel_map(3, 1, |i| i), vec![0, 1, 2]);
}
