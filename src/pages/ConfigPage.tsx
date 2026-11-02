import { Box, Stack } from "@mui/material";
import { useNavigate } from "react-router-dom";
import ConfigCenterPanel, { type OpsSubTab } from "../components/ConfigCenterPanel";
import { PageHeader } from "../components/PageHeader";

/**
 * 配置中心（v0.2.46 从运维中心子页签升为顶级入口）：
 * 所有组件配置文件路径 + 参数摘要 + 展开编辑的统一入口。
 * 「去管理」跳转从子页签切换改为独立路由。
 */
export default function ConfigPage() {
  const navigate = useNavigate();
  const goto = (tab: OpsSubTab) => {
    if (tab === "redis") navigate("/redis");
    else if (tab === "deploy") navigate("/deploy");
    else navigate("/mysql");
  };

  return (
    <Stack spacing={2} sx={{ height: "100%", minHeight: 0 }}>
      <PageHeader
        title="配置中心"
        description="集中查看与编辑 MySQL / Redis / Nginx / 后端 / 前端各组件的配置文件"
      />
      <Box sx={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
        <ConfigCenterPanel onGotoOpsTab={goto} />
      </Box>
    </Stack>
  );
}
