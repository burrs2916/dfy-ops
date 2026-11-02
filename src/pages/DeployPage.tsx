import { Box, Stack } from "@mui/material";
import RemoteServicesPanel from "../components/RemoteServicesPanel";
import { PageHeader } from "../components/PageHeader";

/**
 * 远程部署（v0.2.46 从运维中心子页签升为顶级入口）：
 * 虚拟机/远程主机上的第三方中间件（VerneMQ/MinIO/Redis）地址集中配置与网段发现。
 */
export default function DeployPage() {
  return (
    <Stack spacing={2} sx={{ height: "100%", minHeight: 0 }}>
      <PageHeader
        title="远程部署"
        description="远程中间件（VerneMQ / MinIO / Redis）的地址配置与网段发现"
      />
      <Box sx={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
        <RemoteServicesPanel />
      </Box>
    </Stack>
  );
}
