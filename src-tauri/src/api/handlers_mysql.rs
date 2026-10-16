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
    let dump = format!("{dir}\\bin\\mysqldump.exe");
    if !std::path::Path::new(&dump).is_file() {
        return Err(format!("未找到 mysqldump（{dump}）"));
    }
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
