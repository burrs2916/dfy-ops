import { useEffect, useState } from "react";
import { Box, Stack } from "@mui/material";
import MySQLOps from "./MySQLOps";
import { PageHeader } from "../components/PageHeader";
import { api } from "../api";

/**
 * MySQL 管理（v0.2.46 从运维中心子页签升为顶级入口）。
 * mysqlInstalled 探测原在 OpsCenter，拆分后由本页自带。
 * null = 探测中：避免探测返回前误闪「未安装」引导。
 */
export default function MysqlPage() {
  const [mysqlInstalled, setMysqlInstalled] = useState<boolean | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .uninstallStatus()
      .then((s) => {
        if (!alive) return;
        setMysqlInstalled(s.targets.find((t) => t.target === "mysql")?.serviceInstalled ?? false);
      })
      .catch(() => {
        if (alive) setMysqlInstalled(true); // 探测失败不阻塞：按已安装处理，页面自身会报连接错误
      });
    return () => {
      alive = false;
    };
  }, []);

  return (
    <Stack spacing={2} sx={{ height: "100%", minHeight: 0 }}>
      <PageHeader
        title="MySQL 管理"
        description="库表浏览与维护、SQL 控制台、用户权限、导入备份——本机 MySQL 的日常运维工作台"
      />
      <Box sx={{ flex: 1, minHeight: 0 }}>
        <MySQLOps active mysqlInstalled={mysqlInstalled} />
      </Box>
    </Stack>
  );
}
