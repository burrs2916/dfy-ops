import { Box, Stack, Typography } from "@mui/material";
import { ContentCopy, OpenInNew } from "@mui/icons-material";
import { openUrl } from "@tauri-apps/plugin-opener";
import { accessLinks } from "../access";

/**
 * 卡片常驻地址行：状态行下方直接标注访问地址，不用弹框、不会消失。
 * - 浏览器页面地址：链接色 + 下划线，点击默认浏览器打开（href 只含纯 URL，无多余文字）；
 * - 非网页地址（数据库/API）：等宽纯文本，点击复制；
 * - 停止 / 未安装 / 其他状态一律不渲染。
 */
export function AccessLinks({
  id,
  opts,
  running,
}: {
  id: string;
  opts?: { kind?: string; ports?: number[]; remote?: [string, number] | null };
  running: boolean;
}) {
  if (!running) return null;
  const links = accessLinks(id, opts);
  if (!links.length) return null;
  return (
    <Stack direction="row" spacing={1.5} alignItems="baseline" flexWrap="wrap" useFlexGap sx={{ mt: 1, minWidth: 0 }}>
      <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>
        访问地址
      </Typography>
      {links.map((l) =>
        l.url ? (
          <Typography key={l.url} variant="caption" component="span" sx={{ color: "text.secondary" }}>
            {l.label}{" "}
            <Box
              component="span"
              onClick={() => void openUrl(l.url!).catch(() => undefined)}
              sx={{
                color: "primary.main",
                cursor: "pointer",
                textDecoration: "underline",
                textDecorationColor: "rgba(30,63,102,0.35)",
                "&:hover": { textDecorationColor: "primary.main" },
              }}
            >
              {l.url}
              <OpenInNew sx={{ fontSize: 11, ml: 0.4, verticalAlign: "text-bottom" }} />
            </Box>
          </Typography>
        ) : (
          <Typography
            key={l.label}
            variant="caption"
            component="span"
            onClick={() => void navigator.clipboard?.writeText(l.text ?? "").catch(() => undefined)}
            title="点击复制地址"
            sx={{ color: "text.secondary", display: "inline-flex", alignItems: "baseline", gap: 0.4 }}
          >
            {l.label}{" "}
            <Box component="span" sx={{ fontFamily: "monospace", color: "text.primary" }}>
              {l.text}
            </Box>
            <ContentCopy sx={{ fontSize: 11, color: "text.disabled" }} />
          </Typography>
        ),
      )}
    </Stack>
  );
}
