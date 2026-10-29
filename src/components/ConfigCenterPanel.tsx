import { useEffect, useState } from "react";
import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Box,
  Button,
  Chip,
  Divider,
  Grid,
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
import TuneOutlinedIcon from "@mui/icons-material/TuneOutlined";
import HubsOutlinedIcon from "@mui/icons-material/HubsOutlined";
import ArrowForwardIcon from "@mui/icons-material/ArrowForward";
import EditNoteOutlinedIcon from "@mui/icons-material/EditNoteOutlined";
import { api } from "../api";
import type { ExternalConfigResponse, NginxConfig } from "../types";
import { MONO_STACK } from "../theme";
import NginxConfigPanel from "./NginxConfigPanel";
import ExternalConfigPanel from "./ExternalConfigPanel";

/** 分区标题：圆底图标 + 标题 + 一句话说明（全页统一节奏） */
function Section({ icon, title, hint }: { icon: React.ReactNode; title: string; hint: string }) {
  return (
    <Stack direction="row" spacing={1.25} alignItems="center" sx={{ mt: 1 }}>
      <Box
        sx={{
          width: 30,
          height: 30,
          borderRadius: 1.5,
          display: "grid",
          placeItems: "center",
          bgcolor: "primary.main",
          color: "primary.contrastText",
          flexShrink: 0,
        }}
      >
        {icon}
      </Box>
      <Box sx={{ minWidth: 0 }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 700, lineHeight: 1.2 }}>
          {title}
        </Typography>
        <Typography variant="caption" color="text.secondary">
          {hint}
        </Typography>
      </Box>
    </Stack>
  );
}

