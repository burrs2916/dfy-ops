//! dfy-backend 启动前依赖预检（MySQL / Redis / MQTT broker 连通性）。
//!
//! 背景（2026-11-02 实测）：后端自动启动失败只报「未在超时内就绪」，用户无法
//! 区分是 Java 挂了还是依赖连不通。启动前读外部配置里的依赖地址做 TCP 预检，
//! 连不上直接点名「哪个依赖、哪个地址」——不再猜谜。
//!
//! 预检是 TCP 级「能否联通」：不校验账号密码/库表（那是后端起来之后的报错，
//! 日志里看得到）。无外部配置文件时跳过（后端用 jar 内置默认值）。

use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

/// 单个依赖的预检结果。
#[derive(Debug, Clone)]
pub struct DepCheck {
    /// 依赖名（MySQL / Redis / MQTT）
    pub name: &'static str,
    /// 探测的 host:port
    pub addr: String,
    pub ok: bool,
    /// 失败原因（连不上时的 io 错误摘要；成功时为「连通」）
    pub detail: String,
}

/// 单次 TCP 探测超时。
const TCP_TIMEOUT: Duration = Duration::from_secs(2);

/// 从 url 提取 host:port：支持 `jdbc:mysql://host:port/db?…` 与 `tcp://host:port`。
/// 解析不出返回 None（该依赖跳过预检）。
fn parse_host_port(url: &str, default_port: u16) -> Option<(String, u16)> {
    let rest = url.split("://").nth(1)?;
    let host_part = rest.split(['/', '?']).next()?.trim();
    if host_part.is_empty() {
        return None;
    }
    Some(match host_part.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(default_port)),
        None => (host_part.to_string(), default_port),
    })
}

/// TCP 连通性探测（2s 超时；域名先解析，逐地址尝试）。
fn tcp_ok(host: &str, port: u16) -> Result<(), String> {
    use std::net::ToSocketAddrs;
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("域名解析失败：{e}"))?
        .collect();
    let mut last = "无可用地址".to_string();
    for a in addrs {
        match TcpStream::connect_timeout(&a, TCP_TIMEOUT) {
            Ok(_) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

/// dfy-backend 启动前依赖预检：读外部配置，逐项 TCP 探测。
/// 返回空 = 没有外部配置文件（调用方跳过预检，后端用 jar 内置默认值）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn check_backend(install_dir: &str) -> Vec<DepCheck> {
    let cfg = match crate::ops::external_config::read(Path::new(install_dir)) {
        Ok(Some(c)) => c,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    // MySQL（master 库连通是硬依赖；slave 惰性加载不探）
    if let Some((h, p)) = parse_host_port(&cfg.mysql_master_url, 3306) {
        let ok = tcp_ok(&h, p);
        out.push(DepCheck {
            name: "MySQL",
            addr: format!("{h}:{p}"),
            ok: ok.is_ok(),
            detail: ok.err().unwrap_or_else(|| "连通".into()),
        });
    }
    // Redis
    if !cfg.redis_host.trim().is_empty() {
        let h = cfg.redis_host.trim();
        let p: u16 = cfg.redis_port.trim().parse().unwrap_or(6379);
        let ok = tcp_ok(h, p);
        out.push(DepCheck {
            name: "Redis",
            addr: format!("{h}:{p}"),
            ok: ok.is_ok(),
            detail: ok.err().unwrap_or_else(|| "连通".into()),
        });
    }
    // MQTT broker（VerneMQ）
    if let Some((h, p)) = parse_host_port(&cfg.vernemq_host_url, 1883) {
        let ok = tcp_ok(&h, p);
        out.push(DepCheck {
            name: "MQTT",
            addr: format!("{h}:{p}"),
            ok: ok.is_ok(),
            detail: ok.err().unwrap_or_else(|| "连通".into()),
        });
    }
    out
}

/// 启动前修复历史模板的 MySQL URL 缺陷（幂等，与 nginx pid 位置修复同理）。
///
/// 背景（2026-11-02 实测）：v0.2.44 及更早平台生成的外部配置，master URL 是
/// `useSSL=false` 且缺 `allowPublicKeyRetrieval`——MySQL 8 默认
/// caching_sha2_password 账号在非 SSL 连接下不允许客户端取公钥，Druid 建连
/// 必报 `Public Key Retrieval is not allowed`，后端启动即退。升级到 v0.2.45+
/// 后旧配置文件不会自动重写，因此在每次启动前就地补上该参数。
/// 只补参数，不动账号/库名/地址；已含参数、无 useSSL=false 或注释行都不改。
/// 返回被修复的 URL 列表（空 = 无需修复；无配置文件也返回空）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn repair_legacy_mysql_urls(install_dir: &str) -> Result<Vec<String>, String> {
    let path = crate::ops::external_config::config_path(Path::new(install_dir));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let mut changed: Vec<String> = Vec::new();
    let mut out_lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        // 只动未注释的 datasource url 行（注释行 trim 后以 # 开头，天然排除）
        let hit = trimmed.starts_with("url:")
            && line.contains("jdbc:mysql://")
            && line.contains("useSSL=false")
            && !line.contains("allowPublicKeyRetrieval");
        if !hit {
            out_lines.push(line.to_string());
            continue;
        }
        // 剥行尾注释再追加参数（URL 值本身不含 '#'，注释必须保持在参数之后）
        let (body, comment) = match line.find('#') {
            Some(i) => (line[..i].trim_end(), Some(line[i..].to_string())),
            None => (line.trim_end(), None),
        };
        changed.push(trimmed.to_string());
        let mut new_line = format!("{body}&allowPublicKeyRetrieval=true");
        if let Some(c) = comment {
            new_line.push(' ');
            new_line.push_str(&c);
        }
        out_lines.push(new_line);
    }
    if changed.is_empty() {
        return Ok(changed);
    }
    // 保持原文件行尾风格（Windows 生成件多为 CRLF）
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = out_lines.join(eol);
    if text.ends_with('\n') {
        out.push_str(eol);
    }
    std::fs::write(&path, &out).map_err(|e| format!("写回外部配置失败：{e}"))?;
    Ok(changed)
}

