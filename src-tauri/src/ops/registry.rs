use serde::{Deserialize, Serialize};
use std::path::Path;

/// 服务类型，决定构建/运行方式与保活策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    /// Java Vert.x 胖 jar
    Vertx,
    /// Vue3 单页应用（由 nginx 托管）
    VueSpa,
    /// Angular 单页应用（静态托管）
    AngularSpa,
    /// Rust Axum 原生二进制
    RustBin,
    /// Tauri 桌面客户端（非常驻服务）
    TauriApp,
    /// 第三方 MQTT broker（VerneMQ，Erlang/OTP）
    MqttBroker,
    /// 第三方 S3 兼容对象存储（MinIO）
    ObjectStorage,
    /// 第三方数据库/缓存（MySQL、Redis）
    Database,
}

/// 声明式依赖检查项：环境检查时按此探测，不再在代码里硬编码路径/端口。
/// `path` 相对 `install_dir`（或绝对路径），文件存在即满足（自动尝试 .exe 变体）；
/// `port` 为需要可达的回环 TCP 端口。至少给一项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiresCheck {
    /// 稳定 id（修复建议/前端键控用），缺省用 label
    #[serde(default)]
    pub id: String,
    /// 检查项显示名
    pub label: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

impl RequiresCheck {
    pub fn check_id(&self) -> &str {
        if self.id.is_empty() {
            &self.label
        } else {
            &self.id
        }
    }
}

/// 被运维的本地服务定义。路径默认按 Windows 部署约定给出，可在 UI/配置中覆盖。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDef {
    pub id: String,
    pub name: String,
    #[serde(rename = "kind")]
    pub kind: ServiceKind,
    /// Java 运行时期望主版本（Vertx kind 的环境检查用）。None 时按 8 兜底。
    /// 声明驱动：dfy-boot-base 按 JDK 8 编译（pom.xml java.version=1.8），期望值
    /// 跟着项目走而不是硬编码在检查逻辑里。
    #[serde(rename = "javaMajor", default)]
    pub java_major: Option<u32>,
    #[serde(rename = "installDir")]
    pub install_dir: String,
    /// 可选构建命令（首次或更新后执行）
    #[serde(rename = "buildCmd")]
    pub build_cmd: Option<String>,
    /// 运行命令（由进程管理器以 shell 执行）
    #[serde(rename = "runCmd")]
    pub run_cmd: String,
    pub ports: Vec<u16>,
    /// 进程名候选：本机存在命令行匹配该名字的进程时，视为「检测到在运行」（外部运行）。
    /// 用于无端口服务（Tauri 桌面客户端等）的检测兜底，Unix 走 `pgrep -f`、Windows 走 `tasklist`。
    #[serde(rename = "processNames", default)]
    pub process_names: Vec<String>,
    /// 「已安装」检测的产品名 token（Windows 读卸载注册表，按 DisplayName /
    /// DisplayIcon 匹配，与部署路径无关）。仅对需要判断「装没装」的程序声明
    /// （如 Tauri 桌面客户端）；为空表示不适用——视为已安装，不改变既有显示逻辑。
    #[serde(rename = "productNames", default)]
    pub product_names: Vec<String>,
    #[serde(rename = "healthUrl")]
    pub health_url: Option<String>,
    #[serde(rename = "logPaths")]
    pub log_paths: Vec<String>,
    #[serde(rename = "dependsOn")]
    pub depends_on: Vec<String>,
    /// 是否为常驻守护服务（Tauri 桌面客户端为 false，不参与保活）
    #[serde(rename = "isDaemon")]
    pub is_daemon: bool,
    /// 第三方依赖（VerneMQ / MinIO 等系统级中间件）：平台只检测不托管——
    /// 不参与「全部启动/停止」、不提供启停按钮、runCmd 无意义。运行由
    /// Windows 服务管理器或容器运行时负责。
    #[serde(rename = "thirdParty", default)]
    pub third_party: bool,
    /// 纯远程第三方（Redis/VerneMQ/MinIO/GreptimeDB）：本机不安装、不检测本地进程/端口，
    /// 检测 = 「远程部署」配置页托管的远程端点可达性（Redis 存凭据文件、
    /// 其余存 remote_endpoints.json）。
    /// 安装与维护不再提供这些服务的本地安装/卸载入口。
    #[serde(rename = "remoteOnly", default)]
    pub remote_only: bool,
    /// 声明式依赖检查（环境检查按此探测，不在代码里硬编码路径/端口）
    #[serde(default)]
    pub requires: Vec<RequiresCheck>,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct ServiceConfig {
    services: Vec<ServiceDef>,
}