/** 摘要行：固定标签列 + 等宽值列，多行对齐 */
function SummaryRow({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <Stack direction="row" spacing={1.5} alignItems="baseline" sx={{ minWidth: 0 }}>
      <Typography
        variant="caption"
        sx={{ color: "text.secondary", flexShrink: 0, width: 68, textAlign: "right" }}
      >
        {label}
      </Typography>
      <Typography
        variant="body2"
        sx={{ fontFamily: MONO_STACK, wordBreak: "break-all", fontWeight: 500, lineHeight: 1.5 }}
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

/** 从 jdbc url 提取 host:port */
function hostFromUrl(url: string | undefined): string {
  if (!url) return "";
  return url.match(/\/\/([^/?]+)/)?.[1] ?? url;
}

export type OpsSubTab = "mysql" | "redis" | "deploy";

interface Props {
  /** 跳转到运维中心其他 tab（MySQL / Redis / 远程部署） */
  onGotoOpsTab: (tab: OpsSubTab) => void;
}

/** 配置中心：所有组件配置文件的统一入口，分区规划——
 *  ① 应用配置（Nginx / DFY 后端）：磁盘路径 + 参数摘要，展开即编辑；
 *  ② 中间件连接：MySQL / Redis / 远程中间件直达各管理页。
 *  安装前弹窗可配、安装后这里常驻可查可改。 */
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
  const backendAny = backendCfg?.config as Record<string, string> | undefined;

  const middlewareCards = [
    {
      key: "mysql",
      icon: <StorageIcon fontSize="small" color="primary" />,
      title: "MySQL",
      desc: "数据库 / 表结构 / SQL 查询 / 权限",
      tab: "mysql" as const,
      btn: "去 MySQL 管理",
    },
    {
      key: "redis",
      icon: <BoltIcon fontSize="small" color="primary" />,
      title: "Redis",
      desc: "远程连接 / Key 浏览 / TTL 运维",
      tab: "redis" as const,
      btn: "去 Redis 管理",
    },
    {
      key: "remote",
      icon: <SettingsInputAntennaIcon fontSize="small" color="primary" />,
      title: "远程中间件",
      desc: "VerneMQ / MinIO / GreptimeDB 地址",
      tab: "deploy" as const,
      btn: "去远程部署",
    },
  ];

  return (
    <Stack spacing={0.5} sx={{ overflow: "auto", minHeight: 0, pb: 2 }}>
      {/* ===== ① 应用配置 ===== */}
      <Section
        icon={<TuneOutlinedIcon fontSize="small" />}
        title="应用配置"
        hint="卡片显示配置文件在磁盘上的位置与关键参数，展开即可编辑，保存立即落盘"
      />
      <Grid container spacing={1.5} sx={{ mt: 0.25 }}>
        {/* --- Nginx --- */}
        <Grid size={{ xs: 12, md: 6 }}>
          <Paper
            variant="outlined"
            sx={{ height: "100%", display: "flex", flexDirection: "column", overflow: "hidden" }}
          >
            <Stack direction="row" spacing={1.25} alignItems="center" sx={{ px: 2, pt: 1.75, pb: 0.75 }}>
              <LanguageIcon color="primary" fontSize="small" />
              <Typography variant="subtitle2" sx={{ fontWeight: 700, flex: 1 }}>
                Nginx 反向代理
              </Typography>
              <Chip
                size="small"
                color={nginxCfg?.exists ? "success" : "default"}
                variant="outlined"
                sx={{ height: 22 }}
                label={nginxCfg?.exists ? "配置就绪" : "未安装"}
              />
            </Stack>
            <Box sx={{ px: 2, pb: 1.5, display: "grid", gap: 0.5 }}>
              {loading ? (
                <>
                  <Skeleton height={18} width="80%" />
                  <Skeleton height={18} width="65%" />
                </>
              ) : (
                <>
                  <SummaryRow label="配置文件" value={nginxCfg?.configPath} />
                  <SummaryRow label="监听端口" value={nginx?.listenPort} />
                  <SummaryRow label="域名" value={nginx?.serverName} />
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
            <Box sx={{ flex: 1 }} />
            <Accordion
              elevation={0}
              disableGutters
              sx={{ "&:before": { display: "none" }, borderTop: "1px solid", borderColor: "divider", flexShrink: 0 }}
            >
              <AccordionSummary
                expandIcon={<ExpandMoreIcon />}
                sx={{ minHeight: 42, px: 2, "& .MuiAccordionSummary-content": { my: 0.5, alignItems: "center", gap: 0.75 } }}
              >
                <EditNoteOutlinedIcon fontSize="small" color="primary" />
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  编辑 nginx.conf
                </Typography>
              </AccordionSummary>
              <AccordionDetails sx={{ pt: 0, px: 2, pb: 1.5 }}>
                <NginxConfigPanel serviceId="nginx" />
              </AccordionDetails>
            </Accordion>
          </Paper>
        </Grid>

        {/* --- DFY 后端 --- */}
        <Grid size={{ xs: 12, md: 6 }}>
          <Paper
            variant="outlined"
            sx={{ height: "100%", display: "flex", flexDirection: "column", overflow: "hidden" }}
          >
            <Stack direction="row" spacing={1.25} alignItems="center" sx={{ px: 2, pt: 1.75, pb: 0.75 }}>
              <TerminalIcon color="primary" fontSize="small" />
              <Typography variant="subtitle2" sx={{ fontWeight: 700, flex: 1 }}>
                DFY 后端（Spring Boot）
              </Typography>
              <Chip
                size="small"
                color={backendCfg?.exists ? "success" : "default"}
                variant="outlined"
                sx={{ height: 22 }}
                label={backendCfg?.exists ? "外部配置生效中" : "未安装"}
              />
            </Stack>
            <Box sx={{ px: 2, pb: 1.5, display: "grid", gap: 0.5 }}>
              {loading ? (
                <>
                  <Skeleton height={18} width="80%" />
                  <Skeleton height={18} width="65%" />
                </>
              ) : (
                <>
                  <SummaryRow label="配置文件" value={backendCfg?.configPath} />
                  <SummaryRow label="服务端口" value={backendAny?.serverPort} />
                  <SummaryRow
                    label="数据库"
                    value={
                      backendAny?.mysqlMasterUrl
                        ? `${dbNameFromUrl(backendAny.mysqlMasterUrl)} @ ${hostFromUrl(backendAny.mysqlMasterUrl)}`
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
            <Box sx={{ flex: 1 }} />
            <Accordion
              elevation={0}
              disableGutters
              sx={{ "&:before": { display: "none" }, borderTop: "1px solid", borderColor: "divider", flexShrink: 0 }}
            >
              <AccordionSummary
                expandIcon={<ExpandMoreIcon />}
                sx={{ minHeight: 42, px: 2, "& .MuiAccordionSummary-content": { my: 0.5, alignItems: "center", gap: 0.75 } }}
              >
                <EditNoteOutlinedIcon fontSize="small" color="primary" />
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  编辑 application-local.yaml
                </Typography>
              </AccordionSummary>
              <AccordionDetails sx={{ pt: 0, px: 2, pb: 1.5 }}>
                <ExternalConfigPanel serviceId="dfy-backend" />
              </AccordionDetails>
            </Accordion>
          </Paper>
        </Grid>
      </Grid>

      <Divider sx={{ mt: 2.5 }} />

      {/* ===== ② 中间件连接 ===== */}
      <Section
        icon={<HubsOutlinedIcon fontSize="small" />}
        title="中间件连接"
        hint="地址、端口、密码在各自管理页配置，从这里直达"
      />
      <Grid container spacing={1.5} sx={{ mt: 0.25 }}>
        {middlewareCards.map((m) => (
          <Grid size={{ xs: 12, sm: 4 }} key={m.key}>
            <Paper
              variant="outlined"
              sx={{ height: "100%", px: 2, py: 1.75, display: "flex", flexDirection: "column", gap: 0.5 }}
            >
              <Stack direction="row" spacing={1} alignItems="center">
                {m.icon}
                <Typography variant="subtitle2" sx={{ fontWeight: 700, flex: 1 }}>
                  {m.title}
                </Typography>
              </Stack>
              <Typography variant="body2" color="text.secondary" sx={{ flex: 1 }}>
                {m.desc}
              </Typography>
              <Button
                size="small"
                variant="outlined"
                endIcon={<ArrowForwardIcon />}
                onClick={() => onGotoOpsTab(m.tab)}
                sx={{ textTransform: "none", alignSelf: "flex-start" }}
              >
                {m.btn}
              </Button>
            </Paper>
          </Grid>
        ))}
      </Grid>
    </Stack>
  );
}
