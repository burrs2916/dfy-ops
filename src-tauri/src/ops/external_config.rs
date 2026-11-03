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
    /// 运行日志文件路径（Spring Boot logging.file.name → logback ${LOG_FILE}）。
    /// 默认定死平台目录 C:/services/dfy-boot-base/logs/dfy-server.log（与
    /// registry logPaths 声明同源、一眼可见）；可视化「浏览」可换输出目录。
    #[serde(rename = "logFilePath", default)]
    pub log_file_path: String,
}

/// 外部配置文件路径：`{install_dir}/config/application-local.yaml`。
pub fn config_path(install_dir: &Path) -> PathBuf {
    install_dir.join("config").join("application-local.yaml")
}

/// 外部配置文件模板（v0.2.45 起为完整版）。
///
/// 背景：dfy-boot-base 打包已排除内置 application-local/dev.yaml（环境配置
/// 不进交付包），外部 `config/application-local.yaml` 成为 local profile
/// 唯一配置来源——薄模板会丢 autoconfigure.exclude（Druid 自动配置冲突）、
/// wx 公众号必填 app-id、iot client-id、Quartz 等启动必需项，后端必起不来。
/// 因此模板以源码 application-local.yaml 全量为底，仅把 MySQL/Redis/MQTT/
/// 日志四项替换为生产默认值（dfy_base_db / dfy123456 / 192.168.56.101），
/// 其余配置与开发环境保持一致。平台可视化只托管 12 个关键字段（parse/apply
/// 状态机），其他段落原样透传。
// 调用方（jar 安装管线）仅 Windows 编译，macOS dev 侧放行 dead_code
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const TEMPLATE: &str = r#"server:
  port: 48085

