use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// 预置 JRE 的 home 目录：**启动 exe 同级**下的相对路径（绿色交付，
/// 免安装、不改系统环境变量）。在哪里启动，对应的目录就是哪里。
/// `relative` 来自 ServiceDef.javaHome 声明（如 "resource/jre"）。
/// 找不到返回 None（dev 机没有预置 → 调用方回退 PATH java）。
pub fn bundled_java_home(relative: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let home = dir.join(relative);
    home.is_dir().then_some(home)
}

/// run_cmd 中 `{java}` 占位符解析：优先预置 JRE 的 java 绝对路径，
/// 预置缺失回退 `java`（PATH 语义，dev 机开发不受影响）。
/// 检测（environment.rs）与启动（这里）使用同一解析，保证同源。
pub fn resolve_java_placeholder(cmd: &str) -> String {
    if !cmd.contains("{java}") {
        return cmd.to_string();
    }
    let java = bundled_java_home("resource/jre")
        .and_then(|home| {
            let bin = if cfg!(target_os = "windows") {
                home.join("bin").join("java.exe")
            } else {
                home.join("bin").join("java")
            };
            bin.exists().then(|| bin.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "java".to_string());
    cmd.replace("{java}", &java)
}

/// 进程管理器：以 shell 启动本地服务，并记录子进程句柄以便停止。
pub struct ProcManager {
    children: Mutex<HashMap<String, Child>>,
    /// 用户显式 stop 过的服务：保活看门狗跳过，直到下一次显式 start。
    suppressed: Mutex<HashMap<String, ()>>,
    /// 平台至少成功拉起过一次的服务：保活只管自己拉起过的，
    /// 平台没碰过的外部服务（detected）不由看门狗接管。
    ever_started: Mutex<HashMap<String, ()>>,
}

impl ProcManager {
    pub fn new() -> Self {
        Self {
            children: Mutex::new(HashMap::new()),
            suppressed: Mutex::new(HashMap::new()),
            ever_started: Mutex::new(HashMap::new()),
        }
    }

    /// 该服务是否被用户显式停止过（保活看门狗据此跳过）。
    pub fn is_suppressed(&self, id: &str) -> bool {
        self.suppressed.lock().unwrap().contains_key(id)
    }

    /// 保活看门狗的候选：平台拉起过、当前不在跑、且未被显式停止的服务 id。
    /// 走 `is_running`（内部 try_wait 会回收死进程句柄），确保「崩了但未回收」
    /// 的服务也能进入重启名单。
    pub fn keepalive_candidates(&self) -> Vec<String> {
        let ever: Vec<String> = self
            .ever_started
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        ever.into_iter()
            .filter(|id| !self.is_suppressed(id) && !self.is_running(id))
            .collect()
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
    /// `{java}` 占位符在此统一解析为预置 JRE（exe 同级 resource/jre）——
    /// 检测与启动同源，绿色交付免安装。
    pub fn start(&self, id: &str, install_dir: &str, run_cmd: &str) -> std::io::Result<()> {
        let mut map = self.children.lock().unwrap();
        if map.contains_key(id) {
            return Ok(());
        }
        let resolved = resolve_java_placeholder(run_cmd);
        let child = spawn_detached(install_dir, &resolved)?;
        map.insert(id.to_string(), child);
        // 显式启动解除保活抑制；并记录「平台拉起过」供看门狗接管
        self.suppressed.lock().unwrap().remove(id);
        self.ever_started
            .lock()
            .unwrap()
            .insert(id.to_string(), ());
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
            // 显式停止：保活看门狗不再自动拉起，直到下一次显式 start
            self.suppressed
                .lock()
                .unwrap()
                .insert(id.to_string(), ());
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

/// 本机命令行匹配给定名字的全部 PID（全命令行 substring 匹配，best-effort）。
///
/// 这是「进程是事实来源」检测模型的入口：先按进程名定位 PID，再用
/// [`listening_ports`] 反查它们实际监听的端口——**平台不规定端口，端口由进程反查得出**。
/// 注意：pattern 应使用运行器特征（如 `ng serve`、`dfy-server.jar`、`edgecd-daemon`），
/// 不要用项目路径——IDE 语言服务进程的命令行里常带全部项目路径，会误命中。
pub fn process_pids(name: &str) -> Vec<u32> {
    process_pids_inner(name, true)
}

/// 仅按进程镜像名匹配（跳过命令行兜底）。
///
/// 无端口的桌面客户端（TauriApp）必须走这里：主程序 exe 镜像名命中即可信。
/// 命令行兜底是全进程子串匹配——查询自身的命令行里就带着目标名，且 Windows
/// 部署机上任何命令行含该名字的进程（如打开过项目目录的 IDE）都会误命中，
/// 导致「没安装/没运行」被误报成「检测到在运行」（2026-10-14 Windows 实测踩坑）。
pub fn process_pids_image_only(name: &str) -> Vec<u32> {
    process_pids_inner(name, false)
}

fn process_pids_inner(name: &str, allow_cli_fallback: bool) -> Vec<u32> {
    if name.trim().is_empty() {
        return vec![];
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let image_match = |image: &str| {
            Command::new("tasklist")
                .args(["/FI", &format!("IMAGENAME eq {image}"), "/NH", "/FO", "CSV"])
                .creation_flags(CREATE_NO_WINDOW)
                .stderr(Stdio::null())
                .output()
                .map(|o| {
                    // tasklist CSV 每行: "镜像名","PID","会话名",...
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter_map(|l| l.split('"').nth(3))
                        .filter_map(|pid| pid.parse().ok())
                        .collect::<Vec<u32>>()
                })
                .unwrap_or_default()
        };
        // 先按镜像名匹配（含/不含 .exe 各试一次）；
        // 都未命中再按命令行子串匹配——Erlang 系应用（VerneMQ）的实际进程是
        // beam.smp.exe，与 RabbitMQ 等共用镜像名，只能靠命令行里的应用标识区分。
        let mut pids = image_match(name);
        if pids.is_empty() && !name.ends_with(".exe") {
            pids = image_match(&format!("{name}.exe"));
        }
        if allow_cli_fallback && pids.is_empty() {
            pids = windows_commandline_pids(name);
        }
        pids
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Unix 只有 pgrep -f 一条路，镜像名/命令行之分不存在
        let _ = allow_cli_fallback;
        let pids: Vec<u32> = Command::new("pgrep")
            .args(["-f", name])
            .stderr(Stdio::null())
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        filter_tool_noise(pids)
    }
}

/// Windows：按命令行子串匹配进程（PowerShell Get-CimInstance，比 wmic 现代）。
///
/// 用途：VerneMQ 这类 Erlang 应用，进程镜像名（beam.smp.exe）与 RabbitMQ 等共用，
/// 镜像名匹配会误命中；其命令行里含应用标识（如 `...\vernemq\...`），按命令行匹配才准。
/// 注意：查询本身约 1-2s，仅在镜像名匹配落空时作为兜底调用，不进高频轮询路径。
///
/// 必须排除自身 PID：powershell 进程自己的命令行含 `-Command "...token..."`
/// 原文，不排除就会匹配到自己，返回一个注定已退出的「幽灵 PID」，
/// 把「未运行」误判成「检测到在运行」（2026-10-14 Windows 实测踩坑）。
#[cfg(target_os = "windows")]
fn windows_commandline_pids(token: &str) -> Vec<u32> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // token 来自 services.toml（本机受信配置），无注入面；like 通配符转义 % 即可
    let pattern = format!("*{}*", token.replace('%', ""));
    let self_pid = std::process::id();
    let ps = format!(
        "Get-CimInstance Win32_Process | Where-Object {{ $_.ProcessId -ne {self_pid} -and $_.CommandLine -like '{pattern}' }} | Select-Object -ExpandProperty ProcessId"
    );
    Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .stderr(Stdio::null())
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// 进程匹配后剔除「工具噪音」PID。
///
/// IDE / agent 宿主（Trae、WorkBuddy、CodeBuddy、Cursor…）的命令行里嵌着全部项目路径，
/// 按命令行匹配进程名时会误命中——例如把 WorkBuddy 自己监听的端口归给某个服务。
/// 规则：可执行名是常见 shell、Electron 容器或含 IDE/Helper 标识的剔除；
/// `ps` 不可用时保守保留（只过滤确认是噪音的，宁多勿错杀真实服务）。
#[cfg(not(target_os = "windows"))]
fn filter_tool_noise(pids: Vec<u32>) -> Vec<u32> {
    if pids.is_empty() {
        return pids;
    }
    let list = pids
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let Ok(out) = Command::new("ps")
        .args(["-o", "pid=,comm=", "-p", &list])
        .stderr(Stdio::null())
        .output()
    else {
        return pids;
    };
    let mut comms: HashMap<u32, String> = HashMap::new();
    for l in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = l.splitn(2, char::is_whitespace);
        if let (Some(p), Some(c)) = (parts.next(), parts.next()) {
            if let Ok(pid) = p.trim().parse::<u32>() {
                comms.insert(pid, c.trim().to_string());
            }
        }
    }
    pids.into_iter()
        .filter(|p| match comms.get(p) {
            Some(c) => !is_tool_noise(c),
            None => true, // 查不到可执行名的保守保留
        })
        .collect()
}

/// 可执行名是否属于工具噪音（shell / GUI 容器 / IDE·agent 宿主）。
#[cfg(not(target_os = "windows"))]
fn is_tool_noise(comm: &str) -> bool {
    let base = comm.rsplit('/').next().unwrap_or(comm);
    const SHELLS: [&str; 6] = ["zsh", "bash", "sh", "fish", "pwsh", "powershell"];
    if SHELLS.contains(&base) || base == "Electron" {
        return true;
    }
    // macOS comm 可能被截断（MAXCOMLEN），故用包含匹配且只匹配噪音侧
    ["Helper", "Trae", "CodeBuddy", "WorkBuddy", "Cursor", "LanguageServer"]
        .iter()
        .any(|k| base.contains(k))
}

/// 反查给定 PID 集合正在 LISTEN 的本地端口（去重升序）。
///
/// 「服务实际监听什么端口就检测什么端口」的实现：Unix/macOS 走
/// `lsof -nP -iTCP -sTCP:LISTEN -a -p <pids>`，Windows 走 `netstat -ano`。
/// Best-effort：命令不可用（如部分 Linux 无 lsof）时返回空，调用方自行回退。
pub fn listening_ports(pids: &[u32]) -> Vec<u16> {
    if pids.is_empty() {
        return vec![];
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let pid_set: Vec<String> = pids.iter().map(|p| p.to_string()).collect();
        Command::new("netstat")
            .args(["-ano", "-p", "tcp"])
            .creation_flags(0x0800_0000)
            .stderr(Stdio::null())
            .output()
            .map(|o| parse_netstat_ports(&String::from_utf8_lossy(&o.stdout), &pid_set))
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let pid_list = pids
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",");
        // +c0：不截断 COMMAND 列，供解析层做工具噪音过滤
        // （pid 级过滤依赖 ps，ps 不可用的环境下这里是最后一道防线）。
        Command::new("lsof")
            .args(["-nP", "+c0", "-iTCP", "-sTCP:LISTEN", "-a", "-p", &pid_list])
            .stderr(Stdio::null())
            .output()
            .map(|o| parse_lsof_ports(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }
}

/// 解析 `lsof -iTCP -sTCP:LISTEN` 输出，提取 NAME 列（如 `*:4200`、`127.0.0.1:48085`、
/// `[::1]:4200`）中的端口，去重升序。纯函数便于测试。
///
/// 行尾可能带 `(LISTEN)` 后缀，因此从右往左找第一个「冒号后是数字」的 token。
/// COMMAND 列（首列）命中工具噪音（shell / Electron 容器 / IDE·agent 宿主）的行
/// 整行丢弃——pid 级过滤（`filter_tool_noise`）依赖 `ps`，ps 不可用时由这里兜底，
/// 避免 WorkBuddy/Trae 这类命令行嵌项目路径的宿主进程把自家端口反查给服务。
///
/// 仅非 Windows 平台编译：本函数只被 `listening_ports` 的非 Windows 分支调用，
/// 且其内部依赖同为非 Windows 专属的 `is_tool_noise`（漏加 cfg 曾导致
/// Windows CI E0425：cannot find function `is_tool_noise`）。
#[cfg(not(target_os = "windows"))]
fn parse_lsof_ports(text: &str) -> Vec<u16> {
    let mut ports: Vec<u16> = text
        .lines()
        .skip(1) // 跳过表头 COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
        .filter(|l| {
            l.split_whitespace()
                .next()
                .map(|cmd| !is_tool_noise(cmd))
                .unwrap_or(false)
        })
        .filter_map(|l| {
            l.split_whitespace()
                .rev()
                .find_map(|t| t.rsplit_once(':')?.1.parse().ok())
        })
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// 解析 Windows `netstat -ano -p tcp` 输出：取 LISTENING 行，本地地址列的端口，
/// 且 PID 在给定集合内。纯函数便于测试。
#[cfg(target_os = "windows")]
fn parse_netstat_ports(text: &str, pids: &[String]) -> Vec<u16> {
    let mut ports: Vec<u16> = text
        .lines()
        .filter(|l| l.contains("LISTENING"))
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            // Proto Local Foreign State PID
            if cols.len() < 5 || !pids.contains(&cols[4].to_string()) {
                return None;
            }
            cols[1].rsplit_once(':')?.1.parse().ok()
        })
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
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

#[cfg(test)]
mod tests {
    use super::*;

    // lsof 输出解析仅存在于非 Windows 编译路径，测试同步加 cfg
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn parse_lsof_ports_extracts_listening_ports() {
        let sample = "COMMAND   PID  USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME\n\
                      java    35668  lc   100u  IPv6 0xabc      0t0  TCP *:48085 (LISTEN)\n\
                      node    67613  lc   101u  IPv6 0xdef      0t0  TCP [::1]:4200 (LISTEN)\n\
                      edgecd  88147  lc   102u  IPv4 0x123      0t0  TCP 127.0.0.1:8084 (LISTEN)\n\
                      edgecd  88147  lc   103u  IPv4 0x124      0t0  TCP 127.0.0.1:8084 (LISTEN)\n";
        assert_eq!(parse_lsof_ports(sample), vec![4200, 8084, 48085]);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn parse_lsof_ports_ignores_header_and_empty() {
        let expected: Vec<u16> = vec![];
        assert_eq!(
            parse_lsof_ports("COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\n"),
            expected
        );
        assert_eq!(parse_lsof_ports(""), expected);
    }

    /// 工具噪音进程（Electron 宿主 / shell，命令行嵌项目路径被 pgrep 误命中）
    /// 的 LISTEN 行必须在解析层被整行丢弃——ps 不可用时这是唯一防线。
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn parse_lsof_ports_drops_tool_noise_rows() {
        let sample = "COMMAND   PID  USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME\n\
                      Electron 75875 lc   22u  IPv4 0xb02      0t0  TCP 127.0.0.1:50052 (LISTEN)\n\
                      zsh      26878 lc   23u  IPv4 0xb03      0t0  TCP 127.0.0.1:52792 (LISTEN)\n\
                      node     28040 lc   24u  IPv6 0xb04      0t0  TCP *:80 (LISTEN)\n";
        assert_eq!(parse_lsof_ports(sample), vec![80]);
    }

    /// 保活的抑制/候选语义：从未拉起 → 不是候选；在跑 → 不是候选；
    /// 显式 stop → 抑制且不是候选；显式 start 解除抑制；进程退出 → 成为候选。
    #[cfg(unix)]
    #[test]
    fn keepalive_suppression_and_candidates() {
        use std::time::Duration;

        let pm = ProcManager::new();
        let ids = || pm.keepalive_candidates();
        // 平台从未拉起过：不是候选（看门狗不接管外部服务）
        assert!(!ids().iter().any(|id| id == "svc-test"));

        pm.start("svc-test", "/tmp", "sleep 30").unwrap();
        assert!(pm.is_running("svc-test"));
        assert!(!ids().iter().any(|id| id == "svc-test"));

        // 显式 stop：进程被杀 + 进入抑制名单
        pm.stop("svc-test").unwrap();
        assert!(pm.is_suppressed("svc-test"));
        assert!(!pm.is_running("svc-test"));
        assert!(!ids().iter().any(|id| id == "svc-test"));

        // 显式 start 解除抑制
        pm.start("svc-test", "/tmp", "sleep 30").unwrap();
        assert!(!pm.is_suppressed("svc-test"));

        // 换一个立即退出的进程：退出后应重新成为候选（保活该重启它）
        pm.start("svc-exit", "/tmp", "true").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(ids().iter().any(|id| id == "svc-exit"));

        pm.stop("svc-test").unwrap();
        pm.stop("svc-exit").unwrap();
    }
}
