import { useEffect, useState } from "react";
import { Box, Stack, Tab, Tabs, Typography } from "@mui/material";
import { BoltOutlined, StorageOutlined } from "@mui/icons-material";
import MySQLOps from "./MySQLOps";
import RedisOps from "./RedisOps";
import { api } from "../api";
import { useOpsStore } from "../store";

/**
 * 运维中心：数据库与缓存中间件的在线运维域（与「修复中心」同级的顶级页签）。
 * 修复中心专注环境安装与修复；本页专注已就绪中间件的日常运维——
 * MySQL（表/数据/权限/SQL 查询）与 Redis（远程连接管理）。
 */
export default function OpsCenter() {
  // 修复中心问题项「去连接配置」引导：落地即落在 Redis 管理，并自动弹出连接配置
  const redisConfigPending = useOpsStore((s) => s.redisConfigPending);
  const setRedisConfigPending = useOpsStore((s) => s.setRedisConfigPending);
  const [subTab, setSubTab] = useState<"mysql" | "redis">(() =>
    redisConfigPending ? "redis" : "mysql",
  );

  // MySQL 安装状态：MySQLOps 据此区分「未安装引导」与「完整运维面板」
  const [mysqlInstalled, setMysqlInstalled] = useState(false);
  useEffect(() => {
    let alive = true;
    api
      .uninstallStatus()
      .then((s) => {
        if (!alive) return;
        setMysqlInstalled(s.targets.find((t) => t.target === "mysql")?.serviceInstalled ?? false);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  return (
    <Stack spacing={2} sx={{ height: "100%", minHeight: 0 }}>
      <Box sx={{ flexShrink: 0, mb: -0.5 }}>
        <Typography variant="h5" sx={{ fontWeight: 700 }}>
          运维中心
        </Typography>
        <Typography variant="body2" color="text.secondary">
          MySQL 与 Redis 的在线管理：表/数据/权限/SQL 查询、Key 浏览与 TTL 运维
        </Typography>
      </Box>

      <Tabs
        value={subTab}
        onChange={(_, v: "mysql" | "redis") => setSubTab(v)}
        sx={{ "& .MuiTab-root": { textTransform: "none", minHeight: 44 }, flexShrink: 0 }}
      >
        <Tab value="mysql" icon={<StorageOutlined fontSize="small" />} iconPosition="start" label="MySQL 管理" />
        <Tab value="redis" icon={<BoltOutlined fontSize="small" />} iconPosition="start" label="Redis 管理" />
      </Tabs>

      {/* ===== MySQL 管理：CSS 隐藏挂载，切页不丢状态 ===== */}
      <Box sx={{ flex: 1, minHeight: 0, display: subTab === "mysql" ? undefined : "none" }}>
        <MySQLOps active={subTab === "mysql"} mysqlInstalled={mysqlInstalled} />
      </Box>

      {/* ===== Redis 管理（远程连接模式）：CSS 隐藏挂载，全高度工作台 ===== */}
      <Box sx={{ flex: 1, minHeight: 0, display: subTab === "redis" ? undefined : "none" }}>
        <RedisOps
          active={subTab === "redis"}
          autoOpenConfig={redisConfigPending}
          onAutoOpenConsumed={() => setRedisConfigPending(false)}
        />
      </Box>
    </Stack>
  );
}
