import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControl,
  LinearProgress,
  MenuItem,
  Select,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Tooltip,
  TextField,
  Typography,
} from "@mui/material";
import { Hub, Radar, TravelExplore } from "@mui/icons-material";
import { api } from "../api";
import type { DiscoveredHost, NetworkAdapter } from "../types";

/** 后端识别名 → 服务 id（采用时写对应远程端点配置） */
const SERVICE_ID: Record<string, string> = {
  Redis: "redis",
  VerneMQ: "vernemq",
  MinIO: "minio",
};

/** 参与发现的目标服务说明（引导文案用，点名用户能得到什么） */
const TARGET_LABEL: Record<string, string> = {
  redis: "Redis（6379）",
  vernemq: "VerneMQ（18830）",
  minio: "MinIO（9000）",
};

export interface ServiceDiscoveryDialogProps {
  open: boolean;
  onClose: () => void;
  /** 参与发现与采用的服务 id（如 ["redis"] 或 ["vernemq","minio"]） */
  targets: string[];
  /**
   * true：采用只回填 onAdopted(host, port)，不写后端——用于「连接配置」等
   * 还有其他待填字段（如密码）的场景，由用户确认后统一保存；
   * false（默认）：采用即写入远程端点配置（服务卡片场景）。
   */
  fillOnly?: boolean;
  /** 采用成功后回调（host：被采用主机的 IP，port：命中服务的端口），父级用于刷新状态 */
  onAdopted?: (host: string, port?: number) => void;
}

/**
 * 服务发现对话框：网卡网段选择 → 网段扫描 → 一键采用。
 *
 * 引导式交互（产品要求）：每一步缺前置条件时给出明确补救指引，
 * 绝不甩笼统报错——未选网段时按钮置灰并说明"缺这步就无法发现哪些服务"，
 * 扫不到时给出逐条排查清单。
 */
