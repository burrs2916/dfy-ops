import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import VpnKeyIcon from "@mui/icons-material/VpnKey";
import { MONO_STACK } from "../../theme";
import { api } from "../../api";
import { btnSx } from "./shared";

// ===== 业务用户改密码（root 执行，无需旧密码） =====
export default function UserPasswordButton({
  user,
  host,
  onDone,
  onError,
}: {
  user: string;
  host: string;
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setPassword("");
      setConfirm("");
    }
  }, [open]);

  const submit = async () => {
    if (password !== confirm) {
      onError("两次输入的密码不一致");
      return;
    }
    setBusy(true);
    try {
      const r = await api.mysqlUserPassword(user, host, password);
      if (r.ok) {
        setOpen(false);
        onDone(r.message ?? "密码修改成功");
      } else onError(r.error ?? "修改失败");
    } catch (e) {
      onError(`修改失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Button size="small" startIcon={<VpnKeyIcon />} onClick={() => setOpen(true)} sx={btnSx}>
        改密码
      </Button>
      <Dialog open={open} onClose={() => setOpen(false)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontWeight: 700 }}>修改用户密码</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 0.5 }}>
            <Typography variant="body2" color="text.secondary">
              目标账号 <Typography component="span" sx={{ fontFamily: MONO_STACK, fontWeight: 600 }}>{user}@{host}</Typography>
              ，由 root 执行改密，无需旧密码。使用该账号的应用需同步更新连接密码。
            </Typography>
            <TextField
              label="新密码（至少 6 位）"
              type="password"
              size="small"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              fullWidth
            />
            <TextField
              label="确认新密码"
              type="password"
              size="small"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              fullWidth
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)} sx={btnSx}>
            取消
          </Button>
          <Button
            variant="contained"
            disabled={busy || password.length < 6 || !confirm}
            onClick={() => void submit()}
            sx={btnSx}
          >
            {busy ? "提交中…" : "确认修改"}
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