/// jar 文件名标注的 JDK 版本与实际启动 JDK 不一致时给出警告文案。
/// 例：dfy-server-2.3.0-jdk8-SNAPSHOT.jar 用 JDK17 启动——若后端使用了
/// JDK9+ 移除的 API（如 JAXB），会启动即退。启发式：文件名含 jdk8/jdk-8。
/// java 走 PATH（解析不出路径）时无从判断版本，不误报。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn jdk_mismatch_warning(jar_path: &str, java_path: &str) -> Option<String> {
    let jar = jar_path.replace('\\', "/").to_lowercase();
    let jar_name = jar.rsplit('/').next().unwrap_or(&jar);
    if !(jar_name.contains("jdk8") || jar_name.contains("jdk-8")) {
        return None;
    }
    let jp = java_path.replace('\\', "/").to_lowercase();
    let java_name = jp.rsplit('/').next().unwrap_or(&jp);
    if java_name == "java" {
        return None;
    }
    let jdk17 = jp.contains("jdk-17") || jp.contains("jdk17") || jp.contains("jdk_17");
    jdk17.then(|| {
        format!(
            "注意：jar 文件名标注 jdk8（{jar_name}），但实际用 JDK17 启动（{java_path}）。\
             若后端使用了 JDK9+ 移除的 API 会启动即退——请确认交付要求的 JDK 版本，\
             必要时在资源目录放置对应版本 JDK 后重装"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_port_handles_jdbc_and_tcp_urls() {
        assert_eq!(
            parse_host_port(
                "jdbc:mysql://127.0.0.1:3306/dfy_base_db?useSSL=false&characterEncoding=utf8",
                3306
            ),
            Some(("127.0.0.1".into(), 3306))
        );
        assert_eq!(
            parse_host_port("tcp://192.168.56.10:18830", 1883),
            Some(("192.168.56.10".into(), 18830))
        );
        // 无端口 → 默认端口
        assert_eq!(
            parse_host_port("jdbc:mysql://db.internal/dfy", 3306),
            Some(("db.internal".into(), 3306))
        );
        // 解析不出 → None
        assert_eq!(parse_host_port("localhost:3306", 3306), None);
        assert_eq!(parse_host_port("", 3306), None);
    }

    #[test]
    fn tcp_ok_reports_unreachable_clearly() {
        // 回环上几乎必然无监听的高端口：报错而非 panic，错误信息可读
        let r = tcp_ok("127.0.0.1", 59999);
        assert!(r.is_err());
        assert!(!r.err().unwrap().is_empty());
    }

    #[test]
    fn repair_legacy_mysql_urls_appends_param_idempotently() {
        let dir = std::env::temp_dir().join(format!(
            "dfyops-preflight-repair-{}-{}",
            std::process::id(),
            line!()
        ));
        let cfg_dir = dir.join("config");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        let path = cfg_dir.join("application-local.yaml");
        std::fs::write(
            &path,
            "spring:\n  datasource:\n    dynamic:\n      datasource:\n        master:\n          # url: jdbc:mysql://old/useSSL=false 注释行不动\n          url: jdbc:mysql://127.0.0.1:3306/dfy_base_db?useSSL=false&characterEncoding=utf8&serverTimezone=Asia/Shanghai\n          username: root\n        slave:\n          url: jdbc:mysql://127.0.0.1:3306/dfy_base_db?characterEncoding=utf8 # 无 useSSL 不动\n          username: root\n        already_ok:\n          url: jdbc:mysql://h/db?useSSL=false&allowPublicKeyRetrieval=true\n",
        )
        .unwrap();
        let changed = repair_legacy_mysql_urls(dir.to_str().unwrap()).unwrap();
        assert_eq!(changed.len(), 1, "只该修缺参数的那条 master URL");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("serverTimezone=Asia/Shanghai&allowPublicKeyRetrieval=true\n"));
        assert!(text.contains("# url: jdbc:mysql://old/useSSL=false 注释行不动"));
        assert!(text.contains("?characterEncoding=utf8 # 无 useSSL 不动"));
        // 幂等：第二次无改动、不重写
        assert!(repair_legacy_mysql_urls(dir.to_str().unwrap()).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn repair_keeps_trailing_comment_after_param() {
        let dir = std::env::temp_dir().join(format!(
            "dfyops-preflight-repair-cmt-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        let path = dir.join("config").join("application-local.yaml");
        std::fs::write(
            &path,
            "        master:\n          url: jdbc:mysql://127.0.0.1:3306/dfy_base_db?useSSL=false # 主库\r\n",
        )
        .unwrap();
        assert_eq!(repair_legacy_mysql_urls(dir.to_str().unwrap()).unwrap().len(), 1);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("useSSL=false&allowPublicKeyRetrieval=true # 主库\r\n"),
            "参数必须追加在注释前且保持 CRLF：{text}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn jdk_mismatch_warning_only_for_marked_jdk8_jars() {
        let jdk17 = "C:/services/jdk/jdk-17.0.20.1+1/bin/java.exe";
        assert!(jdk_mismatch_warning(
            "D:/res/dfy-server-2.3.0-jdk8-SNAPSHOT.jar",
            jdk17
        )
        .is_some());
        // 无 jdk8 标注 / 非 JDK17 / PATH java：不误报
        assert!(jdk_mismatch_warning(
            "D:/res/dfy-server-2.3.0-SNAPSHOT.jar",
            jdk17
        )
        .is_none());
        assert!(jdk_mismatch_warning(
            "D:/res/dfy-server-2.3.0-jdk8-SNAPSHOT.jar",
            "C:/services/jdk/jdk-8u411/bin/java.exe"
        )
        .is_none());
        assert!(jdk_mismatch_warning(
            "D:/res/dfy-server-2.3.0-jdk8-SNAPSHOT.jar",
            "java"
        )
        .is_none());
    }
}
