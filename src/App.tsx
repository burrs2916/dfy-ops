import { useEffect } from "react";
import { Routes, Route } from "react-router-dom";
import Layout from "./components/Layout";
import Dashboard from "./pages/Dashboard";
import ServiceDetail from "./pages/ServiceDetail";
import EnvCheck from "./pages/EnvCheck";
import OpsCenter from "./pages/OpsCenter";
import Settings from "./pages/Settings";
import AiAssistant from "./pages/AiAssistant";
import Topology from "./pages/Topology";
import { useAppSettingsStore, useOpsStore } from "./store";

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
        <Route path="ops" element={<OpsCenter />} />
        <Route path="topology" element={<Topology />} />
        <Route path="ai" element={<AiAssistant />} />
        <Route path="settings" element={<Settings />} />
        <Route path="service/:id" element={<ServiceDetail />} />
      </Route>
    </Routes>
  );
}
