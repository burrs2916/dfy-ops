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
    pub fn is_running(&self, id: &str) -> bool {
        self.children.lock().unwrap().contains_key(id)
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

    /// 停止服务（结束记录中的子进程）。
    ///
    /// Windows 注意：服务经 `cmd /C` 启动，`kill()` 只能杀掉 cmd.exe，
    /// java/nginx 等孙进程会存活——必须用 `taskkill /T /F` 连进程树一起结束。
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
                    .status();
            }
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
