import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
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
import { api } from "../api";
import type { MysqlDataPage, MysqlQueryResult } from "../types";

// 与后端 data 接口单页上限一致：一次拉满 200 行，观感接近 Navicat 打开表
const PAGE_SIZE = 200;

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

  const loadData = useCallback(
    async (p: number, s: { col: string; dir: "asc" | "desc" } | null, c: string, k: string) => {
      setLoading(true);
      try {
        const r = await api.mysqlData({
          db,
          table,
          page: p,
          size: PAGE_SIZE,
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
    [db, table, onNotify],
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

  const totalPages = Math.max(1, Math.ceil((data?.total ?? 0) / PAGE_SIZE));

  if (view === "schema") {
    return (
      <Stack spacing={1.5} sx={{ mt: 1.5 }}>
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
            <Typography component="pre" sx={{ fontFamily: "monospace", fontSize: 12, whiteSpace: "pre-wrap", m: 0 }}>
              {ddl}
            </Typography>
          </Paper>
        )}
        {schemaLoading ? (
          <Typography variant="body2" color="text.secondary">
            加载中…
          </Typography>
        ) : schema?.ok ? (
          <TableContainer sx={{ maxHeight: 620, border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
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
                      <TableCell key={ci} sx={{ fontFamily: ci <= 1 ? "monospace" : undefined }}>
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
    <Stack spacing={1} sx={{ mt: 1.5 }}>
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
          <TableContainer sx={{ maxHeight: 620, border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
            <Table size="small" stickyHeader>
              <TableHead>
                <TableRow>
                  {(data.columns ?? []).map((c) => (
                    <TableCell
                      key={c}
                      sortDirection={sort?.col === c ? (sort.dir === "asc" ? "asc" : "desc") : false}
                      sx={{ fontWeight: 700, fontFamily: "monospace", whiteSpace: "nowrap" }}
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
                  <TableRow key={ri} hover>
                    {r.map((cell, ci) => (
                      <TableCell
                        key={ci}
                        sx={{
                          fontFamily: "monospace",
                          color: cell === "NULL" ? "text.disabled" : undefined,
                          whiteSpace: "nowrap",
                          maxWidth: 320,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {cell}
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
              共 {data.total ?? 0} 行 · 第 {data.page ?? page} / {totalPages} 页 · 每页 {PAGE_SIZE} 行
            </Typography>
            <Box sx={{ flex: 1 }} />
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
          </Stack>
        </>
      ) : (
        <Alert severity="info">{loading ? "查询中…" : "暂无数据（或筛选无结果）"}</Alert>
      )}
    </Stack>
  );
}
