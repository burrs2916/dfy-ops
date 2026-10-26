import { useEffect, useState } from "react";
import {
  Button,
  Stack,
  TableCell,
  TableRow,
  TextField,
  Tooltip,
} from "@mui/material";
import { MONO_STACK } from "../../theme";
import { api } from "../../api";
import type { MysqlVariableItem } from "../../types";
import { btnSx } from "./shared";

// ===== 服务器参数行：单参数在线调整（SET GLOBAL） =====
export default function VariableRow({
  item,
  onSaved,
  onError,
}: {
  item: MysqlVariableItem;
  onSaved: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(item.value);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!editing) setValue(item.value);
  }, [item.value, editing]);

  const cancel = () => {
    setValue(item.value);
    setEditing(false);
  };

  const save = async () => {
    const v = value.trim();
    if (!v || v === item.value) {
      cancel();
      return;
    }
    setBusy(true);
    try {
      const r = await api.mysqlSetVariable(item.name, v);
      if (r.ok) {
        setEditing(false);
        onSaved(r.message ?? `${item.name} 已调整`);
      } else onError(r.error ?? "调整失败");
    } catch (e) {
      onError(`调整失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <TableRow hover>
      <TableCell sx={{ fontFamily: MONO_STACK, fontWeight: 600, whiteSpace: "nowrap" }}>{item.name}</TableCell>
      <TableCell sx={{ fontFamily: MONO_STACK, maxWidth: 280, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        <Tooltip title={item.value} disableHoverListener={editing}>
          <span>{item.value}</span>
        </Tooltip>
      </TableCell>
      <TableCell>
        {editing ? (
          <Stack direction="row" spacing={1} alignItems="center">
            <TextField
              size="small"
              autoFocus
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void save();
                if (e.key === "Escape") cancel();
              }}
              sx={{ width: 220 }}
            />
            <Button size="small" variant="contained" disabled={busy} onClick={() => void save()} sx={btnSx}>
              {busy ? "保存中…" : "保存"}
            </Button>
            <Button size="small" disabled={busy} onClick={cancel} sx={btnSx}>
              取消
            </Button>
          </Stack>
        ) : (
          <Button size="small" onClick={() => setEditing(true)} sx={btnSx}>
            调整
          </Button>
        )}
      </TableCell>
    </TableRow>
  );
}
