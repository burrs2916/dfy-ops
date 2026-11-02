import { Box, Stack } from "@mui/material";
import RedisOps from "./RedisOps";
import { PageHeader } from "../components/PageHeader";
import { useOpsStore } from "../store";

/**
 * Redis 管理（v0.2.46 从运维中心子页签升为顶级入口）。
 * Redis 为纯远程部署模式：本页管理远程连接与键值运维。
 */
export default function RedisPage() {
  const redisConfigPending = useOpsStore((s) => s.redisConfigPending);
  const setRedisConfigPending = useOpsStore((s) => s.setRedisConfigPending);

  return (
    <Stack spacing={2} sx={{ height: "100%", minHeight: 0 }}>
      <PageHeader
        title="Redis 管理"
        description="远程 Redis 连接配置与键值运维（Redis 不装本机，统一连接远程端点）"
      />
      <Box sx={{ flex: 1, minHeight: 0 }}>
        <RedisOps
          active
          autoOpenConfig={redisConfigPending}
          onAutoOpenConsumed={() => setRedisConfigPending(false)}
        />
      </Box>
    </Stack>
  );
}
