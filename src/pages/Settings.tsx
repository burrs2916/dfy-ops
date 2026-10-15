import { useEffect, useState } from "react";
import { Alert, Button, Divider, MenuItem, Paper, Select, Snackbar, Stack, Table, TableBody, TableCell, TableRow, TextField, Typography } from "@mui/material";
import { Tune, Storage, Info, FolderOpen } from "@mui/icons-material";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useAppSettingsStore } from "../store";
import { SettingsLayout } from "../components/SettingsLayout";
import { SettingRow } from "../components/SettingRow";
import type { AppSettings, ResourceScan } from "../types";

/**
 * 设置：应用自身配置（通用 / 资源目录 / 数据 / 关于）。
 * 与「AI助手」严格分离——LLM 配置在 AI 助手页，此处不涉及。
 * 布局与其他设置类页面共用 SettingsLayout（web-craft 风格）。
 */

// 0 = 关闭自动轮询（默认）：启动时检测一次，此后按需手动/单服务刷新
const STATUS_OPTIONS = [0, 3, 5, 10, 30];
const ENV_OPTIONS = [0, 10, 15, 30, 60];

const intervalLabel = (v: number) => (v === 0 ? "关闭" : `${v} 秒`);

const mb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;

export default function Settings() {
  const { settings, loaded, apply } = useAppSettingsStore();
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saving, setSaving] = useState(false);
  const [scan, setScan] = useState<ResourceScan | null>(null);
  const [scanning, setScanning] = useState(false);
  const [snack, setSnack] = useState<{ open: boolean; msg: string; severity: "success" | "error" }>({
    open: false,
    msg: "",
    severity: "success",
  });

  useEffect(() => {
    if (loaded) setDraft(settings);
  }, [loaded, settings]);

  const save = async () => {
    setSaving(true);
    try {
      const r = await api.saveAppSettings(draft);
      if (r.ok) {
        apply(draft);
        setSnack({ open: true, msg: "已保存，设置立即生效", severity: "success" });
      } else {
        setSnack({ open: true, msg: "保存失败：" + (r.error ?? "未知错误"), severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: "保存失败：" + (e instanceof Error ? e.message : String(e)), severity: "error" });
    } finally {
      setSaving(false);
    }
  };

  // 扫描预览：对输入框里的目录（未保存也行）即时识别安装包，配置时直接看到效果
  const doScan = async () => {
    const dir = draft.resourceDir.trim();
    if (!dir) {
      setScan(null);
      return;
    }
    setScanning(true);
    try {
      setScan(await api.resources(dir));
    } catch {
      setScan(null);
    } finally {
      setScanning(false);
    }
  };

  // 原生目录选择：选中即填入并自动扫描预览（免去手动输入路径与再点一次扫描）
  const pickDir = async () => {
    let picked: string | null = null;
    try {
      const r = await open({ directory: true, multiple: false, title: "选择本地资源目录" });
      picked = typeof r === "string" ? r : null;
    } catch {
      return; // 用户取消或对话框异常：保持现状
    }
    if (!picked) return;
    setDraft((d) => ({ ...d, resourceDir: picked as string }));
    setScanning(true);
    try {
      setScan(await api.resources(picked));
    } catch {
      setScan(null);
    } finally {
      setScanning(false);
    }
  };

  const clearAck = () => {
    try {
      localStorage.removeItem("dfy-ops:ack");
      setSnack({ open: true, msg: "已清除全部「已知悉」记录", severity: "success" });
    } catch {
      setSnack({ open: true, msg: "清除失败", severity: "error" });
    }
  };

  const dirty =
    draft.statusRefreshSecs !== settings.statusRefreshSecs ||
    draft.envRefreshSecs !== settings.envRefreshSecs ||
    draft.resourceDir !== settings.resourceDir;

  const resourceScanBody =
    scan === null ? null : (
      <Paper variant="outlined" sx={{ mt: 1.5, p: 1.5, bgcolor: "action.hover" }}>
        {!scan.exists ? (
          <Typography variant="body2" color="text.secondary">
            目录不存在：{scan.dir}
          </Typography>
        ) : scan.files.length === 0 ? (
          <Typography variant="body2" color="text.secondary">
            目录存在，但未识别到安装包（支持 MySQL / Redis / VerneMQ / JDK / MinIO / GreptimeDB 的 msi/exe/zip）
          </Typography>
        ) : (
          <>
            {scan.misplaced.length > 0 && (
              <Alert severity="warning" icon={false} sx={{ mb: 1.5 }}>
                {scan.misplaced.map((m) => (
                  <Typography key={m.path} variant="body2" component="div">
                    「{m.fileName}」放在了 {m.foundIn}/ 目录，但它属于 {m.fileTarget.toUpperCase()} ——建议移到对应的约定子目录（按软件名命名，如 MySQL、JDK）
                  </Typography>
                ))}
              </Alert>
            )}
            <Table size="small" aria-label="资源目录扫描结果">
              <TableBody>
                {scan.files.map((f) => (
                  <TableRow key={f.path}>
                    <TableCell sx={{ fontFamily: "monospace", fontSize: 12, py: 0.5 }}>{f.fileName}</TableCell>
                    <TableCell align="right" sx={{ py: 0.5, whiteSpace: "nowrap" }}>
                      {f.target ? `识别为 ${f.target}` : "未识别"}
                      {f.inConventionDir && (
                        <Typography component="span" variant="caption" sx={{ ml: 1, color: "success.main" }}>
                          约定目录
                        </Typography>
                      )}
                    </TableCell>
                    <TableCell align="right" sx={{ py: 0.5, whiteSpace: "nowrap", color: "text.secondary" }}>
                      {mb(f.sizeBytes)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </>
        )}
      </Paper>
    );

  return (
    <>
      <SettingsLayout
        navTitle="设置"
        sections={[
          {
            key: "general",
            icon: <Tune />,
            label: "通用",
            title: "通用",
            description: "实时刷新行为。保存后立即生效，无需重启。",
            content: (
              <>
                <Paper variant="outlined" sx={{ overflow: "hidden" }}>
                  <SettingRow
                    label="服务状态刷新间隔"
                    description="自动轮询各服务运行状态的频率；关闭 = 启动时检测一次，此后用卡片上的「刷新此服务」按钮按需刷新"
                  >
                    <Select
                      size="small"
                      value={draft.statusRefreshSecs}
                      onChange={(e) => setDraft((d) => ({ ...d, statusRefreshSecs: Number(e.target.value) }))}
                      sx={{ width: 130 }}
                    >
                      {STATUS_OPTIONS.map((v) => (
                        <MenuItem key={v} value={v}>
                          {intervalLabel(v)}
                        </MenuItem>
                      ))}
                    </Select>
                  </SettingRow>
                  <Divider />
                  <SettingRow
                    label="环境复探间隔"
                    description="重新探测运行环境依赖的频率（探测会执行命令，不宜过快）；关闭 = 仅进入页面时检测"
                  >
                    <Select
                      size="small"
                      value={draft.envRefreshSecs}
                      onChange={(e) => setDraft((d) => ({ ...d, envRefreshSecs: Number(e.target.value) }))}
                      sx={{ width: 130 }}
                    >
                      {ENV_OPTIONS.map((v) => (
                        <MenuItem key={v} value={v}>
                          {intervalLabel(v)}
                        </MenuItem>
                      ))}
                    </Select>
                  </SettingRow>
                </Paper>
                {dirty && (
                  <Stack direction="row" spacing={1} sx={{ mt: 1.5 }}>
                    <Button variant="contained" onClick={save} disabled={saving}>
                      保存
                    </Button>
                    <Button onClick={() => setDraft(settings)} disabled={saving}>
                      放弃
                    </Button>
                  </Stack>
                )}
              </>
            ),
          },
          {
            key: "resources",
            icon: <FolderOpen />,
            label: "资源目录",
            title: "本地资源目录",
            description: "环境修复的离线安装包来源。无外网的交付机上，把 MySQL / Redis / JDK / VerneMQ / MinIO 等安装包放到这个目录，「环境修复」会优先扫描此目录自动安装。",
            content: (
              <>
                <Paper variant="outlined" sx={{ overflow: "hidden" }}>
                  <SettingRow label="资源目录路径" description="支持根目录与子目录分层（最多两级），按文件名自动识别归属软件">
                    <Stack direction="row" spacing={1} alignItems="center">
                      <TextField
                        size="small"
                        placeholder="例如 D:\dfy-resources"
                        value={draft.resourceDir}
                        onChange={(e) => setDraft((d) => ({ ...d, resourceDir: e.target.value }))}
                        sx={{ width: 280 }}
                      />
                      <Button variant="outlined" onClick={pickDir}>
                        浏览…
                      </Button>
                    </Stack>
                  </SettingRow>
                </Paper>
                <Stack direction="row" spacing={1} sx={{ mt: 1.5 }}>
                  <Button
                    variant="contained"
                    onClick={save}
                    disabled={saving || !dirty}
                  >
                    保存
                  </Button>
                  <Button onClick={doScan} disabled={scanning || !draft.resourceDir.trim()}>
                    {scanning ? "扫描中…" : "扫描此目录"}
                  </Button>
                  {dirty && (
                    <Button onClick={() => setDraft(settings)} disabled={saving}>
                      放弃
                    </Button>
                  )}
                </Stack>
                {resourceScanBody}
              </>
            ),
          },
          {
            key: "data",
            icon: <Storage />,
            label: "数据",
            title: "数据",
            description: "本机记录的运维数据管理。",
            content: (
              <Paper variant="outlined" sx={{ overflow: "hidden" }}>
                <SettingRow
                  label="清除「已知悉」记录"
                  description="运维中心里逐项确认的跟踪记录（保存在本机浏览器存储中）"
                >
                  <Button size="small" variant="outlined" color="warning" onClick={clearAck}>
                    清除
                  </Button>
                </SettingRow>
              </Paper>
            ),
          },
          {
            key: "about",
            icon: <Info />,
            label: "关于",
            title: "关于",
            description: "平台信息与能力边界。",
            content: (
              <Paper variant="outlined" sx={{ overflow: "hidden" }}>
                <SettingRow label="版本" description="DFY 运维平台">
                  <Typography variant="body2" sx={{ fontFamily: "monospace" }}>
                    0.1.0
                  </Typography>
                </SettingRow>
                <Divider />
                <SettingRow label="运维 API 地址" description="内嵌 Axum 后端，仅监听本机回环地址，外部不可达">
                  <Typography variant="body2" sx={{ fontFamily: "monospace" }}>
                    127.0.0.1:18721
                  </Typography>
                </SettingRow>
                <Divider />
                <SettingRow
                  label="能力边界"
                  description="环境检测仅探测与提醒；环境修复按「本地资源目录离线安装 → 在线下载（仅 Java）→ AI 方案」分级执行，安装类操作需你在 UAC 弹窗中确认，AI 生成的命令仅白名单内且经你逐条确认后才代执行"
                >
                  <Alert severity="info" icon={false} sx={{ py: 0.25, px: 1.5 }}>
                    探测提醒 · 分级修复
                  </Alert>
                </SettingRow>
              </Paper>
            ),
          },
        ]}
      />

      <Snackbar
        open={snack.open}
        autoHideDuration={3000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity={snack.severity} variant="filled" onClose={() => setSnack((s) => ({ ...s, open: false }))}>
          {snack.msg}
        </Alert>
      </Snackbar>
    </>
  );
}
