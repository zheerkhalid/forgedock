//! Tests that talk to GitHub. They are ignored by default; run them with
//! `cargo test --test live -- --ignored --nocapture`.
//! Nothing is installed: one small portable ZIP is downloaded to a temporary
//! folder, checked and deleted.

use forgedock::catalog::Catalog;
use forgedock::github::{self, AssetKind};
use forgedock::{actions, net};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[test]
#[ignore = "needs the network"]
fn live_release_lookup_and_one_download() {
    let cat = Catalog::embedded();
    let app = cat.apps.iter().find(|a| a.id == "shredforge").unwrap();
    let repo = app.repo_name().unwrap();
    let release = github::fetch_latest(repo).expect("latest release");
    println!(
        "latest {} {} ({})",
        app.name,
        release.version(),
        release.date()
    );
    assert!(release.asset(app, AssetKind::Setup).is_some());
    assert!(release.asset(app, AssetKind::Exe).is_some());
    let zip = release
        .asset(app, AssetKind::Portable)
        .expect("portable zip");
    println!(
        "asset {} {} bytes sha256 {:?}",
        zip.name, zip.size, zip.sha256
    );

    let dir = std::env::temp_dir().join(format!("forgedock-live-{}", std::process::id()));
    let progress = AtomicU64::new(0);
    let cancel = AtomicBool::new(false);
    let path = actions::download_asset(zip, repo, &dir, &progress, &cancel).expect("download");
    let on_disk = std::fs::metadata(&path).unwrap().len();
    println!("downloaded {} bytes to a temp folder", on_disk);
    assert_eq!(on_disk, zip.size);
    assert_eq!(progress.load(Ordering::Relaxed), zip.size);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "needs the network"]
fn live_catalog_and_icon() {
    // The catalogue may not be published yet; the request itself must work.
    let resp =
        net::get(forgedock::CATALOG_URL, &[], 1 << 20).expect("reach raw.githubusercontent.com");
    println!("catalog.json status {}", resp.status);
    if resp.status == 200 {
        Catalog::parse(&String::from_utf8_lossy(&resp.body)).expect("published catalogue parses");
    }
    let cat = Catalog::embedded();
    for app in cat.apps.iter().filter(|a| a.available()) {
        let icon = net::get(&app.icon, &[], 4 << 20).expect("icon request");
        let shot = net::get(&app.screenshots[0], &[], 12 << 20).expect("screenshot request");
        println!(
            "{}: icon {} ({} bytes), first screenshot {} ({} bytes)",
            app.id,
            icon.status,
            icon.body.len(),
            shot.status,
            shot.body.len()
        );
        assert_eq!(icon.status, 200, "{}", app.icon);
        assert_eq!(shot.status, 200, "{}", app.screenshots[0]);
    }
}
