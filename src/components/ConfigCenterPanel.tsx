import { useEffect, useState } from "react";
import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Box,
  Button,
  Chip,
  Paper,
  Skeleton,
  Stack,
  Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import LanguageIcon from "@mui/icons-material/Language";
import TerminalIcon from "@mui/icons-material/Terminal";
import StorageIcon from "@mui/icons-material/Storage";
import BoltIcon from "@mui/icons-material/Bolt";
import SettingsInputAntennaIcon from "@mui/icons-material/SettingsInputAntenna";
import ArrowForwardIcon from "@mui/icons-material/ArrowForward";
import { api } from "../api";
import type { ExternalConfigResponse, NginxConfig } from "../types";
import { MONO_STACK } from "../theme";
import NginxConfigPanel from "./NginxConfigPanel";
import ExternalConfigPanel from "./ExternalConfigPanel";

/** 摘要行：label + 等宽值，一行一个关键参数 */
function SummaryRow({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <Stack direction="row" spacing={1.5} alignItems="baseline" sx={{ minWidth: 0 }}>
      <Typography variant="caption" sx={{ color: "text.secondary", flexShrink: 0, width: 72 }}>
        {label}
      </Typography>
      <Typography
        variant="body2"
        sx={{ fontFamily: MONO_STACK, wordBreak: "break-all", fontWeight: 500 }}
      >
        {value || "—"}
      </Typography>
    </Stack>
  );
}

/** 从 jdbc url 提取库名：jdbc:mysql://host:3306/dfy_base_db?... → dfy_base_db */
function dbNameFromUrl(url: string | undefined): string {
  if (!url) return "";
  const m = url.match(/\/([^/?]+)\??/);
  return m?.[1] ?? url;
}

export type OpsSubTab = "mysql" | "redis" | "deploy";

interface Props {
  /** 跳转到运维中心其他 tab（MySQL / Redis / 远程部署） */
  onGotoOpsTab: (tab: OpsSubTab) => void;
}

/** 配置中心：所有组件配置文件的统一入口。
 *  每张卡显示配置文件路径 + 关键参数摘要，展开即编辑（复用各组件配置面板）；
 *  中间件类（MySQL/Redis/VerneMQ）跳转对应管理页。安装前后的配置都从这里直观可见。 */
