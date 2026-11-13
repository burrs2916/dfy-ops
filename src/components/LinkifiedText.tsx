import Link from "@mui/material/Link";
import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

/**
 * 把消息文本里的 http(s) 链接渲染成可点击项：点击调用系统默认浏览器直接打开，
 * 用户不需要复制、不需要猜地址。打开失败（无 Tauri 环境 / 权限缺失）时：
 * 复制该链接到剪贴板并原地提示手动粘贴到浏览器。
 */
export function LinkifiedText({ text }: { text: string }) {
  const [failed, setFailed] = useState(false);
  const parts = text.split(/(https?:\/\/[^\s，、；）)]+)/g);
  return (
    <>
      {parts.map((p, i) =>
        /^https?:\/\//.test(p) ? (
          <Link
            key={i}
            component="button"
            type="button"
            underline="hover"
            sx={{ font: "inherit", color: "inherit", mx: 0.25 }}
            onClick={() => {
              setFailed(false);
              void openUrl(p).catch(() => {
                setFailed(true);
                void navigator.clipboard?.writeText(p).catch(() => undefined);
              });
            }}
          >
            {p}
          </Link>
        ) : (
          <span key={i}>{p}</span>
        ),
      )}
      {failed && <span>（无法自动打开，链接已复制，请粘贴到浏览器）</span>}
    </>
  );
}

export default LinkifiedText;