/// 返回内置的目标服务定义（DFY 系列自研服务 + 第三方依赖）。
fn builtin() -> Vec<ServiceDef> {
    vec![
        ServiceDef {
            id: "dfy-backend".into(),
            name: "DFY 后端 (Vert.x)".into(),
            kind: ServiceKind::Vertx,
            java_major: Some(8),
            install_dir: "C:/services/dfy-boot-base".into(),
            build_cmd: Some("mvn -q -DskipTests package".into()),
            run_cmd: "{java} -jar dfy-server/target/dfy-server.jar".into(),
            ports: vec![48085],
            process_names: vec![],
            product_names: vec![],
            health_url: None,
            log_paths: vec!["C:/services/dfy-boot-base/logs/app.log".into()],
            depends_on: vec!["mysql".into(), "redis".into()],
            is_daemon: true,
            third_party: false,
            remote_only: false,
            requires: vec![RequiresCheck {
                id: "jar".into(),
                label: "后端 JAR 产物".into(),
                path: Some("dfy-server/target/dfy-server.jar".into()),
                port: None,
            }],
            description: "鼎飞益智后端服务（Java Vert.x，JDK 8），默认端口 48085；依赖 MySQL/Redis（独立注册的第三方服务）。".into(),
        },
        ServiceDef {
            id: "dfy-ui".into(),
            name: "DFY 管理前端 (Vue3)".into(),
            kind: ServiceKind::VueSpa,
            java_major: None,
            install_dir: "C:/services/dfy-ui-admin-vue3-master".into(),
            build_cmd: Some("pnpm build:prod".into()),
            run_cmd: "nginx -p C:/services/dfy-ui-admin-vue3-master -c nginx.conf".into(),
            ports: vec![80],
            process_names: vec![],
            product_names: vec![],
            health_url: None,
            log_paths: vec!["C:/services/nginx/logs/access.log".into()],
            depends_on: vec!["dfy-backend".into()],
            is_daemon: true,
            third_party: false,
            remote_only: false,
            requires: vec![],
            description: "Vue3 管理后台，由 nginx 托管于 80 端口，反代 /api 到后端 48085。".into(),
        },
        ServiceDef {
            id: "gateway-console".into(),
            name: "Gateway Console (Angular)".into(),
            kind: ServiceKind::AngularSpa,
            java_major: None,
            install_dir: "C:/services/gateway-console".into(),
            build_cmd: Some("npm run build:prod".into()),
            run_cmd: "nginx -p C:/services/gateway-console -c nginx.conf".into(),
            ports: vec![8081],
            process_names: vec![],
            product_names: vec![],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: true,
            third_party: false,
            remote_only: false,
            requires: vec![],
            description: "Angular 19 网关控制台，静态托管。".into(),
        },
        ServiceDef {
            id: "edge-conductor".into(),
            name: "Edge Conductor (Rust Axum)".into(),
            kind: ServiceKind::RustBin,
            java_major: None,
            install_dir: "C:/services/edge-conductor".into(),
            build_cmd: Some("cargo build --release".into()),
            run_cmd:
                "C:/services/edge-conductor/target/release/edgecd-daemon --config C:/services/edge-conductor/config.toml".into(),
            ports: vec![8084, 8085],
            process_names: vec!["edgecd-daemon".into()],
            product_names: vec![],
            health_url: None,
            log_paths: vec!["C:/services/edge-conductor/logs".into()],
            depends_on: vec!["dfy-backend".into(), "vernemq".into(), "minio".into(), "greptime".into()],
            is_daemon: true,
            third_party: false,
            remote_only: false,
            // GreptimeDB/VerneMQ/MinIO 均为远程部署：无任何本地二进制/路径检查项
            //（曾经声明的 greptime 本地二进制检查已随远程化移除，勿再添加）。
            requires: vec![],
            description: "边缘计算后端，自身监听 8084/8085；依赖 VerneMQ（MQTT 18830）、MinIO（对象存储）与 GreptimeDB（时序库，端点由本服务自身配置），依赖项均为远程部署的独立服务。".into(),
        },
        ServiceDef {
            id: "biosphere-spatial".into(),
            name: "空间数据驱动程序".into(),
            kind: ServiceKind::TauriApp,
            java_major: None,
            install_dir: "C:/services/biosphere-spatialdata-system".into(),
            build_cmd: Some("pnpm tauri build".into()),
            run_cmd:
                "C:/services/biosphere-spatialdata-system/target/release/biosphere-spatialdata-system.exe".into(),
            ports: vec![],
            process_names: vec!["biosphere-spatialdata-system".into()],
            // 「已安装」检测：按产品显示名（控制面板「程序和功能」同源）匹配，
            // 与部署路径无关；镜像名只判「跑没跑」，注册表才判「装没装」。
            product_names: vec!["空间数据驱动".into()],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: false,
            third_party: false,
            remote_only: false,
            requires: vec![],
            description: "空间数据驱动程序桌面客户端；无端口，按镜像名判「跑没跑」、卸载注册表判「装没装」（非常驻，不参与批量启动）。".into(),
        },
        ServiceDef {
            id: "vernemq".into(),
            name: "VerneMQ (MQTT)".into(),
            kind: ServiceKind::MqttBroker,
            java_major: None,
            // 纯远程部署：本机无安装目录，检测走「远程部署」配置页托管的端点
            install_dir: String::new(),
            build_cmd: None,
            run_cmd: String::new(),
            ports: vec![18830],
            // 本机无进程可查（跑在虚拟机/远程主机上），processNames 不适用
            process_names: vec![],
            product_names: vec![],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: true,
            third_party: true,
            remote_only: true,
            requires: vec![],
            description: "远程部署的 MQTT broker（Erlang/OTP）：平台不安装不托管，检测 = 「远程部署」配置页指向的端点可达性（MQTT 18830）。".into(),
        },
        ServiceDef {
            id: "minio".into(),
            name: "MinIO (对象存储)".into(),
            kind: ServiceKind::ObjectStorage,
            java_major: None,
            // 纯远程部署：本机无安装目录，检测走「远程部署」配置页托管的端点
            install_dir: String::new(),
            build_cmd: None,
            run_cmd: String::new(),
            ports: vec![9000],
            // 本机无进程可查（跑在虚拟机/远程主机上），processNames 不适用
            process_names: vec![],
            product_names: vec![],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: true,
            third_party: true,
            remote_only: true,
            requires: vec![],
            description: "远程部署的 S3 兼容对象存储：平台不安装不托管，检测 = 「远程部署」配置页指向的端点可达性（API 9000）。".into(),
        },
        ServiceDef {
            id: "greptime".into(),
            name: "GreptimeDB (时序库)".into(),
            kind: ServiceKind::Database,
            java_major: None,
            // 边缘计算的附属时序库：随 Edge Conductor 一同启动，平台不单独安装/托管
            install_dir: String::new(),
            build_cmd: None,
            run_cmd: String::new(),
            ports: vec![4000],
            // 不按本机进程判定：启动由 Edge Conductor 负责，平台只做端点可达性检测
            process_names: vec![],
            product_names: vec![],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: true,
            third_party: true,
            remote_only: true,
            requires: vec![],
            description: "边缘计算的附属时序库：随 Edge Conductor 一同启动，平台不安装不托管，只做「远程部署」配置页指向的端点可达性检测（HTTP 4000）。".into(),
        },
        ServiceDef {
            id: "mysql".into(),
            name: "MySQL (业务库)".into(),
            kind: ServiceKind::Database,
            java_major: None,
            install_dir: "C:/services/mysql".into(),
            build_cmd: None,
            run_cmd: String::new(),
            ports: vec![3306],
            process_names: vec!["mysqld".into(), "mysqld.exe".into()],
            product_names: vec![],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: true,
            third_party: true,
            remote_only: false,
            requires: vec![],
            description: "第三方业务数据库，平台只检测不托管；实际监听端口由进程反查（本机由 OrbStack 容器提供，走端口兜底探测）。".into(),
        },
        ServiceDef {
            id: "redis".into(),
            name: "Redis (缓存)".into(),
            kind: ServiceKind::Database,
            java_major: None,
            install_dir: String::new(),
            build_cmd: None,
            run_cmd: String::new(),
            ports: vec![6379],
            // 纯远程模式：本机无进程/端口可查，processNames 不适用
            process_names: vec![],
            product_names: vec![],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: true,
            third_party: true,
            remote_only: true,
            requires: vec![],
            description: "远程部署模式的缓存服务：本机不安装，检测目标为「运维中心 → 远程部署」配置页托管的远程端点（默认 192.168.56.101:6379）；连接不可达时请在远程部署配置中检查主机/端口/密码。".into(),
        },
    ]
}

