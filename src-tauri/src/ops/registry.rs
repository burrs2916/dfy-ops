use serde::{Deserialize, Serialize};

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
}

/// 被运维的本地服务定义。路径默认按 Windows 部署约定给出，可在 UI/配置中覆盖。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDef {
    pub id: String,
    pub name: String,
    #[serde(rename = "kind")]
    pub kind: ServiceKind,
    #[serde(rename = "installDir")]
    pub install_dir: String,
    /// 可选构建命令（首次或更新后执行）
    #[serde(rename = "buildCmd")]
    pub build_cmd: Option<String>,
    /// 运行命令（由进程管理器以 shell 执行）
    #[serde(rename = "runCmd")]
    pub run_cmd: String,
    pub ports: Vec<u16>,
    #[serde(rename = "healthUrl")]
    pub health_url: Option<String>,
    #[serde(rename = "logPaths")]
    pub log_paths: Vec<String>,
    #[serde(rename = "dependsOn")]
    pub depends_on: Vec<String>,
    /// 是否为常驻守护服务（Tauri 桌面客户端为 false，不参与保活）
    #[serde(rename = "isDaemon")]
    pub is_daemon: bool,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct ServiceConfig {
    services: Vec<ServiceDef>,
}

/// 返回内置的 5 个目标服务定义（DFY 系列本地部署）。
fn builtin() -> Vec<ServiceDef> {
    vec![
        ServiceDef {
            id: "dfy-backend".into(),
            name: "DFY 后端 (Vert.x)".into(),
            kind: ServiceKind::Vertx,
            install_dir: "C:/services/dfy-boot-base".into(),
            build_cmd: Some("mvn -q -DskipTests package".into()),
            run_cmd: "java -jar dfy-server/target/dfy-server.jar".into(),
            ports: vec![48085],
            health_url: None,
            log_paths: vec!["C:/services/dfy-boot-base/logs/app.log".into()],
            depends_on: vec![],
            is_daemon: true,
            description: "鼎飞益智后端服务（Java Vert.x），默认端口 48085。".into(),
        },
        ServiceDef {
            id: "dfy-ui".into(),
            name: "DFY 管理前端 (Vue3)".into(),
            kind: ServiceKind::VueSpa,
            install_dir: "C:/services/dfy-ui-admin-vue3-master".into(),
            build_cmd: Some("pnpm build:prod".into()),
            run_cmd: "nginx -p C:/services/dfy-ui-admin-vue3-master -c nginx.conf".into(),
            ports: vec![80],
            health_url: None,
            log_paths: vec!["C:/services/nginx/logs/access.log".into()],
            depends_on: vec!["dfy-backend".into()],
            is_daemon: true,
            description: "Vue3 管理后台，由 nginx 托管于 80 端口，反代 /api 到后端 48085。".into(),
        },
        ServiceDef {
            id: "gateway-console".into(),
            name: "Gateway Console (Angular)".into(),
            kind: ServiceKind::AngularSpa,
            install_dir: "C:/services/gateway-console".into(),
            build_cmd: Some("npm run build:prod".into()),
            run_cmd: "nginx -p C:/services/gateway-console -c nginx.conf".into(),
            ports: vec![8081],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: true,
            description: "Angular 19 网关控制台，静态托管。".into(),
        },
        ServiceDef {
            id: "edge-conductor".into(),
            name: "Edge Conductor (Rust Axum)".into(),
            kind: ServiceKind::RustBin,
            install_dir: "C:/services/edge-conductor".into(),
            build_cmd: Some("cargo build --release".into()),
            run_cmd:
                "C:/services/edge-conductor/target/release/edgecd-daemon --config C:/services/edge-conductor/config.toml".into(),
            ports: vec![8084, 8085],
            health_url: None,
            log_paths: vec!["C:/services/edge-conductor/logs".into()],
            depends_on: vec!["dfy-backend".into()],
            is_daemon: true,
            description: "边缘计算后端，自身监听 8084/8085；外部依赖 MQTT 18830（VerneMQ broker，非本服务端口），upstream 指向 DFY 后端 48085。".into(),
        },
        ServiceDef {
            id: "biosphere-spatial".into(),
            name: "Biosphere 空间系统 (Tauri)".into(),
            kind: ServiceKind::TauriApp,
            install_dir: "C:/services/biosphere-spatialdata-system".into(),
            build_cmd: Some("pnpm tauri build".into()),
            run_cmd:
                "C:/services/biosphere-spatialdata-system/target/release/biosphere-spatialdata-system.exe".into(),
            ports: vec![],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            is_daemon: false,
            description: "空间数据系统桌面客户端；非常驻服务，按版本/安装/启动运维。".into(),
        },
    ]
}

/// 读取服务注册表。
///
/// 优先使用环境变量 `DFY_OPS_SERVICES` 指向的 TOML 配置文件；
/// 文件缺失或解析失败时回退到内置默认定义。
pub fn services() -> Vec<ServiceDef> {
    if let Ok(path) = std::env::var("DFY_OPS_SERVICES") {
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<ServiceConfig>(&text) {
                Ok(cfg) => return cfg.services,
                Err(e) => eprintln!("服务配置解析失败（{path}）：{e}，回退内置默认"),
            },
            Err(e) => eprintln!("服务配置文件读取失败（{path}）：{e}，回退内置默认"),
        }
    }
    builtin()
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
}
