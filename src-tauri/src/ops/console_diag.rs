//! 边缘控制台链路诊断：iframe 空白问题的全量故障点自检。
//!
//! 背景：控制台 iframe 走 Nginx 80 通道（/console/ 静态托管 + /api/ 反代
//! 8084），该链路任何一环断了在用户眼里都是同一现象——iframe 空白。
//! 本模块把所有可能的故障点逐项探测并写进 debug.log（phase=console_diag），
//! 用户无需回答任何问题，日志自解释：每次打开「边缘控制台」页或独立窗口
//! 时自动触发，把日志发回来即可定位。
//!
//! 检查项（按链路顺序）：
//! 1. 控制台产物：{edge安装目录}/web-console/index.html 是否就位；
//! 2. Nginx 安装：C:/services/nginx/nginx.exe 是否存在；
//! 3. nginx.conf 接入：dfy-ops 标记块是否存在、alias 与产物目录是否一致、
//!    是否存在重复的 `location /api/` 前缀（重复会让 nginx -t 失败、接入回滚）；
//! 4. Nginx 进程与 80 端口；
//! 5. HTTP 探测：80 通道 /console/ 与 /console/index.html 的状态行；
//!    8084 网关 /console/（预期 404，作对照）与 /api（网关活性）；
//! 6. 综合结论：给出最可能原因与对应动作。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::debug_log;

fn log(level: &str, evt: &str, detail: impl Into<String>) {
    debug_log::entry(debug_log::APP_SCOPE, level, "console_diag", evt, &detail.into());
}

/// 最小 HTTP GET：只取响应状态行。连接失败/超时/非 HTTP 响应都给出明确文字。
fn http_status(port: u16, path: &str) -> String {
    let addr: SocketAddr = match format!("127.0.0.1:{port}").parse() {
        Ok(a) => a,
        Err(_) => return "地址解析失败（不应发生）".into(),
    };
    match TcpStream::connect_timeout(&addr, Duration::from_millis(1500)) {
        Err(e) => format!("连接失败：{e}（端口无监听或被防火墙拦截）"),
        Ok(mut s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
            let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
            if let Err(e) = s.write_all(req.as_bytes()) {
                return format!("请求发送失败：{e}");
            }
            let mut buf = [0u8; 512];
            let n = s.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]);
            let status = head.lines().next().unwrap_or("").trim().to_string();
            if status.is_empty() {
                "无响应（连接建立但未返回 HTTP 状态行）".into()
            } else {
                status
            }
        }
    }
}

/// 检查控制台产物，返回 index.html 是否就位。
fn check_artifact(install_dir: &str) -> bool {
    let dir = std::path::Path::new(install_dir).join("web-console");
    let index = dir.join("index.html");
    if index.is_file() {
        let (n, bytes, entry) = std::fs::read_dir(&dir)
            .map(|rd| {
                let mut n = 0usize;
                let mut bytes = 0u64;
                let mut entry = "?".to_string();
                for e in rd.flatten() {
                    if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        n += 1;
                        bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.starts_with("main-") && name.ends_with(".js") {
                            entry = name;
                        }
                    }
                }
                (n, bytes, entry)
            })
            .unwrap_or((0, 0, "?".into()));
        log(
            "INFO",
            "artifact",
            format!(
                "控制台产物就位：{}（{n} 个文件，{bytes} 字节，入口 {entry}）",
                dir.display()
            ),
        );
        true
    } else if dir.is_dir() {
        log(
            "ERROR",
            "artifact",
            format!(
                "产物目录存在但缺 index.html：{}（部署不完整——到修复中心重装边缘计算）",
                dir.display()
            ),
        );
        false
    } else {
        log(
            "ERROR",
            "artifact",
            format!(
                "控制台产物未部署：{} 不存在（安装管线未复制，或安装时资源目录/内置产物缺 web-console/——到修复中心重装边缘计算）",
                dir.display()
            ),
        );
        false
    }
}

