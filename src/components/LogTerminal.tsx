import type { Ref } from "react";
import { Box, Typography } from "@mui/material";
import { MONO_STACK } from "../theme";

/**
 * 全站统一彩色日志终端（2026-11-04 用户反馈「日志还是黑底白字」——着色此前
 * 只接了监控中心，服务详情/构建日志/修复中心都是黑白 pre 原始输出）。
 *
 * 着色三层：
 * 1. 行首时间戳淡化（扫时间列不干扰读正文）
 * 2. 行内关键字按严重度着色：ERROR/异常/失败 红粗、WARN/警告/重试 黄、
 *    成功/完成/Started 绿（用户规则：全站无绿色系语义色，成功绿仅日志
 *    终端内使用——这是日志内容的自然语义，非界面状态色）
 * 3. 错误行整行淡红底（第二层信号，运维一眼锁定出错段落）
 */

/** 日志行时间前缀：Spring/nginx 外大多为 `yyyy-MM-dd HH:mm:ss(.SSS)`；
 *  平台任务日志行是 `[yyyy-MM-dd HH:mm:ss.SSS] msg`。按时间搜索定位的基础。 */
export const LOG_TS_RE = /^\[?(\d{4}-\d{2}-\d{2})[ T](\d{2}:\d{2}:\d{2}(?:[.,]\d+)?)\]?\s*/;

/** 行内关键字 token 正则（按严重度分三组，先匹配者优先）。
 *  运维扫一眼就知道哪条是错、哪条是警示、哪条是成功——不用逐行读。 */
const TOKEN_RE =
  /(ERROR|FATAL|Exception|Caused by|FAILED|Failed|失败|错误|异常|拒绝|超时|无法|不能|Access denied|refused|Cannot find|No such|退出|宕)|(WARN(?:ING)?|警告|重试|retry|Retry|跳过|忽略|retrying)|(成功|完成|Started|started in|已启动|启动成功|OK|正常)/g;

type Tok = { t: string; k?: "e" | "w" | "s" };

/** 把一行拆成着色片段：时间前缀单独淡化，正文按关键字标色。 */
export function tokenizeLine(line: string): { ts: string | null; parts: Tok[] } {
  const m = LOG_TS_RE.exec(line);
  if (!m) return { ts: null, parts: [{ t: line }] };
  const rest = line.slice(m[0].length);
  const parts: Tok[] = [];
  let last = 0;
  TOKEN_RE.lastIndex = 0;
  let mm: RegExpExecArray | null;
  while ((mm = TOKEN_RE.exec(rest))) {
    if (mm.index > last) parts.push({ t: rest.slice(last, mm.index) });
    parts.push({ t: mm[0], k: mm[1] ? "e" : mm[2] ? "w" : "s" });
    last = mm.index + mm[0].length;
    if (mm[0].length === 0) TOKEN_RE.lastIndex++; // 防空匹配死循环
  }
  if (last < rest.length) parts.push({ t: rest.slice(last) });
  return { ts: m[0].replace(/\]?\s*$/, ""), parts };
}

/** 单行着色渲染。容器（深底/滚动/高度）由调用方自定，这里只负责一行。 */
export function ColoredLogLine({
  line,
  onClick,
  highlight,
  errorRowBg,
  dimmed,
  noWrap,
  innerRef,
}: {
  line: string;
  onClick?: () => void;
  /** 定位高亮（时间搜索跳转行）：整行品牌蓝底 */
  highlight?: boolean;
  /** 错误行整行淡红底；缺省自动判定（行内含 ERROR 级关键字） */
  errorRowBg?: boolean;
  /** 旧行淡化（autoScroll 终端里非最新行） */
  dimmed?: boolean;
  /** true = 长行不换行横向滚动（时间列对齐）；默认 pre-wrap 折行 */
  noWrap?: boolean;
  innerRef?: Ref<HTMLDivElement>;
}) {
  const { ts, parts } = tokenizeLine(line);
  const hasErr = errorRowBg ?? parts.some((p) => p.k === "e");
  return (
    <Typography
      ref={innerRef}
      variant="caption"
      component="div"
      onClick={onClick}
      sx={{
        fontFamily: MONO_STACK,
        fontSize: 12,
        whiteSpace: noWrap ? "pre" : "pre-wrap",
        wordBreak: noWrap ? undefined : "break-all",
        lineHeight: 1.7,
        cursor: onClick ? "pointer" : undefined,
        px: 0.5,
        borderRadius: 0.5,
        opacity: dimmed ? 0.75 : 1,
        bgcolor: highlight
          ? "rgba(22,58,94,0.35)"
          : hasErr
            ? "rgba(211,47,47,0.10)"
            : "transparent",
        "&:hover": onClick && !highlight && !hasErr ? { bgcolor: "rgba(255,255,255,0.06)" } : undefined,
        color: "grey.300",
        "& .ts": { color: "grey.500", mr: 1 },
        "& .e": { color: "#ff8a80", fontWeight: 700 },
        "& .w": { color: "#ffd54f" },
        "& .s": { color: "#81c784" },
      }}
    >
      {ts && <span className="ts">{ts}</span>}
      {parts.map((p, j) =>
        p.k ? (
          <span key={j} className={p.k}>
            {p.t}
          </span>
        ) : (
          <span key={j}>{p.t}</span>
        ),
      )}
    </Typography>
  );
}

/** 共享深底日志容器：调用方往里塞 ColoredLogLine 行 */
export function LogTerminalBox({
  children,
  maxHeight,
  boxRef,
  sx,
}: {
  children: React.ReactNode;
  maxHeight: number | string;
  boxRef?: Ref<HTMLDivElement>;
  sx?: object;
}) {
  return (
    <Box
      ref={boxRef}
      sx={{
        overflow: "auto",
        bgcolor: "grey.900",
        borderRadius: 1,
        px: 1.5,
        py: 1,
        maxHeight,
        ...sx,
      }}
    >
      {children}
    </Box>
  );
}
