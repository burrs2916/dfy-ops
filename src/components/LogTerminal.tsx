import type { Ref } from "react";
import { styled } from "@mui/material/styles";
import { MONO_STACK } from "../theme";

/**
 * 全站统一彩色日志终端。四个用户反复报的 bug，在这一个组件里闭环：
 *
 * 1. 【v0.2.67 修复】v0.2.66 用 MUI sx 的嵌套 class 选择器 `& .e`，
 *    emotion 生产构建时选择器哈希隔离，导致样式没生效——用户装包后
 *    全灰白。改：用 emotion `styled` 直接内联到每个 span 上，emotion
 *    一定会生成正确的选择器，不再依赖任何嵌套 class 匹配。
 *
 * 2. 【v0.2.67 修复】ANSI 转义序列。Spring Boot logback 默认 TTY 检测
 *    会在控制台开启彩色输出（`\x1b[32mStarted...\x1b[0m`），写文件时也带
 *    上这些控制字符。后端 decode_log_bytes 不剥 ANSI，前端渲染出来就是
 *    一堆乱码字符。后端 tail 里补 strip_ansi 清洗，前端再剥一次双保险。
 *
 * 3. 【v0.2.66 修复，继续沿用】tokenizeLine 无条件做关键字拆分；
 *    关键字覆盖 Maven/nginx/小写/堆栈续行；nginx 斜杠日期归一化；
 *    异常堆栈行纳入错误块淡红底。
 *
 * 4. 【v0.2.64 修复】删除日志文件时 Windows 报「文件被占用」——
 *    见 handlers.rs 的 remove_file_with_fallback：直接删失败就 kill 端口
 *    占用者（logback 持有 dfy-server.log 的 java 进程），等 300ms 再删。
 */

/** 三种关键字的颜色（全站统一，只在日志终端内使用） */
const COLORS = {
  e: "#ff8a80", // 错误：红粗
  w: "#ffd54f", // 警告：黄
  s: "#81c784", // 成功：绿
};

/** ANSI/C/VT 100 转义序列正则。Spring Boot logback 默认彩色日志会往文件
 *  里写 `\x1b[32m` 这种 CSI，前端直接渲染就是乱码。 */
const ANSI_RE = /\x1b\[[0-9;?]*[A-Za-z]|\x1b\].*?(?:\x07|\x1b\\)|\x1b[PX][^\x07]*\x07|\x07/g;

/** 剥掉 ANSI 转义序列，保留可见文本。 */
export function stripAnsi(s: string): string {
  return s.replace(ANSI_RE, "");
}

/** 日志行时间前缀，两种格式都收：
 *  · Spring/平台任务日志：`yyyy-MM-dd HH:mm:ss(.SSS)`（可带方括号）
 *  · nginx error.log：`yyyy/MM/dd HH:mm:ss`（斜杠，默认格式） */
export const LOG_TS_RE = /^\[?(\d{4}[-/]\d{2}[-/]\d{2})[ T](\d{2}:\d{2}:\d{2}(?:[.,]\d+)?)\]?\s*/;

