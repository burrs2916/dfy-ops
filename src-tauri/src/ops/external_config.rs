//! 云端 Java 服务外部配置（config/application-local.yaml）读写。
//!
//! 背景：dfy-boot-base 打包后配置内置于 jar，但部署环境常需修改端口/MySQL/Redis/
//! VerneMQ 连接。Spring Boot 2.4+ 自动加载 jar 同级 `config/application-*.yaml`
//! 且优先级高于 jar 内配置——外部文件存在即覆盖，删除即回退兜底默认值。
//!
//! 实现：逐行状态机（段落 key：server / master / slave / redis / verneMq，
//! 在整个多文档 yaml 中唯一）定位目标行，原地替换值并保留行尾注释。
//! 不引入 yaml 依赖——运维字段都是唯一标量 key，行级替换足够稳。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 可运维的配置字段（与 config/application-local.yaml 模板一一对应）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExternalConfig {
    #[serde(rename = "serverPort")]
    pub server_port: String,
    #[serde(rename = "mysqlMasterUrl")]
    pub mysql_master_url: String,
    #[serde(rename = "mysqlUsername")]
    pub mysql_username: String,
    #[serde(rename = "mysqlPassword")]
    pub mysql_password: String,
    #[serde(rename = "mysqlSlaveUrl")]
    pub mysql_slave_url: String,
    #[serde(rename = "redisHost")]
    pub redis_host: String,
    #[serde(rename = "redisPort")]
    pub redis_port: String,
    #[serde(rename = "redisPassword")]
    pub redis_password: String,
    #[serde(rename = "vernemqHostUrl")]
    pub vernemq_host_url: String,
    #[serde(rename = "vernemqUsername")]
    pub vernemq_username: String,
    #[serde(rename = "vernemqPassword")]
    pub vernemq_password: String,
}

/// 外部配置文件路径：`{install_dir}/config/application-local.yaml`。
pub fn config_path(install_dir: &Path) -> PathBuf {
    install_dir.join("config").join("application-local.yaml")
}

/// 当前段落（跨多文档 yaml 唯一的段落 key）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    Root,
    Server,
    Datasource,
    Master,
    Slave,
    Redis,
    VerneMq,
}

/// 根据 yaml 行内容推进段落状态机。
fn advance_section(sec: &mut Section, trimmed: &str, indent: usize) {
    match trimmed {
        "server:" => *sec = Section::Server,
        "datasource:" => *sec = Section::Datasource,
        "master:" => *sec = Section::Master,
        "slave:" => *sec = Section::Slave,
        "redis:" if indent <= 4 => *sec = Section::Redis, // spring.redis（缩进 2）
        "verneMq:" => *sec = Section::VerneMq,
        "iot:" | "spring:" | "dynamic:" => {}
        _ => {}
    }
}

/// 行是否为目标 key（当前段落下）。
fn matches_key(sec: Section, key: &str) -> Option<&'static str> {
    match (sec, key) {
        (Section::Server, "port") => Some("server_port"),
        (Section::Master, "url") => Some("mysql_master_url"),
        (Section::Master, "username") => Some("mysql_username"),
        (Section::Master, "password") => Some("mysql_password"),
        (Section::Slave, "url") => Some("mysql_slave_url"),
        (Section::Redis, "host") => Some("redis_host"),
        (Section::Redis, "port") => Some("redis_port"),
        (Section::Redis, "password") => Some("redis_password"),
        (Section::VerneMq, "hostUrl") => Some("vernemq_host_url"),
        (Section::VerneMq, "username") => Some("vernemq_username"),
        (Section::VerneMq, "password") => Some("vernemq_password"),
        _ => None,
    }
}

/// 剥离行尾注释（` #` 起始视为注释），返回 (主体, 注释含#)。
fn split_comment(line: &str) -> (&str, &str) {
    if let Some(pos) = line.find(" #") {
        (&line[..pos], &line[pos..])
    } else {
        (line, "")
    }
}

