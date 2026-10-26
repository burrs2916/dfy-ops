import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  FormControl,
  InputLabel,
  MenuItem,
  Select,
  Stack,
  TextField,
  Typography,
  Box,
} from "@mui/material";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import NetworkCheckIcon from "@mui/icons-material/NetworkCheck";
import { api } from "../../api";
import type { MysqlOverview } from "../../types";
import { btnSx } from "./shared";

/** 强确认对话框：删除类操作必须显式输入「确认」之外的强提示文案确认 */
export function ConfirmDialog({
  target,
  onClose,
}: {
  target: { title: string; content: string; confirmText: string; action: () => Promise<void> } | null;
  onClose: () => void;
}) {
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => setTyped(""), [target]);
  return (
    <Dialog open={target !== null} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 700, display: "flex", alignItems: "center", gap: 1 }}>
        <WarningAmberIcon color="warning" />
        {target?.title}
      </DialogTitle>
      <DialogContent>
        <DialogContentText sx={{ whiteSpace: "pre-wrap" }}>{target?.content}</DialogContentText>
        <TextField
          size="small"
          fullWidth
          sx={{ mt: 2 }}
          label={`输入 ${target?.confirmText ?? ""} 以确认`}
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} sx={btnSx}>
          取消
        </Button>
        <Button
          variant="contained"
          color="error"
          disabled={busy || typed !== target?.confirmText}
          onClick={async () => {
            setBusy(true);
            try {
              await target!.action();
              onClose();
            } finally {
              setBusy(false);
            }
          }}
          sx={btnSx}
        >
          {busy ? "执行中…" : "确认执行"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// ===== 修改 root 密码 =====
export function PasswordDialog({
  open,
  onClose,
  hasPassword,
  onDone,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  hasPassword: boolean;
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setCurrent("");
      setNext("");
      setConfirm("");
    }
  }, [open]);

  const submit = async () => {
    if (next !== confirm) {
      onError("两次输入的新密码不一致");
      return;
    }
    setBusy(true);
    try {
      const r = await api.mysqlChangePassword(current, next);
      if (r.ok) onDone(r.message ?? "root 密码修改成功");
      else onError(r.error ?? "修改失败");
    } catch (e) {
      onError(`修改失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 700 }}>修改 root 密码</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ mt: 0.5 }}>
          {hasPassword ? (
            <TextField
              label="当前密码"
              type="password"
              size="small"
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
              fullWidth
            />
          ) : (
            <Alert severity="info">
              当前为初始空密码，「当前密码」留空即可。修改后平台自动托管新密码。
            </Alert>
          )}
          <TextField
            label="新密码（至少 6 位）"
            type="password"
            size="small"
            value={next}
            onChange={(e) => setNext(e.target.value)}
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
        <Button onClick={onClose} sx={btnSx}>
          取消
        </Button>
        <Button
          variant="contained"
          disabled={busy || !next || (hasPassword && !current)}
          onClick={() => void submit()}
          sx={btnSx}
        >
          {busy ? "提交中…" : "确认修改"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// ===== 新建用户与授权 =====
export function CreateUserDialog({
  open,
  onClose,
  databases,
  onDone,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  databases: string[];
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [user, setUser] = useState("");
  const [host, setHost] = useState("localhost");
  const [password, setPassword] = useState("");
  const [privilege, setPrivilege] = useState<"ro" | "rw" | "all">("rw");
  const [database, setDatabase] = useState("*");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setUser("");
      setHost("localhost");
      setPassword("");
      setPrivilege("rw");
      setDatabase("*");
    }
  }, [open]);

  const submit = async () => {
    setBusy(true);
    try {
      const r = await api.mysqlCreateUser({ user, host, password, privilege, database });
      if (r.ok) onDone(r.message ?? "用户创建成功");
      else onError(r.error ?? "创建失败");
    } catch (e) {
      onError(`创建失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const PRIV_LABEL: Record<typeof privilege, string> = {
    ro: "只读（SELECT）",
    rw: "读写（增删改查）",
    all: "全部权限",
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 700 }}>新建 MySQL 用户</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ mt: 0.5 }}>
          <TextField label="用户名" size="small" value={user} onChange={(e) => setUser(e.target.value)} fullWidth />
          <TextField
            label="允许连接的主机"
            size="small"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            helperText="localhost 仅本机；% 允许任意主机"
            fullWidth
          />
          <TextField label="密码" size="small" value={password} onChange={(e) => setPassword(e.target.value)} fullWidth />
          <FormControl size="small" fullWidth>
            <InputLabel>权限</InputLabel>
            <Select value={privilege} label="权限" onChange={(e) => setPrivilege(e.target.value as typeof privilege)}>
              {(Object.keys(PRIV_LABEL) as Array<typeof privilege>).map((k) => (
                <MenuItem key={k} value={k}>
                  {PRIV_LABEL[k]}
                </MenuItem>
              ))}
            </Select>
          </FormControl>
          <FormControl size="small" fullWidth>
            <InputLabel>授权范围</InputLabel>
            <Select value={database} label="授权范围" onChange={(e) => setDatabase(e.target.value)}>
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
        <Button onClick={onClose} sx={btnSx}>
          取消
        </Button>
        <Button variant="contained" disabled={busy || !user || !password} onClick={() => void submit()} sx={btnSx}>
          {busy ? "创建中…" : "创建用户"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// ===== 连接配置：平台托管凭据修正（平台外改过密码/端口后的自救入口） =====
export function CredentialDialog({
  open,
  onClose,
  ov,
  onDone,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  ov: MysqlOverview | null;
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [user, setUser] = useState("");
  const [port, setPort] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    if (open) {
      setUser(ov?.user ?? "");
      setPort(ov?.port != null ? String(ov.port) : "");
      setPassword("");
      setTestResult(null);
    }
    // ov 只在打开瞬间取预填值，打开期间不随刷新重置输入
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const parsePort = (): number | null => {
    const n = Number(port.trim());
    return Number.isInteger(n) && n >= 1 && n <= 65535 ? n : null;
  };

  // 用表单当前值实测连接（不写托管文件）：保存前先验证
  const runTest = async () => {
    if (!user.trim()) {
      setTestResult({ ok: false, text: "用户名不能为空" });
      return;
    }
    const portNum = parsePort();
    if (portNum === null) {
      setTestResult({ ok: false, text: "端口必须是 1-65535 的整数" });
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      const r = await api.mysqlTestConnection(user.trim(), password, portNum);
      setTestResult(
        r.ok
          ? { ok: true, text: `连接成功${r.version ? `，服务器版本 ${r.version}` : ""}` }
          : { ok: false, text: r.error ?? "连接失败" },
      );
    } catch (e) {
      setTestResult({ ok: false, text: `测试失败：${String(e)}` });
    } finally {
      setTesting(false);
    }
  };

  const submit = async () => {
    if (!user.trim()) {
      onError("用户名不能为空");
      return;
    }
    const portNum = parsePort();
    if (portNum === null) {
      onError("端口必须是 1-65535 的整数");
      return;
    }
    setBusy(true);
    try {
      const r = await api.mysqlUpdateCredential({
        user: user.trim(),
        port: portNum,
        ...(password ? { password } : {}),
      });
      if (r.ok) onDone(r.message ?? "连接配置已保存");
      else onError(r.error ?? "保存失败");
    } catch (e) {
      onError(`保存失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 700 }}>连接配置</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ mt: 0.5 }}>
          <Alert severity="info">
            这是平台连接 MySQL 使用的配置。若你在平台外修改过密码或端口导致平台连不上数据库，在这里改成实际值即可恢复。
          </Alert>
          <TextField
            label="用户名"
            size="small"
            value={user}
            onChange={(e) => setUser(e.target.value)}
            fullWidth
          />
          <TextField
            label="密码"
            type="password"
            size="small"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            helperText="留空表示不修改密码；测试连接按此处输入实测"
            fullWidth
          />
          <TextField
            label="端口"
            size="small"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            helperText="1-65535，默认 3306"
            fullWidth
          />
          {testResult && (
            <Alert severity={testResult.ok ? "success" : "error"} sx={{ whiteSpace: "pre-wrap", wordBreak: "break-all" }}>
              {testResult.text}
            </Alert>
          )}
          <Typography variant="caption" color="text.secondary">
            保存仅更新平台托管配置，不会修改 MySQL 本身；保存后点「刷新」验证连接。
          </Typography>
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button
          startIcon={<NetworkCheckIcon />}
          disabled={testing || busy}
          onClick={() => void runTest()}
          sx={btnSx}
        >
          {testing ? "测试中…" : "测试连接"}
        </Button>
        <Box sx={{ flex: 1 }} />
        <Button onClick={onClose} sx={btnSx}>
          取消
        </Button>
        <Button variant="contained" disabled={busy || testing} onClick={() => void submit()} sx={btnSx}>
          {busy ? "保存中…" : "保存"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
