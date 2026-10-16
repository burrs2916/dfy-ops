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

/// MySQL 客户端工具候选路径（按优先级，与 zip 安装管线 locate_home 同口径）：
/// 平台目录顶层 bin → 一层嵌套 bin（zip 带顶层目录 mysql-8.x-winx64 时主目录
/// 在嵌套层）→ MySQL Installer 常见安装位 → PATH。exe 传 "mysql.exe" /
/// "mysqldump.exe"。PATH 条目（不含路径分隔符）以 --version 实跑验证。
fn tool_candidates(registry_dir: &str, exe: &str) -> Vec<String> {
    let bin = |base: &std::path::Path| {
        base.join("bin").join(exe).to_string_lossy().to_string()
    };
    let root = std::path::Path::new(registry_dir);
    let mut out = vec![bin(root)];
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.push(bin(&p));
            }
        }
    }
    for v in ["8.4", "8.0", "5.7"] {
        out.push(format!(r"C:\Program Files\MySQL\MySQL Server {v}\bin\{exe}"));
    }
    out.push(exe.to_string());
    out
}

/// 在候选里找到第一个可用的可执行文件；PATH 兜底（无路径分隔符）实跑验证。
fn find_tool(candidates: Vec<String>, label: &str) -> Result<String, String> {
    candidates
        .into_iter()
        .find(|p| {
            if p.contains('\\') || p.contains('/') {
                std::path::Path::new(p).is_file()
            } else {
                std::process::Command::new(p)
                    .arg("--version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }
        })
        .ok_or_else(|| {
            format!(
                "未找到 {label}（已尝试平台安装目录及其嵌套子目录、常见 MSI 安装位、PATH）。\
                 请确认 MySQL 已由平台安装，或切到「安装与维护」页签安装"
            )
        })
}

/// 平台 MySQL 客户端可执行文件路径（导入 SQL 等跨 handler 共用）。
pub(crate) fn mysql_client(state: &AppState) -> Result<String, String> {
    let dir = registry_install_dir(state, "mysql");
    find_tool(tool_candidates(&dir, "mysql.exe"), "mysql 客户端")
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

/// 标识符合法性（库名/表名/列名）：字母数字（含中文）、下划线、$。
/// 校验通过后可安全用反引号包裹，杜绝标识符注入。
fn valid_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// 反引号包裹标识符（调用前必须过 valid_ident）。
fn quote_ident(s: &str) -> String {
    format!("`{}`", s)
}

/// 删除类操作（删库/删表/清空）前的自动备份：mysqldump 导出整库到
/// config/backups/mysql_{db}_{epoch}.sql。产品硬性要求——删除前必须先备份，
/// 备份失败则中止删除。
fn backup_database(
    state: &AppState,
    cred: &MysqlCredential,
    db: &str,
) -> Result<String, String> {
    let dir = registry_install_dir(state, "mysql");
    let dump = find_tool(tool_candidates(&dir, "mysqldump.exe"), "mysqldump")?;
    let backup_dir = state.config_dir.join("backups");
    std::fs::create_dir_all(&backup_dir).map_err(|e| format!("无法创建备份目录：{e}"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = backup_dir.join(format!("mysql_{db}_{ts}.sql"));
    let out_file =
        std::fs::File::create(&file).map_err(|e| format!("无法创建备份文件：{e}"))?;
    let port = cred.port.to_string();
    let mut cmd = std::process::Command::new(&dump);
    cmd.args([
        "-h",
        "127.0.0.1",
        "-P",
        &port,
        "-u",
        &cred.user,
        "--default-character-set=utf8mb4",
        "--databases",
        db,
    ]);
    if !cred.password.is_empty() {
        cmd.arg(format!("-p{}", cred.password));
    }
    cmd.stdout(std::process::Stdio::from(out_file));
    let out = cmd.output().map_err(|e| format!("无法启动 mysqldump：{e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&file);
        let tail = stderr_error_tail(&String::from_utf8_lossy(&out.stderr));
        return Err(if tail.is_empty() { "mysqldump 执行失败".into() } else { tail });
    }
    Ok(file.to_string_lossy().to_string())
}

/// 执行一条 SQL（-B 批处理 TSV，平台托管凭据）。返回（列名, 数据行），已还原转义。
/// 非查询语句返回空列空行。失败返回 stderr 里的 ERROR 摘要。
fn run_sql(
    state: &AppState,
    cred: &MysqlCredential,
    sql: &str,
    database: Option<&str>,
) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let client = mysql_client(state)?;
    run_sql_with(&client, cred.port, &cred.user, &cred.password, sql, database)
}

/// 用显式连接参数执行一条 SQL（测试连接用：参数来自请求，不读托管凭据）。
fn run_sql_with(
    client: &str,
    port: u16,
    user: &str,
    password: &str,
    sql: &str,
    database: Option<&str>,
) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let port = port.to_string();
    let mut cmd = std::process::Command::new(client);
    cmd.args(["-h", "127.0.0.1", "-P", &port, "-u", user, "--default-character-set=utf8mb4", "-B"]);
    if !password.is_empty() {
        cmd.arg(format!("-p{}", password));
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
        let vres = run_sql(&state, &cred, "SELECT VERSION()", None);
        let v = vres
            .as_ref()
            .ok()
            .and_then(|(_, rows)| rows.first().and_then(|r| r.first().cloned()));
        let dbs: Vec<String> = run_sql(&state, &cred, "SHOW DATABASES", None)
            .map(|(_, rows)| rows.into_iter().filter_map(|r| r.into_iter().next()).collect())
            .unwrap_or_default();
        // 错误透传真实原因（客户端未找到 / Access denied / 拒绝连接），
        // 不再一律归因为密码不对
        let err = match vres {
            Ok(_) if v.is_some() => None,
            Ok(_) => Some("已连接但查询无结果".to_string()),
            Err(e) => {
                let hint = if e.contains("Access denied") {
                    "。平台托管的密码与实际不符时，可在「连接配置」中修正"
                } else {
                    ""
                };
                Some(format!("{e}{hint}"))
            }
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

/// 权限档位对应的 GRANT 权限清单（新建用户与调整权限共用）。
fn privilege_sql(level: &str) -> Option<&'static str> {
    match level {
        "ro" => Some("SELECT"),
        "rw" => Some("SELECT, INSERT, UPDATE, DELETE"),
        "all" => Some("ALL PRIVILEGES"),
        _ => None,
    }
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
    let priv_sql = match privilege_sql(&req.privilege) {
        Some(p) => p,
        None => return Json(json!({ "ok": false, "error": format!("未知权限档：{}", req.privilege) })),
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

// ===== 库级维护 =====

#[derive(Debug, Deserialize)]
pub struct DbNameReq {
    pub name: String,
}

/// POST /api/mysql/create-db —— 建库（utf8mb4 默认字符集）。
pub async fn create_db(State(state): State<Arc<AppState>>, Json(req): Json<DbNameReq>) -> Json<serde_json::Value> {
    if !valid_ident(&req.name) {
        return Json(json!({ "ok": false, "error": "库名只能包含字母、数字、下划线、$" }));
    }
    let cred = mysql_credential::load(&state.config_dir);
    let sql = format!(
        "CREATE DATABASE {} DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci",
        quote_ident(&req.name)
    );
    let st = state.clone();
    let res = blocking(move || run_sql(&st, &cred, &sql, None)).await;
    match res {
        Ok(_) => Json(json!({ "ok": true, "message": format!("数据库 {} 创建成功", req.name) })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// POST /api/mysql/drop-db —— 删库：先 mysqldump 自动备份整库，备份失败则中止。
pub async fn drop_db(State(state): State<Arc<AppState>>, Json(req): Json<DbNameReq>) -> Json<serde_json::Value> {
    if !valid_ident(&req.name) {
        return Json(json!({ "ok": false, "error": "库名含非法字符" }));
    }
    let st = state.clone();
    let name = req.name.clone();
    let res = blocking(move || {
        let cred = mysql_credential::load(&st.config_dir);
        let backup = backup_database(&st, &cred, &name)
            .map_err(|e| format!("已中止删除（自动备份失败）：{e}"))?;
        run_sql(&st, &cred, &format!("DROP DATABASE {}", quote_ident(&name)), None)?;
        Ok::<_, String>(backup)
    })
    .await;
    match res {
        Ok(backup) => {
            state.repair.log(format!("MySQL 删库 {} 完成，已自动备份到 {}", req.name, backup));
            Json(json!({ "ok": true, "message": format!("数据库 {} 已删除（删除前已备份：{}）", req.name, backup) }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

// ===== 表级浏览与维护 =====

#[derive(Debug, Deserialize)]
pub struct TableQuery {
    pub db: String,
    pub table: String,
}

/// GET /api/mysql/columns?db=&table= —— 表结构（information_schema.COLUMNS 全字段）。
pub async fn columns(State(state): State<Arc<AppState>>, Query(q): Query<TableQuery>) -> Json<serde_json::Value> {
    if !valid_ident(&q.db) || !valid_ident(&q.table) {
        return Json(json!({ "ok": false, "error": "库/表名含非法字符" }));
    }
    let cred = mysql_credential::load(&state.config_dir);
    let sql = format!(
        "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, IFNULL(COLUMN_DEFAULT,'NULL'), EXTRA, IFNULL(COLUMN_COMMENT,'') \
         FROM information_schema.COLUMNS WHERE TABLE_SCHEMA={} AND TABLE_NAME={} ORDER BY ORDINAL_POSITION",
        sql_quote(&q.db),
        sql_quote(&q.table)
    );
    let st = state.clone();
    let res = blocking(move || run_sql(&st, &cred, &sql, None)).await;
    match res {
        Ok((headers, rows)) => Json(json!({ "ok": true, "columns": headers, "rows": rows })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// GET /api/mysql/ddl?db=&table= —— 建表语句（SHOW CREATE TABLE）。
pub async fn ddl(State(state): State<Arc<AppState>>, Query(q): Query<TableQuery>) -> Json<serde_json::Value> {
    if !valid_ident(&q.db) || !valid_ident(&q.table) {
        return Json(json!({ "ok": false, "error": "库/表名含非法字符" }));
    }
    let cred = mysql_credential::load(&state.config_dir);
    let sql = format!("SHOW CREATE TABLE {}.{}", quote_ident(&q.db), quote_ident(&q.table));
    let st = state.clone();
    let res = blocking(move || {
        run_sql(&st, &cred, &sql, None).map(|(_, rows)| {
            rows.first().and_then(|r| r.get(1).cloned()).unwrap_or_default()
        })
    })
    .await;
    match res {
        Ok(ddl_text) => Json(json!({ "ok": true, "ddl": ddl_text })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct DataQuery {
    pub db: String,
    pub table: String,
    #[serde(default = "one")]
    pub page: u32,
    #[serde(default = "page_size")]
    pub size: u32,
    /// 排序列（可选）
    #[serde(default)]
    pub order: Option<String>,
    /// asc / desc
    #[serde(default)]
    pub dir: Option<String>,
    /// 筛选列（可选；缺省在全部列上搜）
    #[serde(default)]
    pub col: Option<String>,
    /// 筛选关键字（LIKE 模糊匹配）
    #[serde(default)]
    pub kw: Option<String>,
}

fn one() -> u32 {
    1
}

fn page_size() -> u32 {
    50
}

/// GET /api/mysql/data —— 表数据分页浏览：LIKE 筛选 + 排序 + COUNT 总行数。
/// 标识符全部过 valid_ident 白名单后反引号包裹；筛选值走 SQL 字符串转义。
pub async fn data(State(state): State<Arc<AppState>>, Query(q): Query<DataQuery>) -> Json<serde_json::Value> {
    if !valid_ident(&q.db) || !valid_ident(&q.table) {
        return Json(json!({ "ok": false, "error": "库/表名含非法字符" }));
    }
    if let Some(o) = q.order.as_deref() {
        if !valid_ident(o) {
            return Json(json!({ "ok": false, "error": "排序列名含非法字符" }));
        }
    }
    if let Some(c) = q.col.as_deref() {
        if !valid_ident(c) {
            return Json(json!({ "ok": false, "error": "筛选列名含非法字符" }));
        }
    }
    if !(1..=200).contains(&q.size) || q.page == 0 {
        return Json(json!({ "ok": false, "error": "分页参数非法" }));
    }
    let table_ref = format!("{}.{}", quote_ident(&q.db), quote_ident(&q.table));
    let st = state.clone();
    let res = blocking(move || {
        let cred = mysql_credential::load(&st.config_dir);
        // WHERE：指定列则单列 LIKE，否则全列 OR LIKE（最多前 30 列，防止宽表语句爆炸）
        let mut where_sql = String::new();
        if let Some(kw) = q.kw.as_deref().filter(|k| !k.is_empty()) {
            let like = sql_quote(&format!("%{kw}%"));
            match q.col.as_deref() {
                Some(c) => where_sql = format!(" WHERE {} LIKE {like}", quote_ident(c)),
                None => {
                    let (_, col_rows) = run_sql(
                        &st,
                        &cred,
                        &format!(
                            "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
                             WHERE TABLE_SCHEMA={} AND TABLE_NAME={} ORDER BY ORDINAL_POSITION LIMIT 30",
                            sql_quote(&q.db),
                            sql_quote(&q.table)
                        ),
                        None,
                    )?;
                    let conds: Vec<String> = col_rows
                        .iter()
                        .filter_map(|r| r.first().cloned())
                        .map(|c| format!("{} LIKE {like}", quote_ident(&c)))
                        .collect();
                    if !conds.is_empty() {
                        where_sql = format!(" WHERE {}", conds.join(" OR "));
                    }
                }
            }
        }
        let order_sql = q
            .order
            .as_deref()
            .map(|o| {
                let dir = match q.dir.as_deref().unwrap_or("").to_ascii_uppercase().as_str() {
                    "DESC" => "DESC",
                    _ => "ASC",
                };
                format!(" ORDER BY {} {dir}", quote_ident(o))
            })
            .unwrap_or_default();
        let total = run_sql(&st, &cred, &format!("SELECT COUNT(*) FROM {table_ref}{where_sql}"), None)?
            .1
            .first()
            .and_then(|r| r.first().and_then(|v| v.parse::<u64>().ok()))
            .unwrap_or(0);
        let offset = (q.page - 1) * q.size;
        let (_, rows) = run_sql(
            &st,
            &cred,
            &format!("SELECT * FROM {table_ref}{where_sql}{order_sql} LIMIT {offset},{}", q.size),
            None,
        )?;
        // 列名单独取（SELECT * 的 header 行就是列名）
        let (_, header_rows) = run_sql(
            &st,
            &cred,
            &format!("SELECT * FROM {table_ref} LIMIT 0"),
            None,
        )?;
        let columns: Vec<String> = header_rows
            .iter()
            .next()
            .map(|r| r.clone())
            .unwrap_or_default();
        Ok::<_, String>((columns, rows, total))
    })
    .await;
    match res {
        Ok((columns, rows, total)) => Json(json!({
            "ok": true, "columns": columns, "rows": rows, "total": total,
            "page": q.page, "size": q.size,
        })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct TableActionReq {
    pub db: String,
    pub table: String,
    /// optimize / analyze / truncate / drop
    pub action: String,
}

/// POST /api/mysql/table-action —— 表维护：optimize/analyze 免备份；
/// truncate/drop 属删除类，先 mysqldump 自动备份（备份失败中止）。
pub async fn table_action(State(state): State<Arc<AppState>>, Json(req): Json<TableActionReq>) -> Json<serde_json::Value> {
    if !valid_ident(&req.db) || !valid_ident(&req.table) {
        return Json(json!({ "ok": false, "error": "库/表名含非法字符" }));
    }
    let destructive = matches!(req.action.as_str(), "truncate" | "drop");
    let sql = match req.action.as_str() {
        "optimize" => format!("OPTIMIZE TABLE {}", quote_ident(&req.table)),
        "analyze" => format!("ANALYZE TABLE {}", quote_ident(&req.table)),
        "truncate" => format!("TRUNCATE TABLE {}", quote_ident(&req.table)),
        "drop" => format!("DROP TABLE {}", quote_ident(&req.table)),
        other => return Json(json!({ "ok": false, "error": format!("未知操作：{other}") })),
    };
    let st = state.clone();
    let db = req.db.clone();
    let db_for_log = req.db.clone();
    let action = req.action.clone();
    let res = blocking(move || {
        let cred = mysql_credential::load(&st.config_dir);
        let backup = if destructive {
            Some(backup_database(&st, &cred, &db).map_err(|e| format!("已中止操作（自动备份失败）：{e}"))?)
        } else {
            None
        };
        // 在目标库上下文执行（OPTIMIZE/TRUNCATE 的 table_ref 无库前缀）
        run_sql(&st, &cred, &sql, Some(&db))?;
        Ok::<_, String>(backup)
    })
    .await;
    match res {
        Ok(backup) => {
            let label = match action.as_str() {
                "optimize" => "优化",
                "analyze" => "分析",
                "truncate" => "清空",
                _ => "删除",
            };
            let note = backup
                .as_ref()
                .map(|b| format!("，删除前已备份：{b}"))
                .unwrap_or_default();
            state.repair.log(format!(
                "MySQL 表 {}（{}.{}）{}完成{note}",
                req.table, req.db, db_for_log, label
            ));
            Json(json!({ "ok": true, "message": format!(
                "{}完成{}",
                label,
                backup.as_ref().map(|b| format!("（已备份：{b}）")).unwrap_or_default()
            ) }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

// ===== 服务器会话（processlist） =====

/// GET /api/mysql/processlist —— 当前连接/会话列表。
pub async fn processlist(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let cred = mysql_credential::load(&state.config_dir);
    let st = state.clone();
    let res = blocking(move || run_sql(&st, &cred, "SHOW FULL PROCESSLIST", None)).await;
    match res {
        Ok((headers, rows)) => Json(json!({ "ok": true, "columns": headers, "rows": rows })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct KillReq {
    pub id: u64,
}

/// POST /api/mysql/kill —— 终止一个会话（KILL {id}，id 为数字天然安全）。
pub async fn kill(State(state): State<Arc<AppState>>, Json(req): Json<KillReq>) -> Json<serde_json::Value> {
    let cred = mysql_credential::load(&state.config_dir);
    let st = state.clone();
    let res = blocking(move || run_sql(&st, &cred, &format!("KILL {}", req.id), None)).await;
    match res {
        Ok(_) => Json(json!({ "ok": true, "message": format!("会话 {} 已终止", req.id) })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct DropUserReq {
    pub user: String,
    pub host: String,
}

/// POST /api/mysql/drop-user —— 删除用户（撤销其全部授权）。
pub async fn drop_user(State(state): State<Arc<AppState>>, Json(req): Json<DropUserReq>) -> Json<serde_json::Value> {
    if !valid_ident(&req.user) {
        return Json(json!({ "ok": false, "error": "用户名含非法字符" }));
    }
    if req.host.is_empty() || req.host.contains('\'') || req.host.contains('\\') {
        return Json(json!({ "ok": false, "error": "主机名含非法字符" }));
    }
    let cred = mysql_credential::load(&state.config_dir);
    let sql = format!("DROP USER {}@{}", sql_quote(&req.user), sql_quote(&req.host));
    let st = state.clone();
    let res = blocking(move || run_sql(&st, &cred, &sql, None)).await;
    match res {
        Ok(_) => {
            state.repair.log(format!("MySQL 用户 {}@{} 已删除", req.user, req.host));
            Json(json!({ "ok": true, "message": format!("用户 {}@{} 已删除", req.user, req.host) }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct UserPasswordReq {
    pub user: String,
    pub host: String,
    pub password: String,
}

/// POST /api/mysql/user-password —— 修改业务用户密码（root 操作，不需要旧密码）。
pub async fn user_password(State(state): State<Arc<AppState>>, Json(req): Json<UserPasswordReq>) -> Json<serde_json::Value> {
    if !valid_ident(&req.user) {
        return Json(json!({ "ok": false, "error": "用户名含非法字符" }));
    }
    if req.host.is_empty() || req.host.contains('\'') || req.host.contains('\\') {
        return Json(json!({ "ok": false, "error": "主机名含非法字符" }));
    }
    if req.password.is_empty() {
        return Json(json!({ "ok": false, "error": "密码不能为空" }));
    }
    let cred = mysql_credential::load(&state.config_dir);
    let sql = format!(
        "ALTER USER {}@{} IDENTIFIED BY {}",
        sql_quote(&req.user),
        sql_quote(&req.host),
        sql_quote(&req.password)
    );
    let st = state.clone();
    let res = blocking(move || run_sql(&st, &cred, &sql, None)).await;
    match res {
        Ok(_) => {
            state.repair.log(format!("MySQL 用户 {}@{} 密码已修改", req.user, req.host));
            Json(json!({ "ok": true, "message": format!("用户 {}@{} 密码修改成功", req.user, req.host) }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateCredentialReq {
    #[serde(default)]
    pub user: Option<String>,
    /// 平台连接 MySQL 用的密码（手动同步：在平台外改过 root 密码后在这里修正）
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

/// POST /api/mysql/credential —— 修正平台托管的连接配置（只写托管文件，不碰 MySQL）。
/// 自救入口：密码在平台外被改动导致平台连不上时，在这里输入真实密码重新对上。
pub async fn update_credential(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateCredentialReq>,
) -> Json<serde_json::Value> {
    let mut cred = mysql_credential::load(&state.config_dir);
    if let Some(u) = req.user.as_deref() {
        if u.trim().is_empty() {
            return Json(json!({ "ok": false, "error": "用户名不能为空" }));
        }
        cred.user = u.trim().to_string();
    }
    if let Some(p) = req.password {
        cred.password = p;
    }
    if let Some(port) = req.port {
        if !(1..=65535).contains(&port) {
            return Json(json!({ "ok": false, "error": "端口范围 1-65535" }));
        }
        cred.port = port;
    }
    if let Err(e) = mysql_credential::save(&state.config_dir, &cred) {
        return Json(json!({ "ok": false, "error": e }));
    }
    state.repair.log("MySQL 平台连接配置已更新（托管凭据同步）");
    Json(json!({ "ok": true, "message": "平台连接配置已保存，可点「刷新」验证连接" }))
}

// ===== 服务器参数动态配置 =====

/// 可在线调整的常用参数白名单（GET 展示与 POST 设置共用，防任意 SET GLOBAL）。
/// 注明类型：数字类 SET GLOBAL 校验纯数字值。
const MANAGED_VARIABLES: &[(&str, bool)] = &[
    ("max_connections", true),
    ("max_allowed_packet", true),
    ("wait_timeout", true),
    ("interactive_timeout", true),
    ("innodb_buffer_pool_size", true),
    ("character_set_server", false),
    ("collation_server", false),
    ("time_zone", false),
    ("sql_mode", false),
];

/// GET /api/mysql/variables —— 常用参数当前值（SHOW VARIABLES 白名单内）。
pub async fn variables(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let names: Vec<String> = MANAGED_VARIABLES.iter().map(|(n, _)| sql_quote(n)).collect();
    let sql = format!("SHOW VARIABLES WHERE Variable_name IN ({})", names.join(", "));
    let cred = mysql_credential::load(&state.config_dir);
    let st = state.clone();
    let res = blocking(move || run_sql(&st, &cred, &sql, None)).await;
    match res {
        Ok((_, rows)) => {
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| json!({ "name": r.first().cloned().unwrap_or_default(), "value": r.get(1).cloned().unwrap_or_default() }))
                .collect();
            Json(json!({ "ok": true, "items": items }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct SetVariableReq {
    pub name: String,
    pub value: String,
}

/// POST /api/mysql/set-variable —— 动态调整常用参数（SET GLOBAL，运行时生效）。
/// 仅白名单内的变量可设置；数字类校验纯数字。重启后恢复 my.ini 配置值。
pub async fn set_variable(State(state): State<Arc<AppState>>, Json(req): Json<SetVariableReq>) -> Json<serde_json::Value> {
    let Some((_, is_numeric)) = MANAGED_VARIABLES.iter().find(|(n, _)| *n == req.name) else {
        return Json(json!({ "ok": false, "error": format!("不支持修改参数：{}（仅支持平台白名单内的常用参数）", req.name) }));
    };
    let value = req.value.trim().to_string();
    if value.is_empty() {
        return Json(json!({ "ok": false, "error": "参数值不能为空" }));
    }
    if *is_numeric && !value.chars().all(|c| c.is_ascii_digit()) {
        return Json(json!({ "ok": false, "error": "该参数只接受纯数字值" }));
    }
    if !*is_numeric && (value.contains('\'') || value.contains('\\') || value.contains(';')) {
        return Json(json!({ "ok": false, "error": "参数值含非法字符" }));
    }
    let cred = mysql_credential::load(&state.config_dir);
    let lit = if *is_numeric { value.clone() } else { sql_quote(&value) };
    let sql = format!("SET GLOBAL {} = {}", quote_ident(&req.name), lit);
    let st = state.clone();
    let name = req.name.clone();
    let res = blocking(move || run_sql(&st, &cred, &sql, None)).await;
    match res {
        Ok(_) => {
            state.repair.log(format!("MySQL 参数 {} 已动态调整为 {}（重启后恢复 my.ini 配置值）", name, value));
            Json(json!({ "ok": true, "message": format!("{} 已调整为 {}（运行时生效；重启后恢复 my.ini 配置值）", name, value) }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

// ===== 测试连接 =====

#[derive(Debug, Deserialize)]
pub struct TestConnectionReq {
    pub user: String,
    #[serde(default)]
    pub password: String,
    pub port: u16,
}

/// POST /api/mysql/test-connection —— 用请求给出的参数实测连接（SELECT VERSION()），
/// 不读写托管凭据文件。连接配置对话框「测试连接」用：先验证再保存。
pub async fn test_connection(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TestConnectionReq>,
) -> Json<serde_json::Value> {
    if req.user.trim().is_empty() {
        return Json(json!({ "ok": false, "error": "用户名不能为空" }));
    }
    if !(1..=65535).contains(&req.port) {
        return Json(json!({ "ok": false, "error": "端口范围 1-65535" }));
    }
    let client = match mysql_client(&state) {
        Ok(c) => c,
        Err(e) => return Json(json!({ "ok": false, "error": e })),
    };
    let res = blocking(move || {
        run_sql_with(&client, req.port, req.user.trim(), &req.password, "SELECT VERSION()", None)
    })
    .await;
    match res {
        Ok((_, rows)) => {
            let version = rows
                .first()
                .and_then(|r| r.first().cloned())
                .unwrap_or_default();
            Json(json!({ "ok": true, "version": version }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

// ===== my.ini 配置文件（在线查看 / 编辑） =====

/// my.ini 候选路径（与客户端定位同口径）：平台目录顶层 → 一层嵌套 →
/// ProgramData（MySQL Installer 默认数据位）。
fn ini_candidates(registry_dir: &str) -> Vec<String> {
    let root = std::path::Path::new(registry_dir);
    let mut out = vec![root.join("my.ini").to_string_lossy().to_string()];
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.push(p.join("my.ini").to_string_lossy().to_string());
            }
        }
    }
    for v in ["8.4", "8.0", "5.7"] {
        out.push(format!(r"C:\ProgramData\MySQL\MySQL Server {v}\my.ini"));
    }
    out
}

fn locate_ini(registry_dir: &str) -> Result<String, String> {
    ini_candidates(registry_dir)
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .ok_or_else(|| {
            format!(
                "未找到 my.ini（已尝试平台安装目录 {registry_dir} 及其嵌套子目录、ProgramData 常见位置）。\
                 请确认 MySQL 已由平台安装"
            )
        })
}

/// GET /api/mysql/config-file —— 读取 my.ini 内容（服务器页配置文件编辑器）。
pub async fn get_config_file(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let dir = registry_install_dir(&state, "mysql");
    let res = blocking(move || {
        let path = locate_ini(&dir)?;
        std::fs::read_to_string(&path)
            .map(|content| (path, content))
            .map_err(|e| format!("读取 my.ini 失败：{e}"))
    })
    .await;
    match res {
        Ok((path, content)) => Json(json!({ "ok": true, "path": path, "content": content })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct SaveConfigFileReq {
    pub content: String,
}

/// POST /api/mysql/config-file —— 保存 my.ini：先自动备份为 my.ini.bak-时间戳，
/// 备份失败则中止保存。修改需重启 MySQL 服务生效；格式错误会导致服务启动失败，
/// 可用备份文件随时还原。
pub async fn save_config_file(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SaveConfigFileReq>,
) -> Json<serde_json::Value> {
    let content = req.content;
    if content.trim().is_empty() {
        return Json(json!({ "ok": false, "error": "配置内容不能为空" }));
    }
    // 基本合法性：至少要有 [mysqld] 段（覆盖 [mysqld] 与 [mysqld-8.0] 写法），
    // 防止误粘贴无关内容导致服务起不来
    if !content.contains("[mysqld") {
        return Json(json!({ "ok": false, "error": "内容缺少 [mysqld] 段，疑似不是 MySQL 配置文件，已拒绝保存" }));
    }
    let dir = registry_install_dir(&state, "mysql");
    let res: Result<String, String> = blocking(move || {
        let path = locate_ini(&dir)?;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backup = format!("{path}.bak-{ts}");
        std::fs::copy(&path, &backup).map_err(|e| format!("备份失败，已中止保存：{e}"))?;
        std::fs::write(&path, &content).map_err(|e| format!("写入失败（备份在 {backup}）：{e}"))?;
        Ok(backup)
    })
    .await;
    match res {
        Ok(backup) => {
            state.repair.log("MySQL my.ini 已修改（已自动备份），重启服务后生效");
            Json(json!({
                "ok": true,
                "message": format!("已保存，原文件备份为 {backup}。重启 MySQL 服务后生效"),
                "backup": backup,
            }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct UserGrantsReq {
    pub user: String,
    pub host: String,
    pub privilege: String,
    pub database: String,
}

/// 从 SHOW GRANTS 结果行提取授权范围（ON 与 TO 之间，如 *.* / `db`.*）。
/// 仅 USAGE 的全局行没有可撤销的权限；PROXY 授权不走 ALL 撤销语法，均跳过。
fn grant_targets(rows: &[Vec<String>]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in rows {
        let Some(line) = r.first() else { continue };
        let Some(on_idx) = line.find(" ON ") else { continue };
        let after = &line[on_idx + 4..];
        let Some(to_idx) = after.find(" TO ") else { continue };
        let privs = line[5..on_idx].trim();
        let target = after[..to_idx].trim().to_string();
        if target.is_empty() || privs.is_empty() || privs == "USAGE" || privs.contains("PROXY") {
            continue;
        }
        if !out.contains(&target) {
            out.push(target);
        }
    }
    out
}

/// POST /api/mysql/user-grants —— 调整已有用户权限：按 SHOW GRANTS 的实际范围
/// 逐一撤销，再按所选档位重新授权。密码保持不变（避免删了重建丢密码）。
pub async fn user_grants(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UserGrantsReq>,
) -> Json<serde_json::Value> {
    if !valid_name(&req.user) {
        return Json(json!({ "ok": false, "error": "用户名只能包含字母、数字、下划线、连字符" }));
    }
    if !req.host.chars().all(|c| c.is_ascii_alphanumeric() || "._-%".contains(c)) || req.host.is_empty() {
        return Json(json!({ "ok": false, "error": "主机名只能包含字母、数字与 . _ - %" }));
    }
    let priv_sql = match privilege_sql(&req.privilege) {
        Some(p) => p,
        None => return Json(json!({ "ok": false, "error": format!("未知权限档：{}", req.privilege) })),
    };
    let on = if req.database == "*" {
        "*.*".to_string()
    } else if valid_name(&req.database) {
        format!("`{}`.*", req.database)
    } else {
        return Json(json!({ "ok": false, "error": "库名含非法字符" }));
    };
    let cred = mysql_credential::load(&state.config_dir);
    let st = state.clone();
    let user = req.user.clone();
    let host = req.host.clone();
    let on_msg = on.clone();
    let res = blocking(move || {
        let (_, rows) = run_sql(
            &st,
            &cred,
            &format!("SHOW GRANTS FOR {}@{}", sql_quote(&req.user), sql_quote(&req.host)),
            None,
        )?;
        let mut stmts: Vec<String> = grant_targets(&rows)
            .into_iter()
            .map(|t| {
                format!(
                    "REVOKE ALL PRIVILEGES, GRANT OPTION ON {t} FROM {}@{}",
                    sql_quote(&req.user),
                    sql_quote(&req.host)
                )
            })
            .collect();
        stmts.push(format!(
            "GRANT {priv_sql} ON {on} TO {}@{}",
            sql_quote(&req.user),
            sql_quote(&req.host)
        ));
        let sql = stmts.join("; ");
        run_sql(&st, &cred, &sql, None).map(|_| ())
    })
    .await;
    match res {
        Ok(_) => {
            let label = match req.privilege.as_str() {
                "ro" => "只读",
                "rw" => "读写",
                _ => "全部权限",
            };
            state.repair.log(format!("MySQL 用户 {user}@{host} 权限已调整为 {label}"));
            Json(json!({
                "ok": true,
                "message": format!("用户 {user}@{host} 权限已调整为「{label}」（范围 {on_msg}），密码保持不变"),
            }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[cfg(test)]
mod tests {
    use super::{grant_targets, tool_candidates};

    /// 候选顺序：平台目录顶层 bin → 一层嵌套 bin（zip 带顶层目录时主目录在
    /// 嵌套层，与安装管线 locate_home 同口径）→ MSI 常见位 → PATH 兜底。
    #[test]
    fn tool_candidates_covers_nested_layout() {
        let tmp = std::env::temp_dir().join(format!("dfy-mysql-cand-{}", std::process::id()));
        let nested = tmp.join("mysql-8.0.46-winx64");
        std::fs::create_dir_all(nested.join("bin")).unwrap();
        let dir = tmp.to_string_lossy().to_string();
        let cands = tool_candidates(&dir, "mysql.exe");
        assert!(
            cands[0].ends_with("bin/mysql.exe") || cands[0].ends_with(r"bin\mysql.exe"),
            "首位必须是平台目录顶层 bin: {:?}",
            cands[0]
        );
        assert!(
            cands.iter().any(|c| c.contains("mysql-8.0.46-winx64")),
            "嵌套层 bin 必须在候选里: {cands:?}"
        );
        assert!(
            cands.iter().any(|c| c.contains("Program Files")),
            "MSI 常见安装位必须覆盖"
        );
        assert_eq!(cands.last().unwrap(), "mysql.exe", "PATH 兜底必须最后");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// SHOW GRANTS 解析：提取可撤销范围，跳过纯 USAGE 与 PROXY 行，范围去重。
    #[test]
    fn grant_targets_parses_scopes() {
        let rows = vec![
            vec!["GRANT USAGE ON *.* TO `app`@`%`".into()],
            vec!["GRANT SELECT, INSERT ON `shop`.* TO `app`@`%`".into()],
            vec!["GRANT ALL PRIVILEGES ON *.* TO `app`@`%` WITH GRANT OPTION".into()],
            vec!["GRANT PROXY ON `other`@`localhost` TO `app`@`%`".into()],
        ];
        let targets = grant_targets(&rows);
        assert_eq!(targets, vec!["`shop`.*", "*.*"]);
        assert!(grant_targets(&[vec!["GRANT USAGE ON *.* TO `u`@`%`".into()]]).is_empty());
        assert!(grant_targets(&[]).is_empty());
    }
}