export default function ConfigCenterPanel({ onGotoOpsTab }: Props) {
  const [nginxCfg, setNginxCfg] = useState<ExternalConfigResponse | null>(null);
  const [backendCfg, setBackendCfg] = useState<ExternalConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    // nginx 与 dfy-backend 共用 external-config API（后端按 service id 分流）
    Promise.all([
      api.getExternalConfig("nginx").catch(() => null),
      api.getExternalConfig("dfy-backend").catch(() => null),
    ]).then(([n, b]) => {
      if (!alive) return;
      setNginxCfg(n);
      setBackendCfg(b);
      setLoading(false);
    });
    return () => {
      alive = false;
    };
  }, []);

  const nginx = (nginxCfg?.config as unknown as NginxConfig | undefined) ?? null;
  const backend = backendCfg?.config;
  const backendAny = backend as Record<string, string> | undefined;

  return (
    <Stack spacing={1.5} sx={{ overflow: "auto", minHeight: 0, pb: 1 }}>
      <Typography variant="body2" color="text.secondary">
        所有组件配置文件的统一入口：卡片直接显示<b>配置文件在磁盘上的路径</b>和关键参数摘要，
        展开即可编辑保存；中间件的连接配置跳转对应管理页。
      </Typography>

      {/* ===== Nginx 反向代理：conf/nginx.conf ===== */}
      <Paper variant="outlined" sx={{ overflow: "hidden" }}>
        <Stack direction="row" spacing={1.5} alignItems="center" sx={{ px: 2, py: 1.5 }}>
          <LanguageIcon color="primary" fontSize="small" />
          <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
            Nginx 反向代理
          </Typography>
          <Chip
            size="small"
            color={nginxCfg?.exists ? "success" : "default"}
            variant="outlined"
            label={nginxCfg?.exists ? "配置就绪" : "未安装 / 无配置"}
          />
        </Stack>
        <Box sx={{ px: 2, pb: 1.5, display: "grid", gap: 0.75 }}>
          {loading ? (
            <Skeleton height={20} />
          ) : (
            <>
              <SummaryRow label="配置文件" value={nginxCfg?.configPath} />
              <SummaryRow label="监听端口" value={nginx?.listenPort} />
              <SummaryRow label="server_name" value={nginx?.serverName} />
              <SummaryRow
                label="反代后端"
                value={
                  nginx?.backendHost
                    ? `http://${nginx.backendHost}:${nginx.backendPort}/admin-api/`
                    : null
                }
              />
            </>
          )}
        </Box>
        <Accordion elevation={0} sx={{ "&:before": { display: "none" }, borderTop: "1px solid", borderColor: "divider" }}>
          <AccordionSummary expandIcon={<ExpandMoreIcon />} sx={{ minHeight: 40, "& .MuiAccordionSummary-content": { my: 0.5 } }}>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              展开编辑 nginx.conf
            </Typography>
          </AccordionSummary>
          <AccordionDetails sx={{ pt: 0 }}>
            <NginxConfigPanel serviceId="nginx" />
          </AccordionDetails>
        </Accordion>
      </Paper>

      {/* ===== DFY 后端：config/application-local.yaml ===== */}
      <Paper variant="outlined" sx={{ overflow: "hidden" }}>
        <Stack direction="row" spacing={1.5} alignItems="center" sx={{ px: 2, py: 1.5 }}>
          <TerminalIcon color="primary" fontSize="small" />
          <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
            DFY 后端（Spring Boot）
          </Typography>
          <Chip
            size="small"
            color={backendCfg?.exists ? "success" : "default"}
            variant="outlined"
            label={backendCfg?.exists ? "外部配置生效中" : "未安装 / 用 jar 内置配置"}
          />
        </Stack>
        <Box sx={{ px: 2, pb: 1.5, display: "grid", gap: 0.75 }}>
          {loading ? (
            <Skeleton height={20} />
          ) : (
            <>
              <SummaryRow label="配置文件" value={backendCfg?.configPath} />
              <SummaryRow label="服务端口" value={backendAny?.serverPort} />
              <SummaryRow
                label="数据库"
                value={
                  backendAny?.mysqlMasterUrl
                    ? `${dbNameFromUrl(backendAny.mysqlMasterUrl)} @ ${
                        backendAny.mysqlMasterUrl.match(/\/\/([^:/]+):(\d+)/)?.[1] ?? "?"
                      }:${backendAny.mysqlMasterUrl.match(/\/\/[^:/]+:(\d+)/)?.[1] ?? "?"}`
                    : null
                }
              />
              <SummaryRow
                label="Redis"
                value={
                  backendAny?.redisHost
                    ? `${backendAny.redisHost}:${backendAny.redisPort}`
                    : null
                }
              />
            </>
          )}
        </Box>
        <Accordion elevation={0} sx={{ "&:before": { display: "none" }, borderTop: "1px solid", borderColor: "divider" }}>
          <AccordionSummary expandIcon={<ExpandMoreIcon />} sx={{ minHeight: 40, "& .MuiAccordionSummary-content": { my: 0.5 } }}>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              展开编辑 application-local.yaml
            </Typography>
          </AccordionSummary>
          <AccordionDetails sx={{ pt: 0 }}>
            <ExternalConfigPanel serviceId="dfy-backend" />
          </AccordionDetails>
        </Accordion>
      </Paper>

      {/* ===== 中间件跳转卡 ===== */}
      <Paper variant="outlined" sx={{ overflow: "hidden" }}>
        <Stack direction="row" spacing={1.5} alignItems="center" sx={{ px: 2, py: 1.5 }}>
          <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
            中间件连接配置
          </Typography>
          <Typography variant="caption" color="text.secondary">
            地址 / 端口 / 密码在各管理页配置
          </Typography>
        </Stack>
        <Stack sx={{ px: 2, pb: 2 }} spacing={1}>
          <Stack direction="row" spacing={1.5} alignItems="center">
            <StorageIcon fontSize="small" color="primary" />
            <Typography variant="body2" sx={{ flex: 1 }}>
              MySQL — 数据库 / 表 / SQL 查询 / 权限
            </Typography>
            <Button size="small" endIcon={<ArrowForwardIcon />} onClick={() => onGotoOpsTab("mysql")} sx={{ textTransform: "none" }}>
              去 MySQL 管理
            </Button>
          </Stack>
          <Stack direction="row" spacing={1.5} alignItems="center">
            <BoltIcon fontSize="small" color="primary" />
            <Typography variant="body2" sx={{ flex: 1 }}>
              Redis — 远程连接 / Key 浏览 / TTL
            </Typography>
            <Button size="small" endIcon={<ArrowForwardIcon />} onClick={() => onGotoOpsTab("redis")} sx={{ textTransform: "none" }}>
              去 Redis 管理
            </Button>
          </Stack>
          <Stack direction="row" spacing={1.5} alignItems="center">
            <SettingsInputAntennaIcon fontSize="small" color="primary" />
            <Typography variant="body2" sx={{ flex: 1 }}>
              VerneMQ / MinIO / GreptimeDB — 远程部署地址
            </Typography>
            <Button size="small" endIcon={<ArrowForwardIcon />} onClick={() => onGotoOpsTab("deploy")} sx={{ textTransform: "none" }}>
              去远程部署
            </Button>
          </Stack>
        </Stack>
      </Paper>
    </Stack>
  );
}
