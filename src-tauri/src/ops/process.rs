use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// 进程管理器：以 shell 启动本地服务，并记录子进程句柄以便停止。
pub struct ProcManager {
    children: Mutex<HashMap<String, Child>>,
}

impl ProcManager {
    pub fn new() -> Self {
        Self {
            children: Mutex::new(HashMap::new()),
        }
    }

    /// 该服务是否已有存活进程。
    ///
    /// 真实存活探测（P1-4）：用 `try_wait` 探一次子进程——若已退出则立即回收僵尸并
    /// 移除记录，返回 false；仍在跑返回 true。这样「进程崩溃但我们还握着句柄」不会一直
    /// 误报 running=true，也不会留下僵尸进程占用 PID 表。
    pub fn is_running(&self, id: &str) -> bool {
        let mut map = self.children.lock().unwrap();
        if let Some(child) = map.get_mut(id) {
            match child.try_wait() {
                // 子进程已退出：回收并清理记录
                Ok(Some(_)) => {
                    map.remove(id);
                    false
                }
                // 仍在运行
                Ok(None) => true,
                // 等待出错（权限/平台异常等）：保守移除，避免僵尸堆积
                Err(_) => {
                    map.remove(id);
                    false
                }
            }
        } else {
            false
        }
    }

    /// 启动服务（若已在运行则忽略）。run_cmd 以系统 shell 执行，
    /// 工作目录为 install_dir（相对路径命令如 `java -jar dfy-server/target/xxx.jar` 依赖它）。
    pub fn start(&self, id: &str, install_dir: &str, run_cmd: &str) -> std::io::Result<()> {
        let mut map = self.children.lock().unwrap();
        if map.contains_key(id) {
            return Ok(());
        }
        let child = spawn_detached(install_dir, run_cmd)?;
        map.insert(id.to_string(), child);
        Ok(())
    }

    /// 停止服务（结束记录中的子进程及其整个进程树）。
    ///
    /// - Windows：服务经 `cmd /C` 启动，用 `taskkill /T /F` 连进程树一起结束
    ///   （`kill()` 只能杀 cmd.exe，java/nginx 等孙进程会变孤儿）。
    /// - Unix：启动时为 shell 设了独立进程组（pgid == shell pid），这里用
    ///   `kill -KILL -<pgid>` 杀整组，`sh` 与其孙进程（java/nginx）一起退（P1-3）。
    pub fn stop(&self, id: &str) -> std::io::Result<()> {
        let mut map = self.children.lock().unwrap();
        if let Some(mut child) = map.remove(id) {
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(unix)]
            {
                let pgid = child.id() as i32;
                // 负 pid 表示「进程组」；该组由 spawn 时 process_group(0) 设定。
                // 进程已自行退出时 kill 会报 EPERM/ESRCH，把 stderr 丢掉避免污染日志
                // （结果我们本就忽略，兜底仍走 child.kill() + child.wait()）。
                let _ = Command::new("kill")
                    .args(["-KILL", &format!("-{pgid}")])
                    .stderr(Stdio::null())
                    .status();
            }
            // 兜底：确保本进程句柄对应的直接子进程被结束并回收
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }

    /// 触发一次构建（异步、独立进程，不跟踪句柄），工作目录为 install_dir。
    /// 构建输出重定向到 `log_path`，便于前端轮询展示进度；另起线程等待进程结束
    /// 并写入结束标记（含 exit code），使前端能识别「构建完成 / 成功 / 失败」并停止轮询。
    pub fn build(&self, install_dir: &str, build_cmd: &str, log_path: &str) -> std::io::Result<()> {
        let child = spawn_to_log(install_dir, build_cmd, log_path)?;
        let marker_path = log_path.to_string();
        std::thread::spawn(move || {
            let mut child = child;
            let marker = match child.wait() {
                Ok(s) => format!("\n==== 构建结束：exit code {} ====\n", s.code().unwrap_or(-1)),
                Err(e) => format!("\n==== 构建等待失败：{} ====\n", e),
            };
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&marker_path)
            {
                let _ = f.write_all(marker.as_bytes());
            }
        });
        Ok(())
    }
}

/// 本机是否存在命令行匹配给定名字的进程（全命令行 substring 匹配）。
///
/// 无端口服务（Tauri 桌面客户端等）无法靠端口探活，这是它们的「检测到在运行」兜底通道：
/// Unix 用 `pgrep -f`（退出码 0 即命中），Windows 用 `tasklist` 过滤镜像名。
/// Best-effort：命令执行失败一律视为未运行，不 panic、不阻塞调用方（调用方已在 blocking 池）。
pub fn process_alive(name: &str) -> bool {
    if name.trim().is_empty() {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {name}"), "/NH", "/FO", "CSV"])
            .creation_flags(CREATE_NO_WINDOW)
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(name))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("pgrep")
            .args(["-f", name])
            .stderr(Stdio::null())
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false)
    }
}

/// install_dir 存在才作为子进程工作目录；不存在则不设置（兜底，避免 spawn 直接失败）。
fn apply_cwd(command: &mut Command, install_dir: &str) {
    if std::path::Path::new(install_dir).is_dir() {
        command.current_dir(install_dir);
    }
}

/// 按平台选择 shell 并以分离方式启动命令，丢弃全部输出。
fn spawn_detached(cwd: &str, cmd: &str) -> std::io::Result<Child> {
    let mut command = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    apply_cwd(&mut command, cwd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 让被拉起的 shell 自成进程组（pgid == 其自身 pid）。stop 时用
        // `kill -KILL -<pgid>` 才能把 shell 及其孙进程（java/nginx 等）一锅端，
        // 否则只能杀 shell、java 变孤儿继续占用端口（P1-3）。
        command.process_group(0);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// 按平台选择 shell 并以分离方式启动命令，stdout/stderr 合并写入 `log_path`。
fn spawn_to_log(cwd: &str, cmd: &str, log_path: &str) -> std::io::Result<Child> {
    if let Some(parent) = std::path::Path::new(log_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = File::create(log_path)?;
    let stderr = file.try_clone()?;
    let mut command = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    apply_cwd(&mut command, cwd);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(stderr))
        .spawn()
}
