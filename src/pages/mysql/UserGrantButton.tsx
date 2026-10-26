import { useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControl,
  InputLabel,
  MenuItem,
  Select,
  Stack,
  Typography,
} from "@mui/material";
import AdminPanelSettingsIcon from "@mui/icons-material/AdminPanelSettings";
import { MONO_STACK } from "../../theme";
import { api } from "../../api";
import { btnSx } from "./shared";

// ===== 调整已有用户权限：撤销现有授权后按档位重授，密码不变 =====
export default function UserGrantButton({
  user,
  host,
  databases,
  onDone,
  onError,
}: {
  user: string;
  host: string;
  databases: string[];
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [privilege, setPrivilege] = useState<"ro" | "rw" | "all">("rw");
  const [database, setDatabase] = useState("*");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    try {
      const r = await api.mysqlUserGrants(user, host, privilege, database);
      if (r.ok) {
        setOpen(false);
        onDone(r.message ?? "权限已调整");
      } else onError(r.error ?? "调整失败");
    } catch (e) {
      onError(`调整失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Button size="small" startIcon={<AdminPanelSettingsIcon />} onClick={() => setOpen(true)} sx={btnSx}>
        调整权限
      </Button>
      <Dialog open={open} onClose={() => setOpen(false)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontWeight: 700 }}>调整权限</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 0.5 }}>
            <Typography variant="body2" color="text.secondary">
              目标账号 <Typography component="span" sx={{ fontFamily: MONO_STACK, fontWeight: 600 }}>{user}@{host}</Typography>
              。先撤销现有全部授权，再按下方选择重新授权，密码保持不变。
            </Typography>
            <FormControl size="small" fullWidth>
              <InputLabel>权限档位</InputLabel>
              <Select
                value={privilege}
                label="权限档位"
                onChange={(e) => setPrivilege(e.target.value as typeof privilege)}
              >
                <MenuItem value="ro">只读（SELECT）</MenuItem>
                <MenuItem value="rw">读写（增删改查）</MenuItem>
                <MenuItem value="all">全部权限</MenuItem>
              </Select>
            </FormControl>
            <FormControl size="small" fullWidth>
              <InputLabel>授权范围</InputLabel>
              <Select
                value={database}
                label="授权范围"
                onChange={(e) => setDatabase(e.target.value)}
              >
                <MenuItem value="*">全部数据库</MenuItem>
                {databases.map((d) => (
                  <MenuItem key={d} value={d}>
                    {d}
                  </MenuItem>
                ))}
              </Select>
            </FormControl>
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)} disabled={busy} sx={btnSx}>
            取消
          </Button>
          <Button variant="contained" disabled={busy} onClick={() => void submit()} sx={btnSx}>
            {busy ? "调整中…" : "确认调整"}
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