--- #################### 数据库相关配置 ####################
spring:
  main:
    allow-bean-definition-overriding: true
  # 数据源配置项
  autoconfigure:
    exclude:
      - com.alibaba.druid.spring.boot.autoconfigure.DruidDataSourceAutoConfigure # 排除 Druid 的自动配置，使用 dynamic-datasource-spring-boot-starter 配置多数据源
      - de.codecentric.boot.admin.server.config.AdminServerAutoConfiguration # 禁用 Spring Boot Admin 的 Server 的自动配置
      - de.codecentric.boot.admin.server.ui.config.AdminServerUiAutoConfiguration # 禁用 Spring Boot Admin 的 Server UI 的自动配置
      - de.codecentric.boot.admin.client.config.SpringBootAdminClientAutoConfiguration # 禁用 Spring Boot Admin 的 Client 的自动配置
  datasource:
    druid: # Druid 【监控】相关的全局配置
      web-stat-filter:
        enabled: true
      stat-view-servlet:
        enabled: true
        allow: # 设置白名单，不填则允许所有访问
        url-pattern: /druid/*
        login-username: # 控制台管理用户名和密码
        login-password:
      filter:
        stat:
          enabled: true
          log-slow-sql: true # 慢 SQL 记录
          slow-sql-millis: 100
          merge-sql: true
        wall:
          config:
            multi-statement-allow: true
    dynamic: # 多数据源配置
      druid: # Druid 【连接池】相关的全局配置
        initial-size: 1 # 初始连接数
        min-idle: 1 # 最小连接池数量
        max-active: 20 # 最大连接池数量
        max-wait: 600000 # 配置获取连接等待超时的时间，单位：毫秒
        time-between-eviction-runs-millis: 60000 # 配置间隔多久才进行一次检测，检测需要关闭的空闲连接，单位：毫秒
        min-evictable-idle-time-millis: 300000 # 配置一个连接在池中最小生存的时间，单位：毫秒
        max-evictable-idle-time-millis: 900000 # 配置一个连接在池中最大生存的时间，单位：毫秒
        validation-query: SELECT 1 FROM DUAL # 配置检测连接是否有效
        test-while-idle: true
        test-on-borrow: false
        test-on-return: false
      primary: master
      datasource:
        master:
          url: jdbc:mysql://127.0.0.1:3306/dfy_base_db?useSSL=false&serverTimezone=Asia/Shanghai&allowPublicKeyRetrieval=true&nullCatalogMeansCurrent=true&rewriteBatchedStatements=true # MySQL Connector/J 8.X 连接的示例
          username: root
          password: dfy123456
        slave: # 模拟从库，可根据自己需要修改
          lazy: true # 开启懒加载，保证启动速度
          url: jdbc:mysql://127.0.0.1:3306/dfy_base_db?useSSL=false&serverTimezone=Asia/Shanghai&allowPublicKeyRetrieval=true&rewriteBatchedStatements=true&nullCatalogMeansCurrent=true
          username: root
          password: dfy123456

  # Redis 配置。Redisson 默认的配置足够使用，一般不需要进行调优
  redis:
    host: 192.168.56.101 # 地址（Redis 为远程部署，默认指向 VM）
    port: 6379 # 端口
    database: 0 # 数据库索引
    password: "dfy123456" # 密码

--- #################### 定时任务相关配置 ####################

# Quartz 配置项，对应 QuartzProperties 配置类
spring:
  quartz:
    auto-startup: true # 本地开发环境，尽量不要开启 Job
    scheduler-name: schedulerName # Scheduler 名字。默认为 schedulerName
    job-store-type: jdbc # Job 存储器类型。默认为 memory 表示内存，可选 jdbc 使用数据库。
    wait-for-jobs-to-complete-on-shutdown: true # 应用关闭时，是否等待定时任务执行完成。默认为 false ，建议设置为 true
    properties: # 添加 Quartz Scheduler 附加属性，更多可以看 http://www.quartz-scheduler.org/documentation/2.4.0-SNAPSHOT/configuration.html 文档
      org:
        quartz:
          # Scheduler 相关配置
          scheduler:
            instanceName: schedulerName
            instanceId: AUTO # 自动生成 instance ID
          # JobStore 相关配置
          jobStore:
            # JobStore 实现类。可见博客：https://blog.csdn.net/weixin_42458219/article/details/122247162
            class: org.springframework.scheduling.quartz.LocalDataSourceJobStore
            isClustered: true # 是集群模式
            clusterCheckinInterval: 15000 # 集群检查频率，单位：毫秒。默认为 15000，即 15 秒
            misfireThreshold: 60000 # misfire 阀值，单位：毫秒。
          # 线程池相关配置
          threadPool:
            threadCount: 25 # 线程池大小。默认为 10 。
            threadPriority: 5 # 线程优先级
            class: org.quartz.simpl.SimpleThreadPool # 线程池类型
    jdbc: # 使用 JDBC 的 JobStore 的时候，JDBC 的配置
      initialize-schema: ALWAYS # 是否自动使用 SQL 初始化 Quartz 表结构。这里设置成 never ，我们手动创建表结构。

--- #################### 消息队列相关 ####################

# rocketmq 配置项，对应 RocketMQProperties 配置类
rocketmq:
  name-server: 127.0.0.1:9876 # RocketMQ Namesrv

spring:
  # RabbitMQ 配置项，对应 RabbitProperties 配置类
  rabbitmq:
    host: 127.0.0.1 # RabbitMQ 服务的地址
    port: 5672 # RabbitMQ 服务的端口
    username: rabbit # RabbitMQ 服务的账号
    password: rabbit # RabbitMQ 服务的密码
  # Kafka 配置项，对应 KafkaProperties 配置类
  kafka:
    bootstrap-servers: 127.0.0.1:9092 # 指定 Kafka Broker 地址，可以设置多个，以逗号分隔

--- #################### 服务保障相关配置 ####################

# Lock4j 配置项
lock4j:
  acquire-timeout: 3000 # 获取分布式锁超时时间，默认为 3000 毫秒
  expire: 30000 # 分布式锁的超时时间，默认为 30 毫秒

--- #################### 监控相关配置 ####################

# Actuator 监控端点的配置项
management:
  endpoints:
    web:
      base-path: /actuator # Actuator 提供的 API 接口的根目录。默认为 /actuator
      exposure:
        include: '*' # 需要开放的端点。默认值只打开 health 和 info 两个端点。通过设置 * ，可以开放所有端点。

# Spring Boot Admin 配置项
spring:
  boot:
    admin:
      # Spring Boot Admin Client 客户端的相关配置
      client:
        url: http://127.0.0.1:${server.port}/${spring.boot.admin.context-path} # 设置 Spring Boot Admin Server 地址
        instance:
          service-host-type: IP # 注册实例时，优先使用 IP [IP, HOST_NAME, CANONICAL_HOST_NAME]
      # Spring Boot Admin Server 服务端的相关配置
      context-path: /admin # 配置 Spring

# 日志文件配置
logging:
  file:
    name: C:/services/dfy-boot-base/logs/dfy-server.log # 运行日志文件（Spring Boot 注入 logback ${LOG_FILE}，10MB 按天滚动保留 30 天）
  charset:
    console: UTF-8 # 日志输出编码显式定死 UTF-8：Windows 缺省 GBK，不指定则文件/控制台中文全乱码
    file: UTF-8
  level:
    # 配置自己写的 MyBatis Mapper 打印日志
    cn.iocoder.dfy.module.bpm.dal.mysql: debug
    cn.iocoder.dfy.module.infra.dal.mysql: debug
    cn.iocoder.dfy.module.infra.dal.mysql.logger.ApiErrorLogMapper: INFO # 配置 ApiErrorLogMapper 的日志级别为 info，避免和 GlobalExceptionHandler 重复打印
    cn.iocoder.dfy.module.infra.dal.mysql.job.JobLogMapper: INFO # 配置 JobLogMapper 的日志级别为 info
    cn.iocoder.dfy.module.infra.dal.mysql.file.FileConfigMapper: INFO # 配置 FileConfigMapper 的日志级别为 info
    cn.iocoder.dfy.module.pay.dal.mysql: debug
    cn.iocoder.dfy.module.pay.dal.mysql.notify.PayNotifyTaskMapper: INFO # 配置 PayNotifyTaskMapper 的日志级别为 info
    cn.iocoder.dfy.module.system.dal.mysql: debug
    cn.iocoder.dfy.module.system.dal.mysql.sms.SmsChannelMapper: INFO # 配置 SmsChannelMapper 的日志级别为 info
    cn.iocoder.dfy.module.tool.dal.mysql: debug
    cn.iocoder.dfy.module.member.dal.mysql: debug
    cn.iocoder.dfy.module.trade.dal.mysql: debug
    cn.iocoder.dfy.module.promotion.dal.mysql: debug
    cn.iocoder.dfy.module.statistics.dal.mysql: debug
    cn.iocoder.dfy.module.crm.dal.mysql: debug
    cn.iocoder.dfy.module.erp.dal.mysql: debug
    cn.iocoder.dfy.module.iot.dal.mysql: debug
    cn.iocoder.dfy.module.ai.dal.mysql: debug
    org.springframework.context.support.PostProcessorRegistrationDelegate: ERROR # TODO lwc：先禁用，Spring Boot 3.X 存在部分错误的 WARN 提示

debug: false

--- #################### 微信公众号、小程序相关配置 ####################
wx:
  mp: # 公众号配置（必填），参见 https://github.com/Wechat-Group/WxJava/blob/develop/spring-boot-starters/wx-java-mp-spring-boot-starter/README.md 文档
    app-id: wx5b23ba7a5589ecbb # 测试号（自己的）
    secret: 2a7b3b20c537e52e74afd395eb85f61f
    # 存储配置，解决 AccessToken 的跨节点的共享
    config-storage:
      type: RedisTemplate # 采用 RedisTemplate 操作 Redis，会自动从 Spring 中获取
      key-prefix: wx # Redis Key 的前缀
      http-client-type: HttpClient # 采用 HttpClient 请求微信公众号平台
  miniapp: # 小程序配置（必填），参见 https://github.com/Wechat-Group/WxJava/blob/develop/spring-boot-starters/wx-java-miniapp-spring-boot-starter/README.md 文档
    appid: wx66186af0759f47c9 # 测试号（puhui 提供的）
    secret: 3218bcbd112cbc614c7264ceb20144ac
    config-storage:
      type: RedisTemplate # 采用 RedisTemplate 操作 Redis，会自动从 Spring 中获取
      key-prefix: wa # Redis Key 的前缀
      http-client-type: HttpClient # 采用 HttpClient 请求微信公众号平台

--- #################### 鼎飞益智相关配置 ####################

# 鼎飞益智配置项，设置当前项目所有自定义的配置
dfy:
  captcha:
    enable: false # 本地环境，暂时关闭图片验证码，方便登录等接口的测试；
  security:
    mock-enable: true
  access-log: # 访问日志的配置项
    enable: false
  demo: false # 关闭演示模式
  wxa-code:
    env-version: develop # 小程序版本: 正式版为 "release"；体验版为 "trial"；开发版为 "develop"
  wxa-subscribe-message:
    miniprogram-state: developer # 跳转小程序类型：开发版为 “developer”；体验版为 “trial”为；正式版为 “formal”
  tencent-lbs-key: TVDBZ-TDILD-4ON4B-PFDZA-RNLKH-VVF6E # QQ 地图的密钥 https://lbs.qq.com/service/staticV2/staticGuide/staticDoc

justauth:
  enabled: true
  type:
    DINGTALK: # 钉钉
      client-id: dingvrnreaje3yqvzhxg
      client-secret: i8E6iZyDvZj51JIb0tYsYfVQYOks9Cq1lgryEjFRqC79P3iJcrxEwT6Qk2QvLrLI
      ignore-check-redirect-uri: true
    WECHAT_ENTERPRISE: # 企业微信
      client-id: wwd411c69a39ad2e54
      client-secret: 1wTb7hYxnpT2TUbIeHGXGo7T0odav1ic10mLdyyATOw
      agent-id: 1000004
      ignore-check-redirect-uri: true
    # noinspection SpringBootApplicationYaml
    WECHAT_MINI_APP: # 微信小程序
      client-id: ${wx.miniapp.appid}
      client-secret: ${wx.miniapp.secret}
      ignore-check-redirect-uri: true
      ignore-check-state: true # 微信小程序，不会使用到 state，所以不进行校验
    WECHAT_MP: # 微信公众号
      client-id: ${wx.mp.app-id}
      client-secret: ${wx.mp.secret}
      ignore-check-redirect-uri: true
  cache:
    type: REDIS
    prefix: 'social_auth_state:' # 缓存前缀，目前只对 Redis 缓存生效，默认 JUSTAUTH::STATE::
    timeout: 24h # 超时时长，目前只对 Redis 缓存生效，默认 3 分钟

--- #################### iot相关配置 ####################
iot:
  verneMq:
    # 账号
    username: cloud
    # 密码
    password: key@00
    # 主机地址
    hostUrl: tcp://192.168.56.101:18830
    # 客户端Id，不能相同，采用随机数 ${random.value}
    client-id: ${random.int}
    # 默认主题
    default-topic: test
    # 保持连接
    keepalive: 60
    # 清除会话(设置为false,断开连接，重连后使用原来的会话 保留订阅的主题，能接收离线期间的消息)
    clearSession: true


--- #################### 插件相关配置 ####################
pf4j:
  pluginsDir: ${user.home}/plugins
"#;

/// 从模板创建外部配置（安装管线用）：write 要求模板已存在，而安装时
/// install_dir 是全新目录——以 [`TEMPLATE`] 为底，apply 用户确认的值后原子落盘。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn create(install_dir: &Path, cfg: &ExternalConfig) -> Result<PathBuf, String> {
    let path = config_path(install_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建配置目录 {} 失败：{e}", parent.display()))?;
    }
    let new_text = apply(TEMPLATE, cfg);
    let dups = find_duplicate_keys(&new_text);
    if !dups.is_empty() {
        return Err(format!(
            "生成外部配置时发现重复键（模板或输入值异常）：{}",
            dups.join("；")
        ));
    }
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, &new_text).map_err(|e| format!("写入临时文件失败：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换配置文件失败：{e}"))?;
    Ok(path)
}

/// 当前段落（跨多文档 yaml 唯一的段落 key）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Section {
    Root,
    Server,
    Datasource,
    Master,
    Slave,
    Redis,
    Logging,
    LogFile,
    VerneMq,
    /// 平台不托管的段落（rabbitmq/kafka/quartz/wx/dfy/justauth…）。
    /// 完整模板里 spring.rabbitmq 也有 host/port/username/password——不显式
    /// 隔离会被误认成 Redis 段改写（v0.2.45 随完整模板一并修复）。
    Other,
}

/// 根据 yaml 行内容推进段落状态机。
fn advance_section(sec: &mut Section, trimmed: &str, indent: usize) {
    match trimmed {
        "server:" => *sec = Section::Server,
        "datasource:" => *sec = Section::Datasource,
        "master:" => *sec = Section::Master,
        "slave:" => *sec = Section::Slave,
        "redis:" if indent <= 4 => *sec = Section::Redis, // spring.redis（缩进 2）
        "logging:" if indent <= 4 => *sec = Section::Logging,
        "file:" if *sec == Section::Logging => *sec = Section::LogFile,
        "verneMq:" => *sec = Section::VerneMq,
        // 完整模板里的其他段落：全部归入 Other，与托管段彻底隔离
        "rabbitmq:" | "kafka:" | "quartz:" | "rocketmq:" | "wx:" | "mp:" | "miniapp:"
        | "config-storage:" | "dfy:" | "captcha:" | "security:" | "access-log:"
        | "wxa-code:" | "wxa-subscribe-message:" | "justauth:" | "type:" | "cache:"
        | "pf4j:" | "management:" | "endpoints:" | "web:" | "exposure:" | "lock4j:"
        | "admin:" | "boot:" | "client:" | "instance:" | "main:" | "autoconfigure:"
        | "exclude:" | "druid:" | "dynamic:" | "stat-view-servlet:" | "filter:"
        | "stat:" | "wall:" | "config:" | "jobStore:" | "threadPool:" | "scheduler:"
        | "properties:" | "jdbc:" | "org:" => *sec = Section::Other,
        "iot:" | "spring:" => {}
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
        (Section::LogFile, "name") => Some("log_file_path"),
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

/// 扫描 yaml 文本中的重复键（同一父路径下同一叶子键出现两次且都未注释）。
/// 返回人类可读的问题列表（空 = 干净）。背景：交付方手改过 jar 内置
/// application-local.yaml，引入了同层两个 `password`，新版 SnakeYAML
/// 严格校验直接 DuplicateKeyException 拒绝启动——必须在启动前把这类
/// 问题点名出来，而不是让用户对着满屏框架栈猜。
/// v0.2.45 改为完整路径跟踪：按缩进维护父路径栈，重复 = 同父路径 + 同键名。
/// 完整模板里 wx.mp.config-storage.type 与 wx.miniapp.config-storage.type
/// 路径不同不误报；master/slave 的 username/password 同理。
pub fn find_duplicate_keys(text: &str) -> Vec<String> {
    // (缩进, 键) 栈：当前叶子键的父路径
    let mut stack: Vec<(usize, String)> = Vec::new();
    // (父路径, 键) -> 首次行号
    let mut seen: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    let mut problems = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("---") {
            continue;
        }
        let indent = line.len() - trimmed.len();
        // 弹掉缩进不小于当前行的栈层，栈顶即父级
        while let Some(&(top_indent, _)) = stack.last() {
            if top_indent >= indent {
                stack.pop();
            } else {
                break;
            }
        }
        let (body, _c) = split_comment(trimmed);
        let Some((raw_key, val)) = body.split_once(':') else {
            continue;
        };
        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }
        if val.trim().is_empty() {
            // 段落键：入栈继续下探
            stack.push((indent, key.to_string()));
            continue;
        }
        let parent = stack
            .iter()
            .map(|(_, k)| k.as_str())
            .collect::<Vec<_>>()
            .join(".");
        match seen.get(&(parent.clone(), key.to_string())) {
            Some(first_no) => {
                problems.push(format!(
                    "第 {line_no} 行的 `{key}` 与第 {first_no} 行重复（同层级两个同名键，Spring Boot 新版 SnakeYAML 会拒绝启动）"
                ));
            }
            None => {
                seen.insert((parent, key.to_string()), line_no);
            }
        }
    }
    problems
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
                advance_section(&mut sec, body.trim_end(), indent);
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
                    "log_file_path" => cfg.log_file_path = v,
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
                advance_section(&mut sec, body.trim_end(), indent);
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
                    "log_file_path" => &new_cfg.log_file_path,
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
    let dups = find_duplicate_keys(&new_text);
    if !dups.is_empty() {
        return Err(format!(
            "外部配置存在重复键，已阻止写入（否则后端启动即退）：{}",
            dups.join("；")
        ));
    }
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

    /// find_duplicate_keys：点名同层重复键（交付方手改 jar 配置引入的实际故障）。
    #[test]
    fn duplicate_keys_are_reported_with_line_numbers() {
        // 第 4/5 行同层两个 password（模拟手改配置时把注释行的 # 弄丢）
        let bad = "spring:\n  redis:\n    host: 127.0.0.1\n    password: \"a\"\n    password: b\n";
        let dups = find_duplicate_keys(bad);
        assert_eq!(dups.len(), 1, "{dups:?}");
        assert!(dups[0].contains("password"), "{dups:?}");
        assert!(dups[0].contains("第 5 行"), "{dups:?}");
        // 干净文件零告警；注释行不算键
        assert!(find_duplicate_keys(SAMPLE).is_empty(), "{:?}", find_duplicate_keys(SAMPLE));
        // master/slave 同名 password 是不同段，不算重复
        assert!(find_duplicate_keys(TEMPLATE).is_empty(), "{:?}", find_duplicate_keys(TEMPLATE));
        // 完整模板里 wx.mp.config-storage.type 与 wx.miniapp.config-storage.type
        // 同名但路径不同——路径跟踪版不误报
        assert!(find_duplicate_keys(TEMPLATE).is_empty());
    }

    /// 完整模板（v0.2.45）：parse 提取生产默认值。
    #[test]
    fn parse_full_template_extracts_production_values() {
        let c = parse(TEMPLATE);
        assert_eq!(c.server_port, "48085");
        assert!(c.mysql_master_url.contains("dfy_base_db"), "{}", c.mysql_master_url);
        assert_eq!(c.mysql_username, "root");
        assert_eq!(c.mysql_password, "dfy123456");
        assert!(c.mysql_slave_url.contains("dfy_base_db"), "{}", c.mysql_slave_url);
        assert_eq!(c.redis_host, "192.168.56.101");
        assert_eq!(c.redis_password, "dfy123456");
        assert_eq!(c.log_file_path, "C:/services/dfy-boot-base/logs/dfy-server.log");
        assert_eq!(c.vernemq_host_url, "tcp://192.168.56.101:18830");
        assert_eq!(c.vernemq_username, "cloud");
        assert_eq!(c.vernemq_password, "key@00");
    }

    /// 完整模板 apply 回归：平台不托管的段落（rabbitmq 的 host/port/password 等）
    /// 绝不能被 Redis/Master 状态机误改；启动必需项原样透传；同值 apply 幂等。
    #[test]
    fn apply_full_template_keeps_unmanaged_sections_intact() {
        let cfg = parse(TEMPLATE);
        let out = apply(TEMPLATE, &cfg);
        // rabbitmq 段原样（曾因 sec 滞留 Redis 被误改的雷）
        assert!(out.contains("host: 127.0.0.1 # RabbitMQ 服务的地址"), "rabbit host 被误改");
        assert!(out.contains("port: 5672 # RabbitMQ 服务的端口"), "rabbit port 被误改");
        assert!(out.contains("password: rabbit # RabbitMQ 服务的密码"), "rabbit password 被误改");
        // 启动必需项透传
        assert!(out.contains("DruidDataSourceAutoConfigure"), "autoconfigure.exclude 丢失");
        assert!(out.contains("client-id: ${random.int}"), "iot client-id 丢失");
        assert!(out.contains("app-id: wx5b23ba7a5589ecbb"), "wx 公众号配置丢失");
        // Redis 段仍是生产值（未被 rabbit 污染）
        assert!(out.contains("host: 192.168.56.101 # 地址（Redis 为远程部署"), "redis host 被误改");
        // 幂等：同 cfg 再 apply 文本不变
        assert_eq!(out, apply(&out, &cfg), "apply 不幂等");
    }

    /// create：从生产模板 + 用户确认值生成外部配置；空目录可建、值全部落盘。
    #[test]
    fn create_writes_template_with_overrides() {
        let base = std::env::temp_dir().join(format!("dfy-ops-ext-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let cfg = ExternalConfig {
            server_port: "48086".into(),
            ..parse(TEMPLATE)
        };
        let path = create(&base, &cfg).unwrap();
        assert!(path.is_file(), "配置文件应已生成");
        let back = parse(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(back.server_port, "48086");
        assert_eq!(back.mysql_master_url, parse(TEMPLATE).mysql_master_url);
        assert_eq!(back.redis_password, "dfy123456");
        assert_eq!(back.vernemq_host_url, "tcp://192.168.56.101:18830");
        assert_eq!(
            back.log_file_path, "C:/services/dfy-boot-base/logs/dfy-server.log",
            "运行日志路径必须写进外部配置（logging.file.name → logback ${{LOG_FILE}}）"
        );
        std::fs::remove_dir_all(&base).ok();
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
