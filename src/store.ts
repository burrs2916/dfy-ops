import { create } from "zustand";
import { api } from "./api";
import type { AppSettings, ServiceStatus } from "./types";

interface OpsState {
  services: ServiceStatus[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

export const useOpsStore = create<OpsState>((set) => ({
  services: [],
  loading: false,
  error: null,
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const services = await api.listServices();
      set({ services, loading: false });
    } catch (e) {
      set({
        loading: false,
        error: e instanceof Error ? e.message : String(e),
      });
    }
  },
}));

interface AppSettingsState {
  settings: AppSettings;
  loaded: boolean;
  load: () => Promise<void>;
  apply: (s: AppSettings) => void;
}

/** 应用设置全局状态：App 挂载时加载一次，设置页保存后经 apply 热更新（轮询间隔立即生效）。 */
export const useAppSettingsStore = create<AppSettingsState>((set) => ({
  settings: { statusRefreshSecs: 5, envRefreshSecs: 15 },
  loaded: false,
  load: async () => {
    try {
      const s = await api.appSettings();
      set({ settings: s, loaded: true });
    } catch {
      set({ loaded: true }); // 后端未就绪时保持默认值，静默降级
    }
  },
  apply: (s) => set({ settings: s, loaded: true }),
}));
