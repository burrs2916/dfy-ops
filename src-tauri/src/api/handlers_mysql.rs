//! 数据库运维（MySQL）：平台托管的 root 凭据 + 轻量 SQL 控制台 + 库表浏览 +
//! 用户权限管理。全部经平台安装的 mysql.exe CLI 执行（与 SQL 导入同链路，
//! 不引入数据库驱动依赖），凭据取自 config/mysql_credential.json。
//!
//! 安全边界：这些接口是本机运维 API（CLIENT 头 + CORS 白名单保护），
//! root 直连本机 MySQL 是运维平台的既定设计。

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use super::handlers_repair::{blocking, registry_install_dir};
use super::server::AppState;
use crate::ops::mysql_credential::{self, MysqlCredential};

/// 平台 MySQL 客户端可执行文件路径。
fn mysql_client(state: &AppState) -> Result<String, String> {
    let dir = registry_install_dir(state, "mysql");
    let client = format!("{dir}\\bin\\mysql.exe");
    if std::path::Path::new(&client).is_file() {
        Ok(client)
    } else {
        Err(format!("未找到 mysql 客户端（{client}），请确认 MySQL 已由平台安装且服务存在"))
    }
}

/// 提取 CLI stderr 里的 ERROR 行（过滤 Warning 噪音），最多 3 行。
fn stderr_error_tail(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|l| l.contains("ERROR"))
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ")
}

