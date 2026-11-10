import { useEffect } from "react";
import { Navigate, Route, Routes, useSearchParams } from "react-router-dom";
import Layout from "./components/Layout";
import Dashboard from "./pages/Dashboard";
import ServiceDetail from "./pages/ServiceDetail";
import EnvCheck from "./pages/EnvCheck";
import MonitorCenter from "./pages/MonitorCenter";
import MysqlPage from "./pages/MysqlPage";
import RedisPage from "./pages/RedisPage";
import ConfigPage from "./pages/ConfigPage";
import DeployPage from "./pages/DeployPage";
import Settings from "./pages/Settings";
import AiAssistant from "./pages/AiAssistant";
import Topology from "./pages/Topology";
import { useAppSettingsStore, useOpsStore } from "./store";

/** 旧「运维中心 /ops?tab=x」链接兼容：v0.2.46 起各子页签已是独立路由 */
function OpsRedirect() {
  const [sp] = useSearchParams();
  const tab = sp.get("tab");
  const to =
    tab === "deploy" ? "/deploy" : tab === "redis" ? "/redis" : tab === "config" ? "/config" : "/mysql";
  return <Navigate to={to} replace />;
}

export default function App() {
  const loadAppSettings = useAppSettingsStore((s) => s.load);
  const bootstrap = useOpsStore((s) => s.bootstrap);
  useEffect(() => {
    loadAppSettings();
    // 应用启动时检测一次服务状态与环境（store 级去重：切页面/返回总览不再重复探测）
    void bootstrap();
  }, [loadAppSettings, bootstrap]);

  return (
    <Routes>
      <Route path="/" element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="env" element={<EnvCheck />} />
        <Route path="monitor" element={<MonitorCenter />} />
        <Route path="mysql" element={<MysqlPage />} />
        <Route path="redis" element={<RedisPage />} />
        <Route path="config" element={<ConfigPage />} />
        <Route path="deploy" element={<DeployPage />} />
        <Route path="ops" element={<OpsRedirect />} />
        <Route path="topology" element={<Topology />} />
        <Route path="ai" element={<AiAssistant />} />
        <Route path="settings" element={<Settings />} />
        <Route path="service/:id" element={<ServiceDetail />} />
      </Route>
    </Routes>
  );
}