/// 检查 Nginx 安装与配置接入，返回（nginx 已装，接入块存在）。
#[cfg(target_os = "windows")]
fn check_nginx(install_dir: &str) -> (bool, bool) {
    let nginx_dir = std::path::Path::new("C:/services/nginx");
    if !nginx_dir.join("nginx.exe").is_file() {
        log(
            "ERROR",
            "nginx_installed",
            "未安装 Nginx（C:/services/nginx/nginx.exe 不存在）——控制台 80 通道不可用，iframe 必空白。到修复中心安装 Nginx 后重装边缘计算（重新触发接入）".into(),
        );
        return (false, false);
    }
    log("INFO", "nginx_installed", "Nginx 已安装（C:/services/nginx）");
    let conf = nginx_dir.join("conf").join("nginx.conf");
    let s = match std::fs::read_to_string(&conf) {
        Ok(s) => s,
        Err(e) => {
            log("ERROR", "nginx_conf", format!("读取 {} 失败：{e}", conf.display()));
            return (true, false);
        }
    };
    const BEGIN: &str = "# >>> dfy-ops edge-console (auto) >>>";
    const END: &str = "# <<< dfy-ops edge-console (auto) <<<";
    if !s.contains(BEGIN) {
        log(
            "ERROR",
            "nginx_conf",
            "nginx.conf 无 dfy-ops 接入标记块——/console/ 与 /api/ 未接入 Nginx（装边缘计算时接入失败或被人工清理）。重装边缘计算会重新接入".into(),
        );
        return (true, false);
    }
    let block = s
        .split(BEGIN)
        .nth(1)
        .and_then(|r| r.split(END).next())
        .unwrap_or("");
    let alias = block
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("alias "))
        .unwrap_or("")
        .to_string();
    let proxy = block
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("proxy_pass "))
        .unwrap_or("")
        .to_string();
    log(
        "INFO",
        "nginx_conf",
        format!("接入块存在：{alias}；{proxy}"),
    );
    // alias 与产物目录一致性：产物热更/换目录后 nginx 仍指旧路径
    let expect = format!("alias {}/;", std::path::Path::new(install_dir).join("web-console").to_string_lossy().replace('\\', "/"));
    if alias == expect {
        log("INFO", "nginx_alias", "alias 指向与产物目录一致");
    } else {
        log(
            "WARN",
            "nginx_alias",
            format!(
                "alias 与产物目录不一致！nginx 指向 [{alias}]，实际产物 [{}]——重装边缘计算可刷新接入块",
                std::path::Path::new(install_dir).join("web-console").display()
            ),
        );
    }
    // 重复前缀 location 冲突：Nginx 拒绝同前缀重复 location（nginx -t 失败 →
    // 接入自动回滚 → /console/ 与 /api/ 都不生效）
    let api_locs = s
        .lines()
        .filter(|l| l.trim().starts_with("location /api/"))
        .count();
    if api_locs > 1 {
        log(
            "ERROR",
            "nginx_conf_conflict",
            format!("nginx.conf 存在 {api_locs} 个 `location /api/` 前缀——Nginx 不允许重复前缀 location，校验失败会导致接入回滚（控制台网关请求全部 404）。需人工合并两处 /api/ 配置"),
        );
    }
    (true, true)
}

#[cfg(not(target_os = "windows"))]
fn check_nginx(_install_dir: &str) -> (bool, bool) {
    log("INFO", "nginx", "非 Windows 环境，跳过 Nginx 检查");
    (false, false)
}

/// 执行全量诊断并写 debug.log。触发点：打开「边缘控制台」页 / 独立窗口。
pub fn diagnose(install_dir: &str, http_port: &str, cloud_api: &str) {
    log(
        "INFO",
        "start",
        format!("边缘控制台链路诊断开始（打开控制台时自动触发）。安装目录={install_dir}，网关端口={http_port}，云端地址={}", if cloud_api.is_empty() { "（未配置）" } else { cloud_api }),
    );
    if cloud_api.is_empty() {
        log(
            "WARN",
            "external_config",
            "upstreamServerUrl 未配置——控制台 ?cloudApi= 注入为空，登录页查租户/云端业务将失败（配置中心「边缘计算」卡设置上游地址）",
        );
    }
    let artifact_ok = check_artifact(install_dir);
    let (nginx_installed, integrated) = check_nginx(install_dir);

    #[cfg(target_os = "windows")]
    {
        let running = crate::api::handlers_repair::image_running("nginx.exe");
        log(
            if running { "INFO" } else { "ERROR" },
            "nginx_process",
            if running {
                "nginx.exe 进程在运行".into()
            } else {
                "nginx.exe 进程未运行——80 通道无服务，iframe 必空白。到服务列表启动 Nginx".into()
            },
        );
    }

    // HTTP 探测（无论前面结论如何都做，交叉验证）
    let p80 = http_status(80, "/console/");
    log(
        "INFO",
        "probe_80_console",
        format!("GET http://127.0.0.1/console/ → {p80}"),
    );
    let p80i = http_status(80, "/console/index.html");
    log(
        "INFO",
        "probe_80_index",
        format!("GET http://127.0.0.1/console/index.html → {p80i}"),
    );
    let port: u16 = http_port.parse().unwrap_or(8084);
    let p8084c = http_status(port, "/console/");
    log(
        "INFO",
        "probe_gateway_console",
        format!("GET http://127.0.0.1:{http_port}/console/ → {p8084c}（对照：网关不托管静态页，404/405 属预期，不是问题）"),
    );
    let p8084a = http_status(port, "/api/health");
    log(
        "INFO",
        "probe_gateway_api",
        format!("GET http://127.0.0.1:{http_port}/api/health → {p8084a}（状态码本身不重要，连接结果说明网关 HTTP 是否存活）"),
    );

    // 综合结论
    if !artifact_ok {
        log("ERROR", "conclusion", "最可能原因：控制台产物未部署 → 修复中心重装边缘计算");
    } else if !nginx_installed {
        log("ERROR", "conclusion", "最可能原因：Nginx 未安装 → 修复中心安装 Nginx 后重装边缘计算");
    } else if !integrated {
        log("ERROR", "conclusion", "最可能原因：Nginx 未接入 /console/ → 重装边缘计算重新触发接入；若重装仍无标记块，检查 nginx.conf 权限/路径");
    } else if p80.starts_with("HTTP/1.1 200") || p80.starts_with("HTTP/1.0 200") {
        log(
            "INFO",
            "conclusion",
            "Nginx 80 通道 /console/ 返回 200——服务端链路正常；若 iframe 仍空白，问题在浏览器侧（F12 看 Console/Network：JS 报错、资源 404、CSP 拦截），把 F12 截图/日志发回",
        );
    } else {
        log(
            "ERROR",
            "conclusion",
            format!("80 通道 /console/ 未返回 200（{p80}）——结合上方 nginx_process/nginx_conf 结论处理"),
        );
    }
    log("INFO", "end", "边缘控制台链路诊断结束");
}