/// 还原 mysql -B 输出的字段转义（\t \n \0 \\ → 原字符）。一次扫描避免
/// 先替换 \ 再替换 \t 导致的二次解转义。
fn unescape_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('0') => out.push('\0'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// SQL 字符串字面量转义（单引号、反斜杠）。
fn sql_quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// 执行一条 SQL（-B 批处理 TSV）。返回（列名, 数据行），已还原转义。
/// 非查询语句返回空列空行。失败返回 stderr 里的 ERROR 摘要。
fn run_sql(
    state: &AppState,
    cred: &MysqlCredential,
    sql: &str,
    database: Option<&str>,
) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let client = mysql_client(state)?;
    let port = cred.port.to_string();
    let mut cmd = std::process::Command::new(&client);
    cmd.args(["-h", "127.0.0.1", "-P", &port, "-u", &cred.user, "--default-character-set=utf8mb4", "-B"]);
    if !cred.password.is_empty() {
        cmd.arg(format!("-p{}", cred.password));
    }
    if let Some(db) = database {
        cmd.arg(db);
    }
    cmd.arg("-e").arg(sql);
    let out = cmd.output().map_err(|e| format!("无法启动 mysql 客户端：{e}"))?;
    if !out.status.success() {
        let tail = stderr_error_tail(&String::from_utf8_lossy(&out.stderr));
        return Err(if tail.is_empty() { "SQL 执行失败（未知错误）".into() } else { tail });
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().filter(|l| !l.is_empty());
    let Some(header) = lines.next() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let columns: Vec<String> = header.split('\t').map(unescape_field).collect();
    let rows: Vec<Vec<String>> = lines
        .map(|l| l.split('\t').map(unescape_field).collect())
        .collect();
    Ok((columns, rows))
}

/// GET /api/mysql/overview —— 连接信息 + 版本 + 库列表（服务未运行时其余字段降级）。
pub async fn overview(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let running = blocking(|| crate::ops::env_repair::service_running("MySQL80")).await;
    let cred = mysql_credential::load(&state.config_dir);
    let (cred_port, cred_user) = (cred.port, cred.user.clone());
    let has_password = !cred.password.is_empty();
    if !running {
        return Json(json!({
            "running": false,
            "host": "127.0.0.1",
            "port": cred_port,
            "user": cred_user,
            "hasPassword": has_password,
            "version": null,
            "databases": [],
        }));
    }
    let (version, databases, error) = blocking(move || {
        let v = run_sql(&state, &cred, "SELECT VERSION()", None)
            .ok()
            .and_then(|(_, rows)| rows.first().and_then(|r| r.first().cloned()));
        let dbs: Vec<String> = run_sql(&state, &cred, "SHOW DATABASES", None)
            .map(|(_, rows)| rows.into_iter().filter_map(|r| r.into_iter().next()).collect())
            .unwrap_or_default();
        let err = if v.is_none() {
            Some("已连接但查询失败：请检查平台托管的 root 密码（可在下方修改）".to_string())
        } else {
            None
        };
        (v, dbs, err)
    })
    .await;
    Json(json!({
        "running": true,
        "host": "127.0.0.1",
        "port": cred_port,
        "user": cred_user,
        "hasPassword": has_password,
        "version": version,
        "databases": databases,
        "error": error,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ExecReq {
    pub sql: String,
    /// 可选：在该库中执行（等价 USE db 后执行）
    #[serde(default)]
    pub database: Option<String>,
}

/// POST /api/mysql/exec —— SQL 控制台：单语句执行，SELECT/SHOW 返回结果集。
pub async fn exec(State(state): State<Arc<AppState>>, Json(req): Json<ExecReq>) -> Json<serde_json::Value> {
    let sql = req.sql.trim().trim_end_matches(';').to_string();
    if sql.is_empty() {
        return Json(json!({ "ok": false, "error": "SQL 语句为空" }));
    }
    let cred = mysql_credential::load(&state.config_dir);
    let res = blocking(move || run_sql(&state, &cred, &sql, req.database.as_deref())).await;
    match res {
        Ok((columns, rows)) => Json(json!({
            "ok": true,
            "columns": columns,
            "rows": rows,
            "rowCount": rows.len(),
        })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct DbQuery {
    pub db: String,
}

/// GET /api/mysql/tables?db= —— 库内表列表（information_schema：行数估算 + 大小 + 注释）。
pub async fn tables(State(state): State<Arc<AppState>>, Query(q): Query<DbQuery>) -> Json<serde_json::Value> {
    if !q.db.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') || q.db.is_empty() {
        return Json(json!({ "ok": false, "error": "库名含非法字符" }));
    }
    let cred = mysql_credential::load(&state.config_dir);
    let sql = format!(
        "SELECT TABLE_NAME, IFNULL(TABLE_ROWS,0), ROUND(IFNULL(DATA_LENGTH+INDEX_LENGTH,0)/1024,1), IFNULL(TABLE_COMMENT,'') \
         FROM information_schema.TABLES WHERE TABLE_SCHEMA={} AND TABLE_TYPE='BASE TABLE' ORDER BY TABLE_NAME",
        sql_quote(&q.db)
    );
    let res = blocking(move || run_sql(&state, &cred, &sql, None)).await;
    match res {
        Ok((_, rows)) => {
            let tables: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|r| json!({ "name": r.first(), "rows": r.get(1), "sizeKb": r.get(2), "comment": r.get(3) }))
                .collect();
            Json(json!({ "ok": true, "tables": tables }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// GET /api/mysql/users —— 用户列表（过滤 mysql.% 系统账号）+ 各自授权语句。
pub async fn users(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let cred = mysql_credential::load(&state.config_dir);
    let res = blocking(move || {
        let (_, rows) = run_sql(
            &state,
            &cred,
            "SELECT user, host FROM mysql.user WHERE user NOT LIKE 'mysql.%' AND user <> '' ORDER BY user, host",
            None,
        )?;
        let mut users = Vec::new();
        for r in rows {
            let (Some(u), Some(h)) = (r.first().cloned(), r.get(1).cloned()) else { continue };
            let grant_sql = format!("SHOW GRANTS FOR {}@{}", sql_quote(&u), sql_quote(&h));
            let grants = run_sql(&state, &cred, &grant_sql, None)
                .map(|(_, rows)| {
                    rows.into_iter()
                        .filter_map(|r| r.into_iter().next())
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default();
            users.push(json!({ "user": u, "host": h, "grants": grants }));
        }
        Ok::<_, String>(users)
    })
    .await;
    match res {
        Ok(users) => Json(json!({ "ok": true, "users": users })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUserReq {
    pub user: String,
    /// 允许连接的主机（默认 localhost）
    #[serde(default = "default_host")]
    pub host: String,
    pub password: String,
    /// 权限档：ro 只读 / rw 读写 / all 全部
    pub privilege: String,
    /// 授权库：* 全部库，或具体库名
    pub database: String,
}

fn default_host() -> String {
    "localhost".into()
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// POST /api/mysql/create-user —— 新建用户并按权限档授权（常用权限的表单化）。
pub async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateUserReq>,
) -> Json<serde_json::Value> {
    if !valid_name(&req.user) {
        return Json(json!({ "ok": false, "error": "用户名只能包含字母、数字、下划线、连字符" }));
    }
    if !req.host.chars().all(|c| c.is_ascii_alphanumeric() || "._-%".contains(c)) || req.host.is_empty() {
        return Json(json!({ "ok": false, "error": "主机名只能包含字母、数字与 . _ - %" }));
    }
    if req.password.is_empty() {
        return Json(json!({ "ok": false, "error": "必须为用户设置密码" }));
    }
    let priv_sql = match req.privilege.as_str() {
        "ro" => "SELECT",
        "rw" => "SELECT, INSERT, UPDATE, DELETE",
        "all" => "ALL PRIVILEGES",
        other => return Json(json!({ "ok": false, "error": format!("未知权限档：{other}") })),
    };
    let on = if req.database == "*" {
        "*.*".to_string()
    } else if valid_name(&req.database) {
        format!("`{}`.*", req.database)
    } else {
        return Json(json!({ "ok": false, "error": "库名含非法字符" }));
    };
    let cred = mysql_credential::load(&state.config_dir);
    let u = sql_quote(&req.user);
    let h = sql_quote(&req.host);
    let p = sql_quote(&req.password);
    let sql = format!(
        "CREATE USER {u}@{h} IDENTIFIED BY {p}; GRANT {priv_sql} ON {on} TO {u}@{h}"
    );
    let st = state.clone();
    let res = blocking(move || run_sql(&st, &cred, &sql, None)).await;
    match res {
        Ok(_) => {
            state.repair.log(format!("MySQL 新建用户 {}@{}（{}）成功", req.user, req.host, req.privilege));
            Json(json!({ "ok": true, "message": format!("用户 {}@{} 创建并授权成功", req.user, req.host) }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordReq {
    /// 当前 root 密码（默认回填平台托管值，可覆盖——覆盖手动改过密码的场景）
    pub current: String,
    pub next: String,
}

/// POST /api/mysql/password —— 修改 root 密码：先用「当前密码」验证连接，
/// 再 ALTER USER USER() 改密，成功后更新平台托管凭据。
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChangePasswordReq>,
) -> Json<serde_json::Value> {
    if req.next.is_empty() {
        return Json(json!({ "ok": false, "error": "新密码不能为空" }));
    }
    if req.next.chars().count() < 6 {
        return Json(json!({ "ok": false, "error": "新密码至少 6 个字符" }));
    }
    let st = state.clone();
    let res = blocking(move || {
        let mut cred = mysql_credential::load(&st.config_dir);
        cred.password = req.current.clone();
        // 1. 验证当前密码（SELECT 1 会在密码错误时失败）
        run_sql(&st, &cred, "SELECT 1", None)
            .map_err(|e| format!("当前密码验证失败：{e}"))?;
        // 2. 改密（USER() 即当前登录用户，不写死 host）
        let alter = format!("ALTER USER USER() IDENTIFIED BY {}", sql_quote(&req.next));
        run_sql(&st, &cred, &alter, None)
            .map_err(|e| format!("ALTER USER 失败：{e}"))?;
        // 3. 更新托管凭据
        cred.password = req.next.clone();
        mysql_credential::save(&st.config_dir, &cred)?;
        Ok::<_, String>(())
    })
    .await;
    match res {
        Ok(()) => {
            state.repair.log("MySQL root 密码修改成功，平台托管凭据已同步更新");
            Json(json!({ "ok": true, "message": "root 密码修改成功，平台已同步托管" }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// GET /api/mysql/credential —— 平台托管的连接信息（密码脱敏，前端点「显示」再取全量）。
pub async fn credential(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let cred = mysql_credential::load(&state.config_dir);
    Json(json!({
        "user": cred.user,
        "password": cred.password,
        "port": cred.port,
        "hasPassword": !cred.password.is_empty(),
    }))
}