/// 读取服务注册表。
///
/// 内置定义为底座；若设置了环境变量 `DFY_OPS_SERVICES` 指向 TOML 配置，
/// 按 **id 合并**覆盖（同 id 整体替换，新 id 追加）——合并而非整体替换，
/// 保证新增的内置第三方依赖（vernemq/minio）不会被机器本机配置冲掉。
pub fn services() -> Vec<ServiceDef> {
    let mut defs = builtin();
    if let Ok(path) = std::env::var("DFY_OPS_SERVICES") {
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<ServiceConfig>(&text) {
                Ok(cfg) => merge_by_id(&mut defs, cfg.services),
                Err(e) => eprintln!("服务配置解析失败（{path}）：{e}，使用内置默认"),
            },
            Err(e) => eprintln!("服务配置文件读取失败（{path}）：{e}，使用内置默认"),
        }
    }
    defs
}

/// 按 id 合并：overrides 中同 id 的定义整体替换 base 里的，新 id 追加到末尾。
fn merge_by_id(base: &mut Vec<ServiceDef>, overrides: Vec<ServiceDef>) {
    for o in overrides {
        match base.iter_mut().find(|d| d.id == o.id) {
            Some(d) => *d = o,
            None => base.push(o),
        }
    }
}

/// 读取服务注册表（带本机覆盖）：`app_config_dir/services.toml`（应用专属配置目录，
/// macOS: ~/Library/Application Support/com.dfy.ops/）按 id 合并到 env/内置定义之上，
/// 同 id 整体替换、新 id 追加。解析失败不 panic，逐级回退。
///
/// 注意必须传 app_config_dir（带 identifier）而非系统级 config_dir——后者不带应用名，
/// 文件放进去读不到（2026-09-05 实测踩坑：覆盖文件就绪却仍按内置 8081 探测）。
pub fn services_with_override(config_dir: &Path) -> Vec<ServiceDef> {
    let mut defs = services();
    let local = config_dir.join("services.toml");
    if let Ok(text) = std::fs::read_to_string(&local) {
        match toml::from_str::<ServiceConfig>(&text) {
            Ok(cfg) => merge_by_id(&mut defs, cfg.services),
            Err(e) => {
                eprintln!(
                    "本机 services.toml 解析失败（{}）：{e}，回退 env/内置定义",
                    local.display()
                );
            }
        }
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归：18830 是外部 VerneMQ broker 的端口，不是 edge-conductor 自身监听的端口，
    /// 绝不能出现在 edge-conductor.ports 里（否则会被误判为「本服务端口」并错误参与就绪探测）。
    #[test]
    fn edge_conductor_ports_exclude_external_mqtt() {
        let def = builtin()
            .into_iter()
            .find(|d| d.id == "edge-conductor")
            .expect("edge-conductor 必须存在");
        assert!(
            !def.ports.contains(&18830),
            "edge-conductor.ports 不应含外部 MQTT 18830，当前：{:?}",
            def.ports
        );
        assert_eq!(def.ports, vec![8084, 8085]);
    }

    /// 回归：processNames 为可选字段（缺省为空），camelCase rename 下 TOML 能正确读取。
    #[test]
    fn service_config_parses_process_names() {
        let cfg: ServiceConfig = toml::from_str(
            r#"
            [[services]]
            id = "x"
            name = "X"
            kind = "tauri_app"
            installDir = "/tmp/x"
            runCmd = "x"
            ports = []
            logPaths = []
            dependsOn = []
            isDaemon = false
            description = ""
            processNames = ["x-app"]
            "#,
        )
        .expect("解析 services TOML");
        assert_eq!(cfg.services[0].process_names, vec!["x-app".to_string()]);
    }

    /// 回归：桌面客户端必须声明 productNames（「已安装」检测依据，Windows 读卸载
    /// 注册表）；显示名按用户要求不带技术栈后缀（「空间数据驱动程序」）。
    /// 2026-10-14 Windows 实测：未安装却报「正常」——根因是检测模型缺「装没装」
    /// 维度且命令行兜底自匹配，此测试锁定声明面。
    #[test]
    fn biosphere_declares_product_names_and_display_name() {
        let d = builtin()
            .into_iter()
            .find(|d| d.id == "biosphere-spatial")
            .expect("biosphere-spatial 必须存在");
        assert_eq!(d.name, "空间数据驱动程序");
        assert_eq!(d.product_names, vec!["空间数据驱动".to_string()]);
        assert!(d.ports.is_empty() && !d.process_names.is_empty());
    }

    /// 回归：内置注册表必须包含第三方依赖 vernemq/minio/mysql/redis/greptime
    /// （thirdParty=true），且自研服务 thirdParty=false。Redis/VerneMQ/MinIO/GreptimeDB
    /// 额外声明 remoteOnly（纯远程部署模式：本机不安装，检测 = 「远程部署」配置页
    /// 托管的端点可达性），且不残留本地安装目录/进程名——防止远程化后又混回本地安装语义。
    #[test]
    fn builtin_contains_third_party_dependencies() {
        let defs = builtin();
        for id in ["vernemq", "minio", "mysql", "redis", "greptime"] {
            let d = defs.iter().find(|d| d.id == id).expect("第三方依赖必须内置");
            assert!(d.third_party, "{id} 应标记为 thirdParty");
            assert!(d.build_cmd.is_none() && d.run_cmd.is_empty(), "{id} 平台不托管启停");
        }
        for id in ["dfy-backend", "dfy-ui", "gateway-console", "edge-conductor"] {
            let d = defs.iter().find(|d| d.id == id).unwrap();
            assert!(!d.third_party, "{id} 是自研服务");
            assert!(!d.remote_only, "{id} 不是纯远程服务");
        }
        for id in ["redis", "vernemq", "minio", "greptime"] {
            let d = defs.iter().find(|d| d.id == id).unwrap();
            assert!(d.remote_only, "{id} 必须声明为 remoteOnly（安装与维护不再提供本地安装）");
            assert!(d.install_dir.is_empty(), "{id} 纯远程模式不应残留本地安装目录");
            assert!(d.process_names.is_empty(), "{id} 纯远程模式不按本机进程判定");
        }
    }

    /// 回归：覆盖配置按 id 合并——同 id 替换、新 id 追加、内置定义不丢。
    /// （此前是整体替换，新增内置 vernemq/minio 会被机器本机 services.toml 冲掉。）
    #[test]
    fn override_merges_by_id() {
        let mut base = builtin();
        let before = base.len();
        let overlay: ServiceConfig = toml::from_str(
            r#"
            [[services]]
            id = "dfy-backend"
            name = "本机后端"
            kind = "vertx"
            installDir = "/Users/x/dfy-boot-base"
            runCmd = "java -jar x.jar"
            ports = [48085]
            logPaths = []
            dependsOn = []
            isDaemon = true
            description = ""
            processNames = ["dfy-server.jar"]
            "#,
        )
        .unwrap();
        let overlay_len = overlay.services.len();
        merge_by_id(&mut base, overlay.services);
        assert_eq!(base.len(), before + overlay_len - 1, "同 id 替换、新 id 追加");
        assert_eq!(
            base.iter().find(|d| d.id == "dfy-backend").unwrap().name,
            "本机后端",
            "同 id 应被覆盖"
        );
        assert!(base.iter().any(|d| d.id == "vernemq"), "内置第三方不丢");
    }
}
