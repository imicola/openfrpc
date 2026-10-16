use serde::Serialize;
use std::collections::HashMap;
use sysinfo::{ProcessesToUpdate, System};

#[derive(Serialize, Clone, Debug)]
pub struct Listener {
    pub proto: String,
    pub local: String,
    pub port: u16,
    pub state: String,
    pub pid: u32,
    pub pname: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct FrpcProcess {
    pub pid: u32,
    pub name: String,
    pub exe: Option<String>,
    pub start: Option<String>,
}

/// 隐藏窗口执行命令并捕获输出。
/// R10：真实超时——超时先终止进程再返回 None；管道读取移入线程，
/// 避免子进程持有 stdout 不退出时把调用方永久挂住
pub fn run_hidden_capture(cmd: &str, args: &[&str], timeout_secs: u64) -> Option<std::process::Output> {
    let mut c = std::process::Command::new(cmd);
    c.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = c.spawn().ok()?;
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let read_all = |mut p: std::process::ChildStdout| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = std::io::Read::read_to_end(&mut p, &mut b);
            b
        })
    };
    let read_err = |mut p: std::process::ChildStderr| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = std::io::Read::read_to_end(&mut p, &mut b);
            b
        })
    };
    let t_out = out_pipe.map(read_all);
    let t_err = err_pipe.map(read_err);

    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs.max(1));
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    // 超时：终止进程树并回收，管道随之关闭，读取线程自然结束
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    };
    let stdout = t_out.map(|t| t.join().unwrap_or_default()).unwrap_or_default();
    let stderr = t_err.map(|t| t.join().unwrap_or_default()).unwrap_or_default();
    status.map(|st| std::process::Output {
        status: st,
        stdout,
        stderr,
    })
}

/// 枚举本机监听端口与对应进程名
pub fn list_listeners() -> Result<Vec<Listener>, String> {
    let af = netstat2::AddressFamilyFlags::all();
    let proto = netstat2::ProtocolFlags::all();
    let sockets = netstat2::get_sockets_info(af, proto).map_err(|e| format!("获取网络连接失败: {e}"))?;

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut names: HashMap<u32, String> = HashMap::new();
    for (pid, proc) in sys.processes() {
        names.insert(
            pid.as_u32(),
            proc.name().to_string_lossy().to_string(),
        );
    }

    let mut out = Vec::new();
    for si in sockets {
        let pid = si.associated_pids.first().copied().unwrap_or(0);
        let pname = names.get(&pid).cloned().unwrap_or_default();
        match &si.protocol_socket_info {
            netstat2::ProtocolSocketInfo::Tcp(t) => {
                if t.state == netstat2::TcpState::Listen {
                    out.push(Listener {
                        proto: "TCP".into(),
                        local: format!("{}", t.local_addr),
                        port: t.local_port,
                        state: "LISTEN".into(),
                        pid,
                        pname,
                    });
                }
            }
            netstat2::ProtocolSocketInfo::Udp(u) => {
                out.push(Listener {
                    proto: "UDP".into(),
                    local: format!("{}", u.local_addr),
                    port: u.local_port,
                    state: String::new(),
                    pid,
                    pname,
                });
            }
        }
    }
    out.sort_by(|a, b| (a.port, a.proto.clone()).cmp(&(b.port, b.proto.clone())));
    out.dedup_by(|a, b| a.port == b.port && a.proto == b.proto && a.local == b.local);
    Ok(out)
}

/// 找出系统中所有 frpc 进程（用于接管/排障）
pub fn list_frpc_processes() -> Vec<FrpcProcess> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut out = Vec::new();
    for (pid, proc) in sys.processes() {
        let name = proc.name().to_string_lossy().to_string();
        let exe_name = proc
            .exe()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if name.eq_ignore_ascii_case("frpc.exe") || exe_name.eq_ignore_ascii_case("frpc.exe") {
            out.push(FrpcProcess {
                pid: pid.as_u32(),
                name,
                exe: proc.exe().map(|p| p.to_string_lossy().to_string()),
                start: Some(
                    chrono::DateTime::from_timestamp(proc.start_time() as i64, 0)
                        .map(|d| {
                            d.with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_default(),
                ),
            });
        }
    }
    out
}

/// 强杀进程树（taskkill /F /T）
pub fn kill_pid(pid: u32) -> Result<(), String> {
    let pid_str = pid.to_string();
    match run_hidden_capture("taskkill", &["/F", "/T", "/PID", &pid_str], 10) {
        Some(o) if o.status.success() => Ok(()),
        Some(o) => Err(format!(
            "结束进程失败: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )),
        None => Err("无法执行 taskkill".into()),
    }
}

/// R25：杀进程前复核目标仍是 frpc.exe，防止列表过期后 PID 被系统复用而误杀
pub fn verify_and_kill_frpc(pid: u32) -> Result<(), String> {
    let target = sysinfo::Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[target]), true);
    let is_frpc = sys
        .process(target)
        .map(|p| {
            let name = p.name().to_string_lossy().to_lowercase();
            let exe = p
                .exe()
                .and_then(|x| x.file_name())
                .and_then(|x| x.to_str())
                .map(|x| x.to_lowercase())
                .unwrap_or_default();
            name.contains("frpc") || exe == "frpc.exe"
        })
        .unwrap_or(false);
    if !is_frpc {
        return Err(format!(
            "PID {pid} 已不是 frpc 进程，已跳过（可能已退出，或 PID 被其他程序复用）"
        ));
    }
    kill_pid(pid)
}
