import type { Ref } from "react";
import { Typography } from "@mui/material";
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

/** 日志行时间前缀，两种格式都收：
 *  · Spring/平台任务日志：`yyyy-MM-dd HH:mm:ss(.SSS)`（可带方括号）
 *  · nginx error.log：`yyyy/MM/dd HH:mm:ss`（斜杠，默认格式）
 *  捕获组保持原样，需要 Date 比较时由调用方把 `/` 归一成 `-`。 */
export const LOG_TS_RE = /^\[?(\d{4}[-/]\d{2}[-/]\d{2})[ T](\d{2}:\d{2}:\d{2}(?:[.,]\d+)?)\]?\s*/;

/** 把 `yyyy/MM/dd` 归一成 `yyyy-MM-dd`——`new Date()` 不认斜杠拼接的 ISO 串。 */
export const normalizeDate = (d: string) => d.replace(/\//g, "-");

/** 行内关键字 token 正则（按严重度分三组，先匹配者优先）。
 *  运维扫一眼就知道哪条是错、哪条是警示、哪条是成功——不用逐行读。
 *  覆盖四类常见输出：
 *  · logback/Spring（ERROR / Exception / Caused by）
 *  · Maven 构建（[ERROR] / BUILD FAILURE / BUILD SUCCESS）
 *  · nginx（[emerg] / [alert] / [crit] / 小写 "failed to"）
 *  · Shell/命令行（command not found / Permission denied）
 *  注意：不用正则 i 标志——"OK" 加 i 会命中 "token" 里的 "ok"，
 *  导致任意小写单词被误标绿。大小写形态逐项列，边界用 \b 收窄。 */
const TOKEN_RE =
  /(ERROR|error|FATAL|Exception|exception|Caused by|caused by|BUILD FAILURE|FAILURE|failed|Failed|FAILED|\[(?:emerg|alert|crit)\]|失败|错误|异常|拒绝|超时|无法|不能|Access denied|Permission denied|refused|command not found|Cannot find|No such|退出|宕)|(WARN|warn|Warning|warning|警告|重试|retry|Retry|跳过|忽略|retrying)|(成功|完成|Started|started in|BUILD SUCCESS|已启动|启动成功|\bOK\b|正常)/g;

type Tok = { t: string; k?: "e" | "w" | "s" };

/** 异常堆栈续行（无 ERROR 关键字，但属于错误块，整段连成淡红）：
 *  · Java：`\tat com.dfy.service.Foo.bar(Foo.java:42)`
 *  · Java 9+ 模块：`    at java.base/java.lang.Thread.run(...)`（含 `/`）
 *  · Python：`  File "app.py", line 42, in main` */
const STACK_RE = /^\s{0,8}(?:at\s+[\w$./]+\.[\w$<>]+\(|File\s+"[^"]+",\s+line\s+\d+)/;

/** 把一行拆成着色片段：时间前缀单独淡化，正文按关键字标色。
 *
 *  【必须无条件做关键字拆分】此前「无行首时间戳就整行原样返回」是致命缺陷：
 *  Maven 构建日志（`[INFO] ...`）、异常堆栈续行（`\tat com.xxx`）、
 *  命令输出续行都没有 `yyyy-MM-dd HH:mm:ss` 前缀——恰恰这些地方最该标红，
 *  结果全站最需要着色的行反而保持白色，用户看到的就是「黑底白字」。
 */
export function tokenizeLine(line: string): { ts: string | null; parts: Tok[] } {
  const m = LOG_TS_RE.exec(line);
  const rest = m ? line.slice(m[0].length) : line; // 无时间戳 → 整行作为正文
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
  return { ts: m ? m[0].replace(/\]?\s*$/, "") : null, parts };
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
  const hasErr = errorRowBg ?? (parts.some((p) => p.k === "e") || STACK_RE.test(line));
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

/** 全站日志终端统一底色。此前监控中心用 grey.900(#212121)、服务详情用
 *  #1e1e1e，两个深色肉眼可辨，切换页面会「跳」一下——统一为一处常量。 */
export const LOG_BG = "#1e1e1e";