export function ServiceDiscoveryDialog({
  open,
  onClose,
  targets,
  fillOnly = false,
  onAdopted,
}: ServiceDiscoveryDialogProps) {
  const [adapters, setAdapters] = useState<NetworkAdapter[] | null>(null);
  const [cidr, setCidr] = useState("");
  const [manualMode, setManualMode] = useState(false);
  const [manualCidr, setManualCidr] = useState("");
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hosts, setHosts] = useState<DiscoveredHost[] | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const [adopted, setAdopted] = useState<string | null>(null);
  const [adopting, setAdopting] = useState(false);

  const targetText = targets.map((t) => TARGET_LABEL[t] ?? t).join("、");
  // 生效网段：下拉模式取选中项，手动模式取输入框
  const activeCidr = manualMode ? manualCidr.trim() : cidr;

  useEffect(() => {
    if (!open) return;
    setAdapters(null);
    setCidr("");
    setManualMode(false);
    setManualCidr("");
    setScanning(false);
    setError(null);
    setHosts(null);
    setSummary(null);
    setAdopted(null);
    // 并行拉网卡列表与上次使用的网段：上次网段在网卡列表里则直接选中，
    // 否则（网段变化后手填过的）切手动模式带入——用户改网段无需每次重填
    void Promise.all([
      api.networkAdapters().then((r) => r.adapters).catch(() => [] as NetworkAdapter[]),
      api.scanConfig().then((r) => r.lastCidr).catch(() => ""),
    ]).then(([list, last]) => {
      setAdapters(list);
      if (last) {
        if (list.some((a) => a.cidr === last)) {
          setCidr(last);
        } else {
          setManualMode(true);
          setManualCidr(last);
        }
      }
    });
  }, [open]);

  const scan = async () => {
    if (!activeCidr || scanning) return;
    setScanning(true);
    setError(null);
    setHosts(null);
    setSummary(null);
    setAdopted(null);
    try {
      const r = await api.discoverServices(activeCidr);
      if (!r.ok) {
        setError(r.error ?? "扫描失败，请稍后重试");
      } else {
        setHosts(r.hosts ?? []);
        setSummary(`已扫描 ${r.scanned ?? 0} 台主机 · 耗时 ${((r.elapsedMs ?? 0) / 1000).toFixed(1)} 秒`);
        // 扫描成功即记住该网段：网段变化后下次打开无需重填
        void api.saveScanConfig(activeCidr).catch(() => undefined);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setScanning(false);
    }
  };

  // 行级采用：该主机上与 targets 匹配的服务全部处理
  //（fillOnly 只回填表单由父级保存；否则直接写入远程端点配置）
  const adopt = async (host: DiscoveredHost) => {
    const hits = host.services.filter((s) => targets.includes(SERVICE_ID[s.service] ?? ""));
    if (hits.length === 0 || adopting) return;
    setAdopting(true);
    setError(null);
    if (fillOnly) {
      setAdopted(`已填入 ${host.ip}:${hits[0].port}，请确认后保存连接配置`);
      onAdopted?.(host.ip, hits[0].port);
      setAdopting(false);
      return;
    }
    try {
      for (const s of hits) {
        const r = await api.saveServiceRemote(SERVICE_ID[s.service] ?? "", host.ip, s.port);
        if (!r.ok) {
          setError(r.error ?? "保存远程端点失败");
          return;
        }
      }
      setAdopted(`已采用 ${host.ip}（${hits.map((s) => s.service).join("、")}），服务状态即将刷新`);
      onAdopted?.(host.ip);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setAdopting(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ fontWeight: 700, display: "flex", alignItems: "center", gap: 1 }}>
        <Radar color="primary" />
        服务发现
      </DialogTitle>
      <DialogContent>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2 }}>
          ① 选择网段 → ② 扫描 → ③ 采用。平台将逐台探测网段内 {targetText} 是否可达，确认后一键填入对应服务的远程地址。
        </Typography>

        <Stack spacing={2}>
          <Stack direction="row" spacing={1.5} alignItems="center">
            <Typography variant="body2" sx={{ whiteSpace: "nowrap", fontWeight: 600 }}>
              扫描网段
            </Typography>
            <Button
              size="small"
              variant="outlined"
              startIcon={<TravelExplore />}
              disabled={!activeCidr || scanning}
              onClick={() => void scan()}
              sx={{ whiteSpace: "nowrap", flexShrink: 0 }}
            >
              {scanning ? "扫描中…" : "开始扫描"}
            </Button>
            {manualMode ? (
              <TextField
                size="small"
                sx={{ flex: 1, minWidth: 0 }}
                placeholder="手动输入网段，如 192.168.56.0/24"
                value={manualCidr}
                disabled={scanning}
                onChange={(e) => setManualCidr(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void scan();
                }}
                InputProps={{
                  endAdornment: (
                    <Button
                      size="small"
                      onClick={() => {
                        setManualMode(false);
                        setManualCidr("");
                      }}
                      disabled={scanning}
                      sx={{ whiteSpace: "nowrap", flexShrink: 0 }}
                    >
                      改选网卡
                    </Button>
                  ),
                }}
              />
            ) : (
              <FormControl size="small" sx={{ flex: 1, minWidth: 0 }} disabled={scanning || adapters === null}>
                <Select
                  value={cidr}
                  onChange={(e) => {
                    if (e.target.value === "__manual__") {
                      setManualMode(true);
                    } else {
                      setCidr(e.target.value);
                    }
                  }}
                  displayEmpty
                >
                  <MenuItem value="">
                    <em>{adapters === null ? "正在读取本机网卡…" : "请选择本机网卡对应的网段…"}</em>
                  </MenuItem>
                  {adapters?.map((a, i) => (
                    <MenuItem key={`${a.cidr}-${i}`} value={a.cidr}>
                      {a.name} · {a.ip}（{a.cidr}，{a.hosts} 台主机）
                    </MenuItem>
                  ))}
                  <MenuItem value="__manual__" sx={{ borderTop: 1, borderColor: "divider" }}>
                    <em>手动输入网段…</em>
                  </MenuItem>
                </Select>
              </FormControl>
            )}
          </Stack>

          {/* 引导核心：未选网段是流程缺口，明说缺什么、补完能做什么，而不是等报错 */}
          {!activeCidr && (
            <Alert severity="info" icon={<Hub fontSize="inherit" />}>
              {manualMode
                ? "请输入要扫描的网段（格式 a.b.c.d/前缀，如 192.168.56.0/24）——输入后平台将逐台探测该网段内的目标服务端口。"
                : "尚未选择扫描网段——没有网段，平台无法逐台探测 " + targetText + " 所在的主机。请在上方选择一张网卡（虚拟机常用 VirtualBox Host-Only / VMware VMnet 等虚拟网卡网段），或切换到手动输入。"}
            </Alert>
          )}

          {scanning && (
            <Stack spacing={1}>
              <LinearProgress />
              <Typography variant="caption" color="text.secondary">
                正在扫描 {cidr}（约 3~5 秒，请稍候）…
              </Typography>
            </Stack>
          )}

          {error && (
            <Alert severity="warning" onClose={() => setError(null)}>
              {error}
            </Alert>
          )}

          {adopted && (
            <Alert severity="success" onClose={() => setAdopted(null)}>
              {adopted}
            </Alert>
          )}

          {hosts !== null && !scanning && (
            hosts.length === 0 ? (
              <Alert severity="warning">
                <Typography variant="body2" sx={{ fontWeight: 600, mb: 0.5 }}>
                  该网段未发现 {targetText}
                </Typography>
                <Typography variant="caption" component="div">
                  请依次确认：
                  <br />
                  1. 虚拟机已开机，且对应服务已启动；
                  <br />
                  2. 网段选择正确——NAT 模式的虚拟机不在物理网卡网段，请改选 VMnet8 / vmnet 虚拟网卡网段；
                  <br />
                  3. 虚拟机防火墙已放行 6379 / 18830 / 9000 端口。
                </Typography>
              </Alert>
            ) : (
              <>
                <Typography variant="caption" color="text.secondary">
                  {summary}
                </Typography>
                <TableContainer sx={{ border: 1, borderColor: "divider", borderRadius: 1.5, maxHeight: 320 }}>
                  <Table size="small" stickyHeader>
                    <TableHead>
                      <TableRow>
                        <TableCell sx={{ fontWeight: 700 }}>主机</TableCell>
                        <TableCell sx={{ fontWeight: 700 }}>发现的服务</TableCell>
                        <TableCell align="right" sx={{ fontWeight: 700 }}>操作</TableCell>
                      </TableRow>
                    </TableHead>
                    <TableBody>
                      {hosts.map((h) => {
                        const hits = h.services.filter((s) => targets.includes(SERVICE_ID[s.service] ?? ""));
                        return (
                          <TableRow key={h.ip} hover>
                            <TableCell sx={{ fontFamily: "monospace" }}>{h.ip}</TableCell>
                            <TableCell>
                              <Stack spacing={0.5}>
                                {h.services.map((s) => (
                                  <Stack key={s.port} direction="row" spacing={1} alignItems="center">
                                    <Chip
                                      size="small"
                                      color={targets.includes(SERVICE_ID[s.service] ?? "") ? "primary" : "default"}
                                      variant={targets.includes(SERVICE_ID[s.service] ?? "") ? "filled" : "outlined"}
                                      label={`${s.service} : ${s.port}`}
                                    />
                                    <Typography variant="caption" color="text.secondary">
                                      {s.detail}
                                    </Typography>
                                  </Stack>
                                ))}
                              </Stack>
                            </TableCell>
                            <TableCell align="right">
                              <Tooltip title={hits.length > 0 ? `把 ${h.ip} 写入 ${hits.map((s) => s.service).join("、")} 的远程地址` : "该主机上没有本次发现的目标服务"}>
                                <span>
                                  <Button
                                    size="small"
                                    variant="contained"
                                    disabled={hits.length === 0 || adopting}
                                    onClick={() => void adopt(h)}
                                  >
                                    采用
                                  </Button>
                                </span>
                              </Tooltip>
                            </TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                </TableContainer>
              </>
            )
          )}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} variant="contained">
          完成
        </Button>
      </DialogActions>
    </Dialog>
  );
}
