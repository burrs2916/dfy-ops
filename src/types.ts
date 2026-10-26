export type ServiceKind =
  | "vertx"
  | "vue_spa"
  | "angular_spa"
  | "rust_bin"
  | "tauri_app"
  | "mqtt_broker"
  | "object_storage";

/** 运行态三态（P0-2）：managed=本平台受管运行 / detected=外部进程在跑 / down=未运行 */
export type RunState = "managed" | "detected" | "down" | "needs-config";

export interface ServiceStatus {
  id: string;
  name: string;
  kind: ServiceKind;
  running: boolean;
  runState: RunState;
  healthy: boolean;
  ports: number[];
  dependsOn: string[];
  isDaemon: boolean;
  /** 第三方依赖（VerneMQ/MinIO 等系统级中间件）：平台只检测不托管，UI 隐藏启停按钮并分组展示 */
  thirdParty: boolean;
  /** 纯远程部署（Redis/VerneMQ/MinIO）：本机不安装，检测 = 托管端点可达性；UI 提示用远程承载语义 */
  remoteOnly: boolean;
  /** 「已安装」（Windows 卸载注册表判定）。false=确认未安装；未声明 productNames 的服务恒为 true（不适用） */
  installed: boolean;
  description: string;
  installDir: string;
  buildCmd?: string | null;
  runCmd: string;
  healthUrl?: string | null;
  logPaths: string[];
}

export interface DiagnosisResponse {
  status: Record<string, [boolean, boolean]>;
  envs: ServiceEnvReport[];
  findings: Finding[];
  llmSummary?: string | null;
  llmNote?: string | null;
}

export interface ActionResult {
  id: string;
  ok: boolean;
  error?: string | null;
}

export interface Finding {
  service: string;
  /** 与 Rust `Severity` 枚举对齐（仅 Warning/Critical；Rust 端不产 Info） */
  severity: "warning" | "critical";
  message: string;
  fixHint?: string | null;
}

export interface LogsResponse {
  id: string;
  logs: string;
}

export type CheckStatus = "pass" | "warn" | "fail";

export interface EnvCheckItem {
  /** 稳定 id（修复建议/前端键控），不随显示名变化 */
  id: string;
  name: string;
  category: string;
  status: CheckStatus;
  expected: string;
  actual: string;
  isFatal: boolean;
  fixHint: string;
}

export interface ServiceEnvReport {
  id: string;
  name: string;
  overall: CheckStatus;
  items: EnvCheckItem[];
}

/** ===== 环境修复（java 检查项自动安装等） ===== */

/** 修复任务状态（GET /api/env/repair/status，前端轮询） */
export interface EnvRepairJob {
  running: boolean;
  /** download → install → verify → done（完成时固定为 done） */
  stage: string;
  /** 任务类别（install/uninstall）：区分完成提示用 */
  op: string;
  /** 修复目标软件（java/mysql/redis/vernemq/…；null = 旧接口未标注） */
  target: string | null;
  logs: string[];
  ok: boolean | null;
  error: string | null;
}

/** POST /api/env/repair 的分级决策结果 */
export interface EnvRepairStart {
  ok: boolean;
  /** none=无需修复 / configure=本机已有达标 JDK 给配置指引 / install=启动自动安装 */
  action?: "none" | "configure" | "install";
  message?: string;
  home?: string;
  error?: string;
}

/** ===== AI 模型配置（三层：供应商→接入点→模型，web-craft ModelConfigPage 同构） ===== */

export interface AiProviderDto {
  id: string;
  name: string;
  apiKey: string;
  /** 后端脱敏后返回：true=已配置 Key（apiKey 字段恒为 ""，保存时留空表示保留原 Key） */
  keySet?: boolean;
  enabled: boolean;
}

export interface AiEndpointDto {
  id: string;
  providerId: string;
  name: string;
  /** openai-completions | anthropic-messages | ollama */
  apiType: string;
  baseUrl: string;
  enabled: boolean;
}

