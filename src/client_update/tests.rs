use super::{
    build_http_client, cache_directory_for_target, client_update_proxy_from_value,
    is_windows_nsis_asset, normalize_version, validate_repository, windows_nsis_target,
    CachedClientUpdate, ClientUpdateCache, GithubReleaseAsset, DEFAULT_CACHE_DIRECTORY,
};
use prelay_protocol::ClientUpdateTarget;
use std::{path::Path, time::Duration};

fn temporary_cache_directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("prelay-client-update-{}", uuid::Uuid::new_v4()))
}

fn windows_target() -> ClientUpdateTarget {
    ClientUpdateTarget {
        platform: "windows".to_string(),
        architecture: "x64".to_string(),
    }
}

fn unreachable_asset() -> GithubReleaseAsset {
    GithubReleaseAsset {
        name: "Prelay_0.2.1_x64-setup.exe".to_string(),
        // 端口 1 上的连接会立即被拒绝：如果实现没有跳过下载，这里必然失败。
        browser_download_url: "http://127.0.0.1:1/never".to_string(),
    }
}

fn release_installer() -> CachedClientUpdate {
    CachedClientUpdate {
        version: "0.2.1".to_string(),
        file_name: "Prelay_0.2.1_x64-setup.exe".to_string(),
    }
}

/// 直连客户端：测试地址必须真正走网络层被拒绝，而不是被系统代理接管。
fn direct_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build direct test client")
}

#[tokio::test]
async fn keeps_an_already_cached_installer_instead_of_downloading_it_again() {
    let cache_directory = temporary_cache_directory();
    let target = windows_target();
    let directory = cache_directory_for_target(&cache_directory, &target, "0.2.1")
        .expect("valid cache directory");
    tokio::fs::create_dir_all(&directory)
        .await
        .expect("create cache directory");
    let installer = directory.join("Prelay_0.2.1_x64-setup.exe");
    tokio::fs::write(&installer, b"cached")
        .await
        .expect("seed cached installer");

    let cache = ClientUpdateCache::new(
        direct_client(),
        "Zoranner/prelay-client".to_string(),
        cache_directory.clone(),
    );
    cache
        .download_asset(&unreachable_asset(), &release_installer(), &target)
        .await
        .expect("a cached installer needs no download");

    let content = tokio::fs::read(&installer)
        .await
        .expect("read cached installer");
    assert_eq!(content, b"cached");
    let _ = tokio::fs::remove_dir_all(&cache_directory).await;
}

#[tokio::test]
async fn still_downloads_when_the_cached_installer_is_missing() {
    let cache_directory = temporary_cache_directory();
    let target = windows_target();
    let cache = ClientUpdateCache::new(
        direct_client(),
        "Zoranner/prelay-client".to_string(),
        cache_directory.clone(),
    );

    let error = cache
        .download_asset(&unreachable_asset(), &release_installer(), &target)
        .await
        .expect_err("a missing installer must be downloaded");
    let message = format!("{error:#}");
    assert!(
        message.contains("download client installer")
            || message.contains("client installer download failed"),
        "unexpected download error: {message}"
    );
    let _ = tokio::fs::remove_dir_all(&cache_directory).await;
}

#[test]
fn stores_client_updates_outside_the_database_directory_by_default() {
    assert_eq!(DEFAULT_CACHE_DIRECTORY, "updates");
}

#[test]
fn selects_tauri_windows_nsis_installer() {
    assert!(is_windows_nsis_asset("Prelay_0.2.0_x64-setup.exe"));
    assert!(!is_windows_nsis_asset("Prelay_0.2.0_x64_en-US.msi"));
    assert!(!is_windows_nsis_asset("Prelay_0.2.0_x64.AppImage"));
}

#[test]
fn validates_repository_and_version_values_before_using_them_as_paths() {
    assert!(validate_repository("Zoranner/prelay-client").is_ok());
    assert!(validate_repository("https://github.com/Zoranner/prelay-client").is_err());
    assert_eq!(normalize_version("v0.2.0").unwrap(), "0.2.0");
    assert!(normalize_version("../../installer").is_err());
}

#[test]
fn selects_original_installers_from_the_requested_target_directory() {
    let target = ClientUpdateTarget {
        platform: "windows".to_string(),
        architecture: "x64".to_string(),
    };
    assert_eq!(
        cache_directory_for_target(Path::new("updates"), &target, "0.3.0").unwrap(),
        Path::new("updates/windows/x64/0.3.0")
    );
    assert_eq!(
        windows_nsis_target("Prelay_0.3.0_arm64-setup.exe")
            .unwrap()
            .architecture,
        "arm64"
    );
}

#[test]
fn accepts_an_optional_client_update_proxy() {
    assert_eq!(client_update_proxy_from_value(None).unwrap(), None);
    assert_eq!(client_update_proxy_from_value(Some("  ")).unwrap(), None);
    assert_eq!(
        client_update_proxy_from_value(Some(" http://127.0.0.1:7890 ")).unwrap(),
        Some("http://127.0.0.1:7890".to_string())
    );
}

#[test]
fn rejects_an_invalid_client_update_proxy() {
    let error = build_http_client(Duration::ZERO, Duration::ZERO, Some("not a URL"))
        .expect_err("invalid proxy should be rejected");

    assert!(format!("{error:#}").contains("CLIENT_UPDATE_PROXY"));
}
