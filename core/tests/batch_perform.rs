//! `engine::batch::perform` admission control under failure.

use momentstays_core::engine::batch::perform;
use momentstays_core::engine::StopToken;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn a_panicking_item_releases_its_budget_instead_of_hanging_the_batch() {
    // Missing files are charged the whole memory budget, so the second
    // item can only start once the first item's reservation is released.
    let files: Vec<PathBuf> = (0..3)
        .map(|i| PathBuf::from(format!("/nonexistent/momentstays-perform-{i}.jpg")))
        .collect();
    let (done, wait) = mpsc::channel();
    std::thread::spawn(move || {
        let token = StopToken::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            perform(
                &files,
                2,
                true,
                &token,
                |i| {
                    if i == 0 {
                        panic!("item 0 fails");
                    }
                    i
                },
                |_, _| {},
            )
        }));
        let _ = done.send(result.is_err());
    });
    // Before the fix the leaked reservation kept the other worker waiting
    // for budget forever, so `perform` never returned.
    let panicked = wait
        .recv_timeout(Duration::from_secs(20))
        .expect("perform returned instead of hanging");
    assert!(panicked, "the item's panic is still reported to the caller");
}