export interface AiModelDto {
  id: string;
  endpointId: string;
  name: string;
  /** 调用时传给 API 的模型标识 */
  refKey: string;
  enabled: boolean;
}

export interface AiModelConfigDto {
  providers: AiProviderDto[];
  endpoints: AiEndpointDto[];
  models: AiModelDto[];
  activeModelId: string | null;
}

export interface AiConfigResult {
  ok: boolean;
  config?: AiModelConfigDto;
  error?: string;
}

export interface AiTestResult {
  ok: boolean;
  message?: string;
  error?: string;
}

/** 应用自身设置（「设置」页；与 AI 助手配置分离）。 */
export interface AppSettings {
  statusRefreshSecs: number;
  envRefreshSecs: number;
  /** 本地资源目录：环境修复离线安装包（MySQL/Redis/JDK/VerneMQ 等安装包）所在目录；空 = 未配置 */
  resourceDir: string;
  /** 声明为「远程部署」的软件（如 ["redis","minio"]）：本机不安装，修复中心隐藏安装引导 */
  remoteDeployed: string[];
}

/** ===== 环境修复中心 ===== */

/** 资源目录里的一个安装包（GET /api/env/resources） */
export interface ResourceFile {
  fileName: string;
  path: string;
  sizeBytes: number;
  /** 识别出的目标软件；null = 未识别（仍展示，AI 方案环节会参考完整清单） */
  target: string | null;
  /** 是否位于该目标的约定子目录内（MySQL/、JDK/…）——约束查找优先命中这类位置 */
  inConventionDir: boolean;
}

/** 放错位置的包：在 A 的约定目录里放了文件名归属 B 的安装包（归属仍按文件名） */
export interface MisplacedItem {
  fileName: string;
  path: string;
  foundIn: string;
  dirTarget: string;
  fileTarget: string;
}

export interface ResourceScan {
  dir: string;
  exists: boolean;
  files: ResourceFile[];
  misplaced: MisplacedItem[];
}

/** POST /api/env/repair/generic 启动结果 */
export interface GenericRepairStart {
  ok: boolean;
  action?: "install";
  message?: string;
  error?: string;
}

/** AI 修复方案中的一个步骤（isExecutable = 命令在代执行白名单内，可确认后代执行） */
export interface RepairPlanStep {
  title: string;
  detail: string;
  command: string | null;
  isExecutable: boolean;
}

/** POST /api/env/repair/plan 返回：mode=ai 结构化方案 / text 原文兜底 / none 失败 */
export interface RepairPlan {
  ok: boolean;
  mode: "ai" | "text" | "none";
  summary: string;
  steps: RepairPlanStep[];
  error: string | null;
}

/** POST /api/env/repair/exec 代执行结果 */
export interface RepairExecResult {
  ok: boolean;
  error: string | null;
}

/** GET /api/env/repair/uninstall/status：一个可卸载组件的安装痕迹 */
export interface UninstallTargetStatus {
  target: string;
  label: string;
  installDir: string;
  /** 实测版本号（java：实跑 java -version；其他目标 null）——卡片直观展示用 */
  version: string | null;
  /** JAVA_HOME 是否已在系统配置（注册表核实：用户级/系统级任一）；java 用 */
  javaHomeConfigured: boolean;
  /** 用户级 PATH 是否已包含 {home}\bin（cmd 里 java 可用的前提）；java 用 */
  javaPathConfigured: boolean;
  serviceInstalled: boolean;
  dirExists: boolean;
  /** 服务运行态（MySQL/Redis 有服务；java 恒 false）——运维面板启停按钮依据 */
  serviceRunning: boolean;
  canUninstall: boolean;
}

/** POST /api/env/repair/uninstall 启动结果 */
export interface GenericUninstallStart {
  ok: boolean;
  action?: "uninstall";
  message?: string;
  error?: string;
}

// ===== MySQL 管理（运维中心） =====

