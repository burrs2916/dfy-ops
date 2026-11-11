import { create } from "zustand";
import { api } from "./api";
import type { AppSettings, ServiceEnvReport, ServiceStatus } from "./types";

interface OpsState {
  services: ServiceStatus[];
  envMap: Record<string, ServiceEnvReport>;
  /** 应用启动后的首次检测是否完成——全局状态，切换页面不重置，返回总览不再重复探测 */
  bootstrapped: boolean;
  /** 全量刷新进行中（手动「刷新」/ 应用首次启动检测） */
  loading: boolean;
  error: string | null;
  lastUpdate: Date | null;
  /** 修复中心问题项「去连接配置」→ Redis 管理页：落地后自动弹出连接配置（一次性） */
  redisConfigPending: boolean;
  /** 应用启动时检测一次；已检测过则直接复用缓存（页面切换零开销） */
  bootstrap: () => Promise<void>;
  /** 手动「刷新全部」：状态 + 环境 + 时间戳（状态与环境并行探测） */
  refreshAll: () => Promise<void>;
  /** 仅刷新环境（不重探服务状态）；force 重探，返回是否成功（供页面给出错误反馈） */
  refreshEnvAll: () => Promise<boolean>;
  /** 单服务刷新：状态 + 该服务环境，局部替换，不触发全量探测 */
  refreshServices: (ids: string[]) => Promise<void>;
  /** 标记「去连接配置」引导；Redis 管理页消费后由页面回调清除 */
  setRedisConfigPending: (v: boolean) => void;
}

export const useOpsStore = create<OpsState>((set, get) => ({
  services: [],
  envMap: {},
  bootstrapped: false,
  loading: false,
  error: null,
  lastUpdate: null,
  redisConfigPending: false,
  setRedisConfigPending: (v) => set({ redisConfigPending: v }),
  bootstrap: async () => {
    // 已检测过（应用启动后首次进入任意页面时完成）：直接复用缓存，
    // 切换页面 / 返回总览不再触发全量探测（此前每次 mount 都全量重探，
    // Windows 上一次要十几秒，「正在检测服务状态与环境…」反复出现）
    if (get().bootstrapped) return;
    await get().refreshAll();
  },
  refreshAll: async () => {
    set({ loading: true, error: null });
    try {
      // 状态与环境并行：二者都是后端全量探测，串行要等两份时间；
      // env force=true：手动刷新要新数据，不能被后端 60s 缓存吞掉
      const [services, envList] = await Promise.all([
        api.listServices(),
        api.envCheckAll(true),
      ]);
      const envMap: Record<string, ServiceEnvReport> = {};
      envList.forEach((r) => (envMap[r.id] = r));
      set({
        services,
        envMap,
        loading: false,
        bootstrapped: true,
        lastUpdate: new Date(),
      });
    } catch (e) {
      // allSettled 语义：任一失败也放行 bootstrap（错误由页面 Alert 呈现）
      set({
        loading: false,
        bootstrapped: true,
        error: e instanceof Error ? e.message : String(e),
      });
    }
  },
  refreshEnvAll: async () => {
    try {
      // force=true：环境页「重新检查」/轮询都要真实重探，不走后端缓存
      const envList = await api.envCheckAll(true);
      const envMap: Record<string, ServiceEnvReport> = {};
      envList.forEach((r) => (envMap[r.id] = r));
      set({ envMap, lastUpdate: new Date() });
      return true;
    } catch {
      // 环境检查依赖后端：轮询场景静默降级，手动复检场景由返回值驱动错误提示
      return false;
    }
  },
  refreshServices: async (ids) => {
    // 单服务组合刷新：后端一次快照同时产出状态与环境（原先 status+env 两个
    // 请求各采一份快照，Windows 上 tasklist/netstat 要跑两轮）；并行执行，
    // 失败保留旧值
    await Promise.all(
      ids.map(async (id) => {
        try {
          const { status: svc, env } = await api.refreshService(id);
          set((st) => ({
            services: st.services.map((s) => (s.id === id ? svc : s)),
            envMap: { ...st.envMap, [id]: env },
            lastUpdate: new Date(),
          }));
        } catch {
          /* 单服务探测失败：保留旧值 */
        }
      }),
    );
  },
}));

/** 修复任务 watcher（模块级，独立于组件生命周期）：安装/卸载/部署可能在用户
 *  切离环境检测页之后才完成——组件内轮询随卸载被清掉，就没有任何人重检，
 *  envMap 会一直停在任务前的旧结论。定时器活在模块层跨页面存活，任务结束时
 *  触发一次全量重检（force），保证「任务事件 → 重新检测」的唯一刷新时机。
 *  幂等：已在监视中直接返回；新修复任务启动时调用。 */
let repairWatchTimer: number | null = null;
export function ensureRepairWatch(): void {
  if (repairWatchTimer !== null) return;
  repairWatchTimer = window.setInterval(async () => {
    try {
      const job = await api.envRepairStatus();
      if (!job.running) {
        window.clearInterval(repairWatchTimer!);
        repairWatchTimer = null;
        // 任务结束 = 环境事实变了（装了/卸了/部署了）→ 全量重检一次
        void useOpsStore.getState().refreshAll();
      }
    } catch {
      /* 单次轮询失败忽略，下一轮重试 */
    }
  }, 2500);
}

interface AppSettingsState {
  settings: AppSettings;
  loaded: boolean;
  load: () => Promise<void>;
  apply: (s: AppSettings) => void;
}

/** 默认值：后端未持久化过、或返回的 JSON 缺少字段（老版本二进制/手改配置）时兜底。
 *  0 = 关闭自动轮询——启动时检测一次，此后按需手动/单服务刷新。 */
const DEFAULT_APP_SETTINGS: AppSettings = {
  statusRefreshSecs: 0,
  envRefreshSecs: 0,
  resourceDir: "",
  // 默认远程部署：VerneMQ（部署架构约定，不本地装）
  remoteDeployed: ["vernemq"],
  // 空串 = 云端服务目录用注册表内置默认值
  backendInstallDir: "",
};

/** 应用设置全局状态：App 挂载时加载一次，设置页保存后经 apply 热更新（轮询间隔立即生效）。 */
export const useAppSettingsStore = create<AppSettingsState>((set) => ({
  settings: DEFAULT_APP_SETTINGS,
  loaded: false,
  load: async () => {
    try {
      const s = await api.appSettings();
      // 字段级兜底：缺哪个字段就用默认值补上，避免新开关拿到 undefined
      set({ settings: { ...DEFAULT_APP_SETTINGS, ...s }, loaded: true });
    } catch {
      set({ loaded: true }); // 后端未就绪时保持默认值，静默降级
    }
  },
  apply: (s) => set({ settings: s, loaded: true }),
}));