/// 去掉 yaml 值两侧引号。
fn unquote(v: &str) -> String {
    let v = v.trim();
    if (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
        || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2)
    {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// 新值是否需要双引号包裹（含 yaml 特殊字符时）。
fn needs_quote(v: &str) -> bool {
    v.contains('#') || v.contains('"') || v.contains(": ") || v.starts_with(' ')
}

/// 从 yaml 文本解析运维字段。
pub fn parse(text: &str) -> ExternalConfig {
    let mut cfg = ExternalConfig::default();
    let mut sec = Section::Root;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("---") {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let (body, _c) = split_comment(trimmed);
        if let Some((key, val)) = body.split_once(':') {
            let key = key.trim();
            // 段落 key 形如 "master:"，带值的行是叶子
            if val.trim().is_empty() {
                advance_section(&mut sec, trimmed, indent);
                continue;
            }
            if let Some(field) = matches_key(sec, key) {
                let v = unquote(val);
                match field {
                    "server_port" => cfg.server_port = v,
                    "mysql_master_url" => cfg.mysql_master_url = v,
                    "mysql_username" => cfg.mysql_username = v,
                    "mysql_password" => cfg.mysql_password = v,
                    "mysql_slave_url" => cfg.mysql_slave_url = v,
                    "redis_host" => cfg.redis_host = v,
                    "redis_port" => cfg.redis_port = v,
                    "redis_password" => cfg.redis_password = v,
                    "vernemq_host_url" => cfg.vernemq_host_url = v,
                    "vernemq_username" => cfg.vernemq_username = v,
                    "vernemq_password" => cfg.vernemq_password = v,
                    _ => {}
                }
            }
        }
    }
    cfg
}

/// 用 new_cfg 的值逐行替换 yaml 中对应字段（保留行尾注释），返回新文本。
pub fn apply(text: &str, new_cfg: &ExternalConfig) -> String {
    let mut sec = Section::Root;
    let mut out = Vec::with_capacity(text.lines().count());
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("---") {
            out.push(line.to_string());
            continue;
        }
        let indent = line.len() - trimmed.len();
        let (body, comment) = split_comment(trimmed);
        if let Some((key, val)) = body.split_once(':') {
            let key = key.trim();
            if val.trim().is_empty() {
                advance_section(&mut sec, trimmed, indent);
                out.push(line.to_string());
                continue;
            }
            if let Some(field) = matches_key(sec, key) {
                let new_val = match field {
                    "server_port" => &new_cfg.server_port,
                    "mysql_master_url" => &new_cfg.mysql_master_url,
                    "mysql_username" => &new_cfg.mysql_username,
                    "mysql_password" => &new_cfg.mysql_password,
                    "mysql_slave_url" => &new_cfg.mysql_slave_url,
                    "redis_host" => &new_cfg.redis_host,
                    "redis_port" => &new_cfg.redis_port,
                    "redis_password" => &new_cfg.redis_password,
                    "vernemq_host_url" => &new_cfg.vernemq_host_url,
                    "vernemq_username" => &new_cfg.vernemq_username,
                    "vernemq_password" => &new_cfg.vernemq_password,
                    _ => "",
                };
                if !new_val.is_empty() {
                    let indent_str = &line[..indent]; // 原样保留缩进
                    let v = if needs_quote(new_val) {
                        format!("\"{}\"", new_val.replace('"', "'"))
                    } else {
                        new_val.to_string()
                    };
                    out.push(format!("{indent_str}{key}: {v}{comment}"));
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    let mut text = out.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// 读取外部配置；文件不存在返回 None（调用方回退「未初始化」提示）。
pub fn read(install_dir: &Path) -> Result<Option<ExternalConfig>, String> {
    let path = config_path(install_dir);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    Ok(Some(parse(&text)))
}

/// 原子写回：临时文件 + rename，避免写一半崩溃留下残缺配置。
pub fn write(install_dir: &Path, cfg: &ExternalConfig) -> Result<PathBuf, String> {
    let path = config_path(install_dir);
    if !path.exists() {
        return Err(format!(
            "外部配置不存在：{}。请先在部署目录创建 config/application-local.yaml（可从项目模板拷贝）",
            path.display()
        ));
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    let new_text = apply(&text, cfg);
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, &new_text).map_err(|e| format!("写入临时文件失败：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换配置文件失败：{e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "server:\n  port: 48085 # 服务启动端口\n\nspring:\n  datasource:\n    dynamic:\n      datasource:\n        master:\n          url: jdbc:mysql://127.0.0.1:3306/dfy-test-db?useSSL=false\n          username: root\n          password: lili@112357\n        slave:\n          lazy: true\n          url: jdbc:mysql://127.0.0.1:3306/dfy-test-db?x=1\n          username: root\n          password: lili@112357\n  redis:\n    host: 127.0.0.1 # 地址\n    port: 6379 # 端口\n    database: 0\n    password: \"\" # 密码\n\niot:\n  verneMq:\n    username: cloud\n    password: key@00\n    hostUrl: tcp://127.0.0.1:18830\n";

    #[test]
    fn parse_extracts_all_fields() {
        let c = parse(SAMPLE);
        assert_eq!(c.server_port, "48085");
        assert_eq!(c.mysql_master_url, "jdbc:mysql://127.0.0.1:3306/dfy-test-db?useSSL=false");
        assert_eq!(c.mysql_username, "root");
        assert_eq!(c.mysql_password, "lili@112357");
        assert_eq!(c.redis_host, "127.0.0.1");
        assert_eq!(c.redis_port, "6379");
        assert_eq!(c.redis_password, "");
        assert_eq!(c.vernemq_host_url, "tcp://127.0.0.1:18830");
        assert_eq!(c.vernemq_username, "cloud");
        assert_eq!(c.vernemq_password, "key@00");
    }

    #[test]
    fn apply_replaces_values_and_keeps_comments() {
        let new = ExternalConfig {
            server_port: "48086".into(),
            mysql_password: "new#pwd".into(),
            redis_password: "redis123".into(),
            vernemq_host_url: "tcp://192.168.56.10:18830".into(),
            ..Default::default()
        };
        let out = apply(SAMPLE, &new);
        assert!(out.contains("  port: 48086 # 服务启动端口"), "{out}");
        assert!(out.contains("password: \"new#pwd\""), "{out}");
        assert!(out.contains("password: redis123 # 密码"), "{out}");
        assert!(out.contains("hostUrl: tcp://192.168.56.10:18830"), "{out}");
        // 未改字段保持原值
        assert!(out.contains("username: root"));
        assert!(out.contains("jdbc:mysql://127.0.0.1:3306/dfy-test-db?useSSL=false"));
        // 空字段不覆盖原值：master 密码未提供 → 保持 lili@112357
        assert!(out.contains("password: lili@112357"), "{out}");
    }

    #[test]
    fn apply_roundtrip_parse() {
        let new = ExternalConfig {
            server_port: "48086".into(),
            mysql_master_url: "jdbc:mysql://10.0.0.5:3306/prod?useSSL=true".into(),
            ..Default::default()
        };
        let c = parse(&apply(SAMPLE, &new));
        assert_eq!(c.server_port, "48086");
        assert_eq!(c.mysql_master_url, "jdbc:mysql://10.0.0.5:3306/prod?useSSL=true");
    }
}
