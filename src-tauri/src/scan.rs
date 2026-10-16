use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

#[derive(Serialize, Clone, Debug)]
pub struct PortResult {
    pub port: u16,
    pub open: bool,
    pub latency_ms: Option<u32>,
}

async fn resolve_host(host: &str) -> Result<IpAddr, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ip);
    }
    let addrs = tokio::net::lookup_host((host, 0u16))
        .await
        .map_err(|e| format!("无法解析主机名「{host}」: {e}"))?;
    addrs
        .into_iter()
        .next()
        .map(|a| a.ip())
        .ok_or_else(|| "主机名没有解析到任何地址".to_string())
}

/// 并发 TCP 连接扫描，进度通过 scan:progress 事件推送。
/// R30：事件携带任务标识 task，前端据此过滤只属于本任务的事件
pub async fn scan_ports(
    app: &AppHandle,
    host: &str,
    ports: Vec<u16>,
    timeout_ms: u64,
    concurrency: usize,
    task: Option<String>,
) -> Result<Vec<PortResult>, String> {
    if ports.is_empty() {
        return Err("端口列表为空".into());
    }
    if ports.len() > 65536 {
        return Err("端口数量超过上限（65536）".into());
    }
    let ip = resolve_host(host).await?;
    let total = ports.len();
    let sem = Arc::new(Semaphore::new(concurrency.clamp(1, 1024)));
    let done = Arc::new(AtomicUsize::new(0));
    let mut ports_sorted = ports;
    ports_sorted.sort_unstable();
    ports_sorted.dedup();

    let mut set = tokio::task::JoinSet::new();
    for port in ports_sorted {
        let permit = sem.clone().acquire_owned().await.unwrap();
        let app = app.clone();
        let done = done.clone();
        let task = task.clone();
        let timeout = Duration::from_millis(timeout_ms.clamp(100, 10_000));
        set.spawn(async move {
            let _permit = permit;
            let addr = SocketAddr::new(ip, port);
            let t0 = Instant::now();
            let connected = tokio::time::timeout(timeout, TcpStream::connect(addr)).await;
            let latency = match &connected {
                Ok(Ok(_)) => Some((t0.elapsed().as_millis() as u32).max(1)),
                _ => None,
            };
            let result = PortResult {
                port,
                open: matches!(connected, Ok(Ok(_))),
                latency_ms: latency,
            };
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            if d % 20 == 0 || d == total {
                let _ = app.emit(
                    "scan:progress",
                    serde_json::json!({ "task": task, "done": d, "total": total }),
                );
            }
            result
        });
    }

    let mut out = Vec::with_capacity(total);
    while let Some(r) = set.join_next().await {
        if let Ok(res) = r {
            out.push(res);
        }
    }
    out.sort_by_key(|r| r.port);
    Ok(out)
}

#[derive(Serialize, Clone, Debug)]
pub struct ProbeResult {
    pub host: String,
    pub port: u16,
    pub samples: Vec<Option<u32>>,
    pub success: usize,
    pub failed: usize,
    pub min_ms: Option<u32>,
    pub avg_ms: Option<u32>,
    pub max_ms: Option<u32>,
}

/// 对 frps 服务器做 TCP 连通性探测（直连，不经过 HTTP 代理）
pub async fn probe_server(host: &str, port: u16, count: u32) -> Result<ProbeResult, String> {
    let ip = resolve_host(host).await?;
    let count = count.clamp(1, 60);
    let mut samples: Vec<Option<u32>> = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let t0 = Instant::now();
        let r = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(SocketAddr::new(ip, port))).await;
        match r {
            Ok(Ok(_)) => samples.push(Some((t0.elapsed().as_millis() as u32).max(1))),
            _ => samples.push(None),
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let ok: Vec<u32> = samples.iter().filter_map(|s| *s).collect();
    let avg = if ok.is_empty() {
        None
    } else {
        Some((ok.iter().sum::<u32>() / ok.len() as u32).max(1))
    };
    Ok(ProbeResult {
        host: host.to_string(),
        port,
        success: ok.len(),
        failed: samples.len() - ok.len(),
        min_ms: ok.iter().min().copied(),
        avg_ms: avg,
        max_ms: ok.iter().max().copied(),
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejected_connection_is_not_open() {
        // 127.0.0.1:1 几乎必然拒绝连接，验证探测逻辑不 panic 且判为关闭
        let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let r = tokio::time::timeout(
            Duration::from_millis(1000),
            TcpStream::connect(std::net::SocketAddr::new(ip, 1)),
        )
        .await;
        let closed = match r {
            Ok(res) => res.is_err(),
            Err(_) => true,
        };
        assert!(closed);
    }

    #[tokio::test]
    async fn probe_lo_handles_rejection() {
        let out = probe_server("127.0.0.1", 1, 2).await.unwrap();
        assert_eq!(out.samples.len(), 2);
        assert!(out.success == 0 || out.samples.iter().any(|s| s.is_none()));
    }
}
