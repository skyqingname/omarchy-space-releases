use crate::{config::{AssetPack, DownloadConfig}, vm};
use reqwest::{blocking::Client, header::RANGE, StatusCode};
use std::{fs::{self, File, OpenOptions}, io::{self, Read, Write}, time::{Duration, Instant}};
use tauri::AppHandle;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

// Parts survive an interrupted run, so the next attempt resumes where it stopped.
pub fn fetch(app: &AppHandle, pack: &AssetPack, cfg: &DownloadConfig) -> Result<(), String> {
    fs::create_dir_all(&pack.images).map_err(|e| e.to_string())?;
    let client = Client::builder().no_proxy().user_agent(&cfg.user_agent).timeout(None)
        .connect_timeout(Duration::from_secs(cfg.connect_timeout_seconds))
        .build().map_err(|_| "无法初始化下载".to_string())?;
    let total = pack.download_bytes();
    let mut done = 0;
    let mut reported = Instant::now();
    for (name, parts) in &pack.manifest.downloads {
        let target = pack.images.join(name);
        if target.is_file() {
            done += parts.iter().map(|part| part.bytes).sum::<u64>();
            continue;
        }
        let paths: Vec<_> = (0..parts.len()).map(|index| pack.images.join(format!("{name}.part{index}"))).collect();
        for (part, path) in parts.iter().zip(&paths) {
            let mut have = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            done += have;
            if have == part.bytes { continue; }
            let response = client.get(&part.url).header(RANGE, format!("bytes={have}-")).send().map_err(|_| "无法连接下载地址，请检查网络后重试".to_string())?;
            if response.status() != StatusCode::PARTIAL_CONTENT { return Err(format!("下载服务返回 HTTP {}", response.status().as_u16())); }
            let mut response = response;
            let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
            let mut buffer = vec![0; cfg.buffer_bytes];
            loop {
                let count = response.read(&mut buffer).map_err(|_| "下载中断，点击继续下载会接着上次的进度".to_string())?;
                if count == 0 { break; }
                file.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
                have += count as u64;
                done += count as u64;
                if reported.elapsed() >= Duration::from_millis(cfg.report_milliseconds) {
                    reported = Instant::now();
                    vm::download_progress(app, done as f64 / total as f64, &format!("正在下载 Omarchy：{:.1} / {:.1} GB", done as f64 / GIB, total as f64 / GIB));
                }
            }
            if have != part.bytes { return Err("下载不完整，点击继续下载会接着上次的进度".into()); }
        }
        let pending = pack.images.join(format!("{name}.pending"));
        let mut merged = File::create(&pending).map_err(|e| e.to_string())?;
        for path in &paths {
            io::copy(&mut File::open(path).map_err(|e| e.to_string())?, &mut merged).map_err(|e| e.to_string())?;
        }
        drop(merged);
        fs::rename(&pending, &target).map_err(|e| e.to_string())?;
        for path in &paths { fs::remove_file(path).map_err(|e| e.to_string())?; }
    }
    Ok(())
}