/** 把 `yyyy/MM/dd` 归一成 `yyyy-MM-dd`——`new Date()` 不认斜杠拼接的 ISO 串。 */
export const normalizeDate = (d: string) => d.replace(/\//g, "-");

/** 行内关键字 token 正则（按严重度分三组，先匹配者优先）。
 *  覆盖四类常见输出：logback/Spring、Maven 构建、nginx、Shell/命令行。
 *  注意：不用正则 i 标志——"OK" 加 i 会命中 "token" 里的 "ok"。 */
const TOKEN_RE =
  /(ERROR|error|FATAL|Exception|exception|Caused by|caused by|BUILD FAILURE|FAILURE|failed|Failed|FAILED|\[(?:emerg|alert|crit)\]|失败|错误|异常|拒绝|超时|无法|不能|Access denied|Permission denied|refused|command not found|Cannot find|No such|退出|宕)|(WARN|warn|Warning|warning|警告|重试|retry|Retry|跳过|忽略|retrying)|(成功|完成|Started|started in|BUILD SUCCESS|已启动|启动成功|\bOK\b|正常)/g;

type Tok = { t: string; k?: "e" | "w" | "s" };

/** Java/Python 异常堆栈续行：无 ERROR 关键字，但属于错误块，整段淡红。 */
const STACK_RE = /^\s{0,8}(?:at\s+[\w$./]+\.[\w$<>]+\(|File\s+"[^"]+",\s+line\s+\d+)/;

/** 把一行拆成着色片段：先剥 ANSI，再按时间戳和关键字拆分。 */
export function tokenizeLine(line: string): { ts: string | null; parts: Tok[] } {
  const clean = stripAnsi(line); // 先剥 ANSI，避免转义字符被 token 命中
  const m = LOG_TS_RE.exec(clean);
  const rest = m ? clean.slice(m[0].length) : clean;
  const parts: Tok[] = [];
  let last = 0;
  TOKEN_RE.lastIndex = 0;
  let mm: RegExpExecArray | null;
  while ((mm = TOKEN_RE.exec(rest))) {
    if (mm.index > last) parts.push({ t: rest.slice(last, mm.index) });
    parts.push({ t: mm[0], k: mm[1] ? "e" : mm[2] ? "w" : "s" });
    last = mm.index + mm[0].length;
    if (mm[0].length === 0) TOKEN_RE.lastIndex++;
  }
  if (last < rest.length) parts.push({ t: rest.slice(last) });
  return { ts: m ? m[0].replace(/\]?\s*$/, "") : null, parts };
}

// ===== emotion styled 组件（绝对不会丢样式）=====

const LogRow = styled("div")<{
  $highlight?: boolean;
  $hasErr?: boolean;
  $noWrap?: boolean;
  $clickable?: boolean;
  $dimmed?: boolean;
}>`
  font-family: ${MONO_STACK};
  font-size: 12.5px;
  white-space: ${(p) => (p.$noWrap ? "pre" : "pre-wrap")};
  word-break: ${(p) => (p.$noWrap ? undefined : "break-all")};
  line-height: 1.65;
  padding: 0 2px;
  border-radius: 4px;
  opacity: ${(p) => (p.$dimmed ? 0.75 : 1)};
  background-color: ${(p) =>
    p.$highlight
      ? "rgba(30,63,102,0.42)"
      : p.$hasErr
        ? "rgba(211,47,47,0.10)"
        : "transparent"};
  color: #b8bfc9;
  cursor: ${(p) => (p.$clickable ? "pointer" : undefined)};
  &:hover {
    background-color: ${(p) =>
      p.$clickable && !p.$highlight && !p.$hasErr ? "rgba(255,255,255,0.06)" : undefined};
  }
`;

const TsSpan = styled("span")`
  color: #828b99;
  margin-right: 4px;
`;

const KeySpan = styled("span")<{ $kind: "e" | "w" | "s" }>`
  color: ${(p) => COLORS[p.$kind]};
  font-weight: ${(p) => (p.$kind === "e" ? 700 : 400)};
`;

/** 单行着色渲染。
 *
 *  用 emotion `styled` 组件（而非 MUI sx / css prop），emotion 一定会
 *  生成正确的选择器，生产构建不会丢样式。v0.2.66 的用户反馈全灰白
 *  就是 MUI sx 嵌套选择器被 emotion 哈希隔离导致的。 */
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
  highlight?: boolean;
  errorRowBg?: boolean;
  dimmed?: boolean;
  noWrap?: boolean;
  innerRef?: Ref<HTMLDivElement>;
}) {
  const { ts, parts } = tokenizeLine(line);
  const hasErr = errorRowBg ?? (parts.some((p) => p.k === "e") || STACK_RE.test(line));

  return (
    <LogRow
      ref={innerRef}
      $highlight={highlight}
      $hasErr={hasErr}
      $noWrap={noWrap}
      $clickable={!!onClick}
      $dimmed={dimmed}
      onClick={onClick}
    >
      {ts && <TsSpan>{ts}</TsSpan>}
      {parts.map((p, j) => (p.k ? (
        <KeySpan key={j} $kind={p.k}>{p.t}</KeySpan>
      ) : (
        <span key={j}>{p.t}</span>
      )))}
    </LogRow>
  );
}

/** 全站日志终端统一底色。 */
export const LOG_BG = "#1e1e1e";
