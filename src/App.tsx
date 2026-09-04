import { useEffect } from "react";
import { Routes, Route } from "react-router-dom";
import Layout from "./components/Layout";
import Dashboard from "./pages/Dashboard";
import ServiceDetail from "./pages/ServiceDetail";
import EnvCheck from "./pages/EnvCheck";
import Settings from "./pages/Settings";
import AiAssistant from "./pages/AiAssistant";
import Topology from "./pages/Topology";
import { useAppSettingsStore } from "./store";

export default function App() {
  const loadAppSettings = useAppSettingsStore((s) => s.load);
  useEffect(() => {
    loadAppSettings();
  }, [loadAppSettings]);

  return (
    <Routes>
      <Route path="/" element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="env" element={<EnvCheck />} />
        <Route path="topology" element={<Topology />} />
        <Route path="ai" element={<AiAssistant />} />
        <Route path="settings" element={<Settings />} />
        <Route path="service/:id" element={<ServiceDetail />} />
      </Route>
    </Routes>
  );
}
