import { useCallback, useEffect, useState } from "react";
import { MONO_STACK } from "../theme";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControl,
  IconButton,
  InputLabel,
  MenuItem,
  Paper,
  Select,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import SearchIcon from "@mui/icons-material/Search";
import RestartAltIcon from "@mui/icons-material/RestartAlt";
import ArrowUpwardIcon from "@mui/icons-material/ArrowUpward";
import ArrowDownwardIcon from "@mui/icons-material/ArrowDownward";
import CodeIcon from "@mui/icons-material/Code";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import FileDownloadIcon from "@mui/icons-material/FileDownload";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { MysqlDataPage, MysqlQueryResult } from "../types";

// 单页行数可选：默认 50（少而清爽），大表可自行切到 100/200；后端单页上限 200
const PAGE_SIZES = [50, 100, 200] as const;

export type TableView = "data" | "schema";

/**
 * 表数据（分页/排序/筛选）+ 表结构（列信息/DDL）双视图面板。
 * db/table 变化自动重置浏览状态并重新加载；删除类维护操作在父级统一确认。
 */
export default function MysqlTablePanel({
  db,
  table,
  view,
  onNotify,
}: {
  db: string;
  table: string;
  view: TableView;
  onNotify: (text: string, severity: "success" | "error") => void;
}) {
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState<number>(50);
  const [jump, setJump] = useState("");
  const [sort, setSort] = useState<{ col: string; dir: "asc" | "desc" } | null>(null);
  const [colInput, setColInput] = useState("");
  const [kwInput, setKwInput] = useState("");
  const [col, setCol] = useState("");
  const [kw, setKw] = useState("");
  const [filterOpen, setFilterOpen] = useState(false);
  const [data, setData] = useState<MysqlDataPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [schema, setSchema] = useState<MysqlQueryResult | null>(null);
  const [schemaKey, setSchemaKey] = useState<string | null>(null);
  const [schemaLoading, setSchemaLoading] = useState(false);
  const [ddl, setDdl] = useState<string | null>(null);
  // 点击行弹出的完整字段对照视图
  const [detail, setDetail] = useState<{ columns: string[]; row: string[] } | null>(null);
  const [exporting, setExporting] = useState(false);

  // 当前筛选条件全量导出 CSV（后端循环查询写文件，带 BOM）
  const exportCsv = async () => {
    const path = await save({
      defaultPath: `${table}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!path) return;
    setExporting(true);
    try {
      const r = await api.mysqlExportCsv({
        path,
        db,
        table,
        col: col || undefined,
        kw: kw || undefined,
      });
      if (r.ok) onNotify(r.message ?? "导出完成", "success");
      else onNotify(r.error ?? "导出失败", "error");
    } catch (e) {
      onNotify(`导出失败：${String(e)}`, "error");
    } finally {
      setExporting(false);
    }
  };

  const loadData = useCallback(
    async (
      p: number,
      s: { col: string; dir: "asc" | "desc" } | null,
      c: string,
      k: string,
      size?: number,
    ) => {
      setLoading(true);
      try {
        const r = await api.mysqlData({
          db,
          table,
          page: p,
          size: size ?? pageSize,
          order: s?.col,
          dir: s?.dir,
          col: c || undefined,
          kw: k || undefined,
        });
        if (r.ok) {
          setData(r);
        } else {
          setData(null);
          onNotify(r.error ?? "查询失败", "error");
        }
      } catch (e) {
        onNotify(`查询失败：${String(e)}`, "error");
      } finally {
        setLoading(false);
      }
    },
    [db, table, onNotify, pageSize],
  );

  // 换表：重置浏览状态 + 拉第一页
  useEffect(() => {
    setPage(1);
    setSort(null);
    setColInput("");
    setKwInput("");
    setCol("");
    setKw("");
    setDdl(null);
    setJump("");
    void loadData(1, null, "", "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [db, table]);

  // 换视图到「表结构」时拉结构（DDL 惰性加载）；同一张表已加载过则跳过，
  // 避免在「表数据/表结构」页签间来回切换时每次都起 mysql 进程重复查询
  useEffect(() => {
    if (view !== "schema") return;
    const key = `${db}.${table}`;
    if (schemaKey === key && schema) return;
    let cancelled = false;
    setSchemaLoading(true);
    api
      .mysqlColumns(db, table)
      .then((r) => {
        if (!cancelled) {
          setSchema(r);
          setSchemaKey(key);
        }
      })
      .catch((e) => {
        if (!cancelled) onNotify(`获取表结构失败：${String(e)}`, "error");
      })
      .finally(() => {
        if (!cancelled) setSchemaLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [view, db, table, schemaKey, schema, onNotify]);

  const loadDdl = useCallback(async () => {
    try {
      const r = await api.mysqlDdl(db, table);
      if (r.ok && r.ddl) setDdl(r.ddl);
      else onNotify(r.error ?? "获取建表语句失败", "error");
    } catch (e) {
      onNotify(`获取建表语句失败：${String(e)}`, "error");
    }
  }, [db, table, onNotify]);

  const toggleSort = (colName: string) => {
    const next: { col: string; dir: "asc" | "desc" } | null =
      sort?.col !== colName
        ? { col: colName, dir: "asc" }
        : sort.dir === "asc"
          ? { col: colName, dir: "desc" }
          : null;
    setSort(next);
    setPage(1);
    void loadData(1, next, col, kw);
  };

  const totalPages = Math.max(1, Math.ceil((data?.total ?? 0) / pageSize));

  // 跳转指定页（越界自动收敛）
  const gotoPage = () => {
    const n = parseInt(jump, 10);
    if (!Number.isFinite(n) || n === 0) return;
    const p = Math.min(Math.max(n, 1), totalPages);
    setPage(p);
    setJump("");
    void loadData(p, sort, col, kw);
  };

  // 每页行数切换：回到第 1 页重新拉取
  const changePageSize = (n: number) => {
    setPageSize(n);
    setPage(1);
    setJump("");
    void loadData(1, sort, col, kw, n);
  };

  if (view === "schema") {
    return (
    <Stack spacing={1.5} sx={{ mt: 1.5, flex: 1, minHeight: 0 }}>
        <Stack direction="row" spacing={1} alignItems="center">
          <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
            {table} · 表结构
          </Typography>
          <Box sx={{ flex: 1 }} />
          <Button
            size="small"
            startIcon={ddl ? <ContentCopyIcon /> : <CodeIcon />}
            onClick={() => {
              if (ddl) void navigator.clipboard.writeText(ddl);
              else void loadDdl();
            }}
            sx={{ textTransform: "none" }}
          >
            {ddl ? "复制建表语句" : "查看建表语句"}
          </Button>
        </Stack>
        {ddl && (
          <Paper variant="outlined" sx={{ p: 1.5, bgcolor: "action.hover", maxHeight: 220, overflow: "auto" }}>
            <Typography component="pre" sx={{ fontFamily: MONO_STACK, fontSize: 12, whiteSpace: "pre-wrap", m: 0 }}>
              {ddl}
            </Typography>
          </Paper>
        )}
        {schemaLoading ? (
          <Typography variant="body2" color="text.secondary">
            加载中…
          </Typography>
        ) : schema?.ok ? (
          <TableContainer sx={{ flex: 1, minHeight: 0, border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
            <Table size="small" stickyHeader>
              <TableHead>
                <TableRow>
                  {(schema.columns ?? []).map((c, i) => (
                    <TableCell key={i} sx={{ fontWeight: 700 }}>
                      {c}
                    </TableCell>
                  ))}
                </TableRow>
              </TableHead>
              <TableBody>
                {(schema.rows ?? []).map((r, ri) => (
                  <TableRow key={ri} hover>
                    {r.map((cell, ci) => (
                      <TableCell key={ci} sx={{ fontFamily: ci <= 1 ? MONO_STACK : undefined }}>
                        {cell}
                      </TableCell>
                    ))}
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableContainer>
        ) : (
          <Alert severity="error">{schema?.error ?? "获取表结构失败"}</Alert>
        )}
      </Stack>
    );
  }

  return (
    <Stack spacing={1} sx={{ mt: 1.5, flex: 1, minHeight: 0 }}>
      {/* 数据为主角：筛选默认收起，激活时在标题行显示条件徽标（可一键清除） */}
      <Stack direction="row" spacing={1} alignItems="center">
        {kw && (
          <Chip
            size="small"
            color="primary"
            variant="outlined"
            label={`筛选：${col || "全部列"} ~ ${kw}`}
            onDelete={() => {
              setColInput("");
              setKwInput("");
              setCol("");
              setKw("");
              setPage(1);
              void loadData(1, sort, "", "");
            }}
          />
        )}
        <Box sx={{ flex: 1 }} />
        {loading && <CircularProgress size={16} />}
        <Button
          size="small"
          startIcon={<SearchIcon />}
          endIcon={filterOpen ? <ExpandLessIcon /> : <ExpandMoreIcon />}
          onClick={() => setFilterOpen((o) => !o)}
          sx={{ textTransform: "none", whiteSpace: "nowrap" }}
        >
          筛选
        </Button>
      </Stack>

      {filterOpen && (
        <Paper variant="outlined" sx={{ p: 1.25 }}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" rowGap={1}>
            <FormControl size="small" sx={{ minWidth: 140 }}>
              <InputLabel>筛选列</InputLabel>
              <Select value={colInput} label="筛选列" onChange={(e) => setColInput(e.target.value)}>
                <MenuItem value="">
                  <em>全部列</em>
                </MenuItem>
                {(data?.columns ?? []).map((c) => (
                  <MenuItem key={c} value={c}>
                    {c}
                  </MenuItem>
                ))}
              </Select>
            </FormControl>
            <TextField
              size="small"
              placeholder="模糊搜索关键字"
              value={kwInput}
              onChange={(e) => setKwInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  setCol(colInput);
                  setKw(kwInput);
                  setPage(1);
                  void loadData(1, sort, colInput, kwInput);
                }
              }}
              sx={{ width: 220 }}
            />
            <Button
              size="small"
              variant="contained"
              startIcon={<SearchIcon />}
              onClick={() => {
                setCol(colInput);
                setKw(kwInput);
                setPage(1);
                void loadData(1, sort, colInput, kwInput);
              }}
              sx={{ textTransform: "none", whiteSpace: "nowrap" }}
            >
              应用筛选
            </Button>
            <Tooltip title="清空筛选条件">
              <span>
                <IconButton
                  size="small"
                  disabled={!col && !kw}
                  onClick={() => {
                    setColInput("");
                    setKwInput("");
                    setCol("");
                    setKw("");
                    setPage(1);
                    void loadData(1, sort, "", "");
                  }}
                >
                  <RestartAltIcon fontSize="small" />
                </IconButton>
              </span>
            </Tooltip>
          </Stack>
        </Paper>
      )}

      {data?.ok ? (
        <>
          {/* 表格填满页签内的剩余布局区域：表头 sticky 常驻，区域内滚动（对齐 Navicat 常规布局） */}
          <TableContainer sx={{ flex: 1, minHeight: 0, border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
            <Table size="small" stickyHeader>
              <TableHead>
                <TableRow>
                  {(data.columns ?? []).map((c) => (
                    <TableCell
                      key={c}
                      sortDirection={sort?.col === c ? (sort.dir === "asc" ? "asc" : "desc") : false}
                      sx={{ fontWeight: 700, fontFamily: MONO_STACK, whiteSpace: "nowrap" }}
                    >
                      <Stack direction="row" spacing={0.5} alignItems="center" sx={{ cursor: "pointer" }} onClick={() => toggleSort(c)}>
                        {c}
                        {sort?.col === c &&
                          (sort.dir === "asc" ? <ArrowUpwardIcon sx={{ fontSize: 13 }} /> : <ArrowDownwardIcon sx={{ fontSize: 13 }} />)}
                      </Stack>
                    </TableCell>
                  ))}
                </TableRow>
              </TableHead>
              <TableBody>
                {(data.rows ?? []).map((r, ri) => (
                  <TableRow
                    key={ri}
                    hover
                    sx={{ cursor: "pointer" }}
                    onClick={() => setDetail({ columns: data.columns ?? [], row: r })}
                  >
                    {r.map((cell, ci) => (
                      <TableCell
                        key={ci}
                        sx={{
                          fontFamily: MONO_STACK,
                          color: cell === "NULL" ? "text.disabled" : undefined,
                          whiteSpace: "nowrap",
                          maxWidth: 320,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {/* 超宽被省略的长值悬浮可看全文（NULL 短值无需提示） */}
                        {cell !== "NULL" && cell.length > 40 ? (
                          <Tooltip title={cell} placement="top-start" arrow>
                            <span>{cell}</span>
                          </Tooltip>
                        ) : (
                          cell
                        )}
                      </TableCell>
                    ))}
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableContainer>
          {/* 分页脚 */}
          <Stack direction="row" spacing={1} alignItems="center">
            <Typography variant="caption" color="text.secondary">
              共 {data.total ?? 0} 行 · 第 {data.page ?? page} / {totalPages} 页 · 点击行查看详情
            </Typography>
            <Box sx={{ flex: 1 }} />
            <Select
              size="small"
              value={pageSize}
              onChange={(e) => changePageSize(Number(e.target.value))}
              sx={{ height: 32, fontSize: 13 }}
            >
              {PAGE_SIZES.map((n) => (
                <MenuItem key={n} value={n}>
                  {n} 行/页
                </MenuItem>
              ))}
            </Select>
            <Button
              size="small"
              startIcon={<FileDownloadIcon />}
              disabled={exporting}
              onClick={exportCsv}
              sx={{ textTransform: "none" }}
            >
              {exporting ? "导出中…" : "导出 CSV"}
            </Button>
            <Button
              size="small"
              disabled={(data.page ?? page) <= 1 || loading}
              onClick={() => {
                const p = (data.page ?? page) - 1;
                setPage(p);
                void loadData(p, sort, col, kw);
              }}
              sx={{ textTransform: "none" }}
            >
              上一页
            </Button>
            <Button
              size="small"
              disabled={(data.page ?? page) >= totalPages || loading}
              onClick={() => {
                const p = (data.page ?? page) + 1;
                setPage(p);
                void loadData(p, sort, col, kw);
              }}
              sx={{ textTransform: "none" }}
            >
              下一页
            </Button>
            <TextField
              size="small"
              placeholder="页码"
              sx={{ width: 76 }}
              inputProps={{ inputMode: "numeric", style: { textAlign: "center", fontSize: 13 } }}
              value={jump}
              onChange={(e) => setJump(e.target.value.replace(/\D/g, ""))}
              onKeyDown={(e) => {
                if (e.key === "Enter") gotoPage();
              }}
            />
            <Button size="small" disabled={loading} onClick={gotoPage} sx={{ textTransform: "none" }}>
              跳转
            </Button>
          </Stack>
        </>
      ) : (
        <Alert severity="info">{loading ? "查询中…" : "暂无数据（或筛选无结果）"}</Alert>
      )}

      {/* 行详情：字段名 + 值逐行对照，长值完整换行显示 */}
      <Dialog open={detail !== null} onClose={() => setDetail(null)} maxWidth="sm" fullWidth>
        <DialogTitle sx={{ fontWeight: 700, fontSize: 15 }}>
          行详情 · {db}.{table}
        </DialogTitle>
        <DialogContent dividers sx={{ p: 0 }}>
          <Table size="small">
            <TableBody>
              {(detail?.columns ?? []).map((c, i) => (
                <TableRow key={c}>
                  <TableCell
                    sx={{ width: 170, fontWeight: 700, fontFamily: MONO_STACK, verticalAlign: "top" }}
                  >
                    {c}
                  </TableCell>
                  <TableCell
                    sx={{
                      fontFamily: MONO_STACK,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-all",
                      color: detail?.row[i] === "NULL" ? "text.disabled" : undefined,
                    }}
                  >
                    {detail?.row[i]}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </DialogContent>
        <DialogActions>
          <Button
            size="small"
            startIcon={<ContentCopyIcon />}
            onClick={() => {
              const text = (detail?.columns ?? [])
                .map((c, i) => `${c}\t${detail?.row[i] ?? ""}`)
                .join("\n");
              void navigator.clipboard?.writeText(text).catch(() => {});
            }}
            sx={{ textTransform: "none" }}
          >
            复制该行
          </Button>
          <Button size="small" onClick={() => setDetail(null)} sx={{ textTransform: "none" }}>
            关闭
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}
