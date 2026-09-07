import { useEffect, useState } from "react";
import { Alert, Button, Divider, MenuItem, Paper, Select, Snackbar, Stack, Switch, Typography } from "@mui/material";
import { Tune, Storage, Info } from "@mui/icons-material";
import { api } from "../api";
import { useAppSettingsStore } from "../store";
import { SettingsLayout } from "../components/SettingsLayout";
import { SettingRow } from "../components/SettingRow";
import type { AppSettings } from "../types";

/**
 * 设置：应用自身配置（通用 / 数据 / 关于）。
 * 与「AI助手」严格分离——LLM 配置在 AI 助手页，此处不涉及。
 * 布局与其他设置类页面共用 SettingsLayout（web-craft 风格）。
 */

const STATUS_OPTIONS = [3, 5, 10];
const ENV_OPTIONS = [10, 15, 30, 60];

export default function Settings() {
  const { settings, loaded, apply } = useAppSettingsStore();
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saving, setSaving] = useState(false);
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
        apply({
          statusRefreshSecs: draft.statusRefreshSecs,
          envRefreshSecs: draft.envRefreshSecs,
          desktopInReadiness: draft.desktopInReadiness,
        });
        setSnack({ open: true, msg: "已保存，立即生效", severity: "success" });
      } else {
        setSnack({ open: true, msg: "保存失败：" + (r.error ?? "未知错误"), severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: "保存失败：" + (e instanceof Error ? e.message : String(e)), severity: "error" });
    } finally {
      setSaving(false);
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
    draft.desktopInReadiness !== settings.desktopInReadiness;

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
            description: "实时刷新行为与就绪度统计口径。保存后立即生效，无需重启。",
            content: (
              <>
                <Paper variant="outlined" sx={{ overflow: "hidden" }}>
                  <SettingRow
                    label="服务状态刷新间隔"
                    description="「服务总览」页轮询各服务运行状态的频率"
                  >
                    <Select
                      size="small"
                      value={draft.statusRefreshSecs}
                      onChange={(e) => setDraft((d) => ({ ...d, statusRefreshSecs: Number(e.target.value) }))}
                      sx={{ width: 130 }}
                    >
                      {STATUS_OPTIONS.map((v) => (
                        <MenuItem key={v} value={v}>
                          {v} 秒
                        </MenuItem>
                      ))}
                    </Select>
                  </SettingRow>
                  <Divider />
                  <SettingRow
                    label="环境复探间隔"
                    description="「运维中心」页重新探测运行环境依赖的频率（探测会执行命令，不宜过快）"
                  >
                    <Select
                      size="small"
                      value={draft.envRefreshSecs}
                      onChange={(e) => setDraft((d) => ({ ...d, envRefreshSecs: Number(e.target.value) }))}
                      sx={{ width: 130 }}
                    >
                      {ENV_OPTIONS.map((v) => (
                        <MenuItem key={v} value={v}>
                          {v} 秒
                        </MenuItem>
                      ))}
                    </Select>
                  </SettingRow>
                  <Divider />
                  <SettingRow
                    label="桌面客户端计入就绪度"
                    description="桌面客户端（isDaemon=false）按 GUI 语义按需打开，默认不占「部署就绪度」分母。打开后，未运行也会拉低分数；关闭时仍会在就绪度卡片里常驻显示在线率"
                  >
                    <Switch
                      checked={draft.desktopInReadiness}
                      onChange={(e) => setDraft((d) => ({ ...d, desktopInReadiness: e.target.checked }))}
                    />
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
                  description="本平台仅做运行环境探测与提醒，不自动安装依赖、不自动部署、不修改系统配置；修复由你在运维中心按建议自助完成"
                >
                  <Alert severity="info" icon={false} sx={{ py: 0.25, px: 1.5 }}>
                    仅探测 · 提醒
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