/** GET /api/mysql/overview：连接信息 + 版本 + 库列表（服务未运行时降级） */
export interface MysqlOverview {
  running: boolean;
  host: string;
  port: number;
  user: string;
  hasPassword: boolean;
  version: string | null;
  databases: string[];
  /** 已连接但查询失败时给出原因（如托管密码与实际不符） */
  error?: string | null;
}

/** GET /api/mysql/tables 的一个表条目（行数为 information_schema 估算值） */
export interface MysqlTableInfo {
  name: string;
  rows: string;
  sizeKb: string;
  comment: string;
}

/** GET /api/mysql/users 的一个用户条目（grants 为 SHOW GRANTS 语句列表） */
export interface MysqlUserInfo {
  user: string;
  host: string;
  grants: string[];
}

/** POST /api/mysql/create-user 入参：权限档 ro 只读 / rw 读写 / all 全部 */
export interface MysqlCreateUserInput {
  user: string;
  host: string;
  password: string;
  privilege: "ro" | "rw" | "all";
  /** "*" 全部库，或具体库名 */
  database: string;
}

/** POST /api/mysql/exec 结果：查询语句返回结果集，非查询语句列为空 */
export interface SqlResult {
  ok: boolean;
  columns?: string[];
  rows?: string[][];
  rowCount?: number;
  error?: string;
}

/** 表结构 / processlist 等通用结果集（columns 为列名数组，rows 为字符串矩阵） */
export interface MysqlQueryResult {
  ok: boolean;
  columns?: string[];
  rows?: string[][];
  error?: string;
}

/** GET /api/mysql/data：表数据分页浏览（total 为筛选后总行数） */
export interface MysqlDataPage {
  ok: boolean;
  columns?: string[];
  rows?: string[][];
  total?: number;
  page?: number;
  size?: number;
  error?: string;
}

/** GET /api/mysql/variables 的一项常用参数 */
export interface MysqlVariableItem {
  name: string;
  value: string;
}

export interface MysqlBackupFile {
  file: string;
  sizeKb: number;
  /** 修改时间（Unix 秒） */
  modified: number;
}

/** GET /api/network/adapters：本机 IPv4 网卡（含 VMnet/vmnet 虚拟网卡） */
export interface NetworkAdapter {
  name: string;
  ip: string;
  /** 网段 CIDR（如 192.168.56.0/24），直接作为扫描输入 */
  cidr: string;
  hosts: number;
}

/** 网段扫描中某台主机上确认到的服务（协议级识别） */
export interface DiscoveredService {
  port: number;
  service: string;
  detail: string;
}

export interface DiscoveredHost {
  ip: string;
  services: DiscoveredService[];
}

/** POST /api/network/discover：网段扫描结果 */
export interface DiscoverResult {
  ok: boolean;
  cidr?: string;
  scanned?: number;
  elapsedMs?: number;
  hosts?: DiscoveredHost[];
  error?: string;
  missing?: string;
}

/** GET /api/redis/overview：INFO 解析结果（字段为原始字符串，缺失为空串） */
export interface RedisOverview {
  ok?: boolean;
  error?: string;
  version: string;
  mode: string;
  os: string;
  uptimeDays: string;
  usedMemory: string;
  usedMemoryPeak: string;
  maxMemory: string;
  connectedClients: string;
  blockedClients: string;
  totalConnections: string;
  totalCommands: string;
  aofEnabled: string;
  rdbLastStatus: string;
  totalKeys: number;
  totalExpires: number;
}

/** GET /api/redis/keys：Key 条目（kind：string/hash/list/set/zset/stream；ttl：-1 永久） */
export interface RedisKeyItem {
  name: string;
  kind: string;
  ttl: number;
}

/** GET /api/redis/key：Key 详情（rows 统一为 [标签, 值] 二元组） */
export interface RedisKeyDetail {
  name: string;
  kind: string;
  /** -1 永久、-2 不存在 */
  ttl: number;
  memory: number | null;
  total: number;
  rows: [string, string][];
}
