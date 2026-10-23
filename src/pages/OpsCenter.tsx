import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Box, Stack, Tab, Tabs, Typography } from "@mui/material";
import { BoltOutlined, DnsOutlined, StorageOutlined } from "@mui/icons-material";
import MySQLOps from "./MySQLOps";
import RedisOps from "./RedisOps";
import { RemoteServicesPanel } from "../components/RemoteServicesPanel";
import { api } from "../api";
import { useOpsStore } from "../store";

type SubTab = "mysql" | "redis" | "deploy";

/**
 * 运维中心：数据库、缓存中间件的在线运维域（与「修复中心」同级的顶级页签）。
 * 修复中心专注环境安装与修复；本页专注已就绪中间件的日常运维——
 * MySQL（表/数据/权限/SQL 查询）、Redis（远程连接管理），
 * 以及远程部署（VerneMQ/MinIO/Redis 的远程地址配置与网段发现）。
 */
export default function OpsCenter() {
  // 修复中心问题项「去连接配置」引导：落地即落在 Redis 管理，并自动弹出连接配置
  const redisConfigPending = useOpsStore((s) => s.redisConfigPending);
  const setRedisConfigPending = useOpsStore((s) => s.setRedisConfigPending);
  const [searchParams, setSearchParams] = useSearchParams();
  const [subTab, setSubTab] = useState<SubTab>(() => {
    if (redisConfigPending) return "redis";
    const t = searchParams.get("tab");
    return t === "redis" || t === "deploy" ? t : "mysql";
  });

  const changeTab = (v: SubTab) => {
    setSubTab(v);
    // tab 写入 URL：待办清单等入口可以带 ?tab=deploy 直达远程部署
    setSearchParams(v === "mysql" ? {} : { tab: v }, { replace: true });
  };

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
          MySQL 与 Redis 的在线管理：表/数据/权限/SQL 查询、Key 浏览与 TTL 运维；远程部署集中配置虚拟机中间件地址
        </Typography>
      </Box>

      <Tabs
        value={subTab}
        onChange={(_, v: SubTab) => changeTab(v)}
        sx={{ "& .MuiTab-root": { textTransform: "none", minHeight: 44 }, flexShrink: 0 }}
      >
        <Tab value="mysql" icon={<StorageOutlined fontSize="small" />} iconPosition="start" label="MySQL 管理" />
        <Tab value="redis" icon={<BoltOutlined fontSize="small" />} iconPosition="start" label="Redis 管理" />
        <Tab value="deploy" icon={<DnsOutlined fontSize="small" />} iconPosition="start" label="远程部署" />
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

      {/* ===== 远程部署：虚拟机/远程主机上的第三方中间件地址集中配置 ===== */}
      <Box sx={{ flex: 1, minHeight: 0, display: subTab === "deploy" ? "flex" : "none", flexDirection: "column" }}>
        {subTab === "deploy" && <RemoteServicesPanel />}
      </Box>
    </Stack>
  );
}
