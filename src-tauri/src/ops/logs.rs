use std::io::{Read, Seek, SeekFrom};

/// 剥掉 ANSI/C/VT 100 转义序列。
/// Spring Boot logback 在非 TTY 环境（文件输出）也可能因为 Windows Java
/// 的 isatty() bug 误判为 TTY，往 dfy-server.log 写入 `\x1b[32m` 这种彩色
/// 控制序列——前端直接渲染就是乱码。我们在后端 tail 读文件时就剥掉，
/// 监控中心/服务详情等所有日志入口都干净，不用每个前端页面各处理一次。
///
/// 覆盖：CSI 序列（\x1b[...m/...K 等）、OSC 序列（\x1b]...\x07）、
/// 单字符转义（\x07、\x1b\\）。不碰 UTF-8 多字节字符（ANSI 只用单字节）。
pub fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        // CSI: \x1b[ 后跟任意个 [0-9;?] 再跟字母
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() {
                let b = bytes[j];
                if (0x30..=0x3f).contains(&b) {
                    j += 1;
                } else if b.is_ascii_alphabetic() {
                    j += 1;
                    break;
                } else {
                    break;
                }
            }
            i = j;
        // OSC: \x1b] 后跟任意字符直到 \x07 或 \x1b\\
        } else if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b']' {
            let mut j = i + 2;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    j += 1;
                    break;
                } else if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                    j += 2;
                    break;
                }
                j += 1;
            }
            i = j;
        // APC/DCS/PM 等：\x1bPX 到 ST
        } else if bytes[i] == 0x1b && i + 1 < bytes.len() && (bytes[i + 1] == b'P' || bytes[i + 1] == b'(' || bytes[i + 1] == b')') {
            let mut j = i + 2;
            while j + 1 < bytes.len() {
                if bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
                    j += 2;
                    break;
                }
                j += 1;
            }
            i = j;
        } else if bytes[i] == 0x07 {
            // BEL 单字符
            i += 1;
        } else {
            // 安全复制：按 UTF-8 边界前进
            let n = utf8_char_len(bytes[i]);
            out.push_str(&s[i..i + n]);
            i += n;
        }
    }
    out
}

fn utf8_char_len(b: u8) -> usize {
    // 单字节字符或 continuation byte（后者不应单独出现）都按 1 字节复制，
    // 不做边界判定让它变成替换符——用户需要看到乱码里的原始字节来诊断。
    if b < 0xc0 { 1 }
    else if b < 0xe0 { 2 }
    else if b < 0xf0 { 3 }
    else { 4 }
}

/// 日志文件名判定：`.log` 结尾，或 `.log.<日期>` 轮转形态。
/// tracing-appender 按天轮转出的 edgecd-daemon.log.2026-11-06、
/// logback 的 dfy-server.log.2026-11-04.0.gz 都属于后者——
/// 目录声明方式收集日志时两者都必须收录，否则轮转文件永远进不了监控中心。
pub fn is_log_file_name(name: &str) -> bool {
    name.ends_with(".log") || name.split(".log.").count() > 1
}

/// 日志字节解码：优先按 UTF-8；无效时回退 GBK。
/// 写入端已在 v0.2.54 统一 UTF-8（logging.charset + file.encoding），
/// 但存量 GBK 文件（历史日志、其他未托管服务）仍会被 tail 读到——
/// 纯 lossy 会把 GBK 中文全部变成替换符，监控中心看起来就是大片乱码。
pub fn decode_log_bytes(bytes: &[u8]) -> String {
    let decoded = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    };
    strip_ansi(&decoded)
}

/// 读取日志文件末尾 `lines` 行。
///
/// 实现：从文件末尾按 8KB 块向前回读，凑够 N 行即停——
/// 不再把整个文件读进内存（此前 read_to_string 对大日志开销过高）。
/// 文件不存在/无权限时返回可读的错误说明。
pub fn tail(path: &str, lines: usize) -> String {
    let lines = lines.max(1);
    let Ok(mut f) = std::fs::File::open(path) else {
        return format!("无法读取日志 {path}: 文件不存在或无权限");
    };
    let len = match f.metadata() {
        Ok(m) => m.len(),
        Err(e) => return format!("无法读取日志 {path}: {e}"),
    };
    if len == 0 {
        return String::new();
    }

    const CHUNK: u64 = 8 * 1024;
    // 各块按文件顺序收集（push_front），最后一次性拼接——
    // 不再做「前插合并」，那是每块 O(已累计字节数) 的 O(n²) 操作。
    let mut chunks: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::new();
    // 换行数增量累加——此前每轮回读都从头扫整段 buf 数换行（O(n²)），
    // 现在每块只扫自己新增的 CHUNK 字节。
    let mut newlines = 0usize;
    let mut pos = len;
    let mut reached_start = false;

    while !reached_start {
        let start = pos.saturating_sub(CHUNK);
        let size = (pos - start) as usize;
        let mut tmp = vec![0u8; size];
        if f.seek(SeekFrom::Start(start)).is_err() {
            break;
        }
        if f.read_exact(&mut tmp).is_err() {
            break;
        }
        newlines += tmp.iter().filter(|&&b| b == b'\n').count();
        chunks.push_front(tmp);
        pos = start;
        reached_start = pos == 0;
        // 凑够 lines+1 个换行即可停止（最后一行可能无换行符）
        if newlines > lines {
            break;
        }
    }
    let buf: Vec<u8> = chunks.into_iter().flatten().collect();

    let text = decode_log_bytes(&buf);
    let mut all: Vec<&str> = text.lines().collect();
    // 未回读到文件头时，第一行可能从多字节字符中间截断，丢弃以防乱码
    if !reached_start && !all.is_empty() {
        all.remove(0);
    }
    let skip = all.len().saturating_sub(lines);
    all.into_iter().skip(skip).collect::<Vec<&str>>().join("\n")
}

/// 合并多个日志路径的尾部输出，段落以路径标题分隔。
pub fn tail_many(paths: &[String], lines: usize) -> String {
    let mut out = String::new();
    for p in paths {
        out.push_str(&format!("===== {p} =====\n"));
        out.push_str(&tail(p, lines));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_returns_last_n_lines() {
        let dir = std::env::temp_dir().join("dfy_ops_logs_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tail_test.log");
        let content: String = (1..=1000).map(|i| format!("line-{i}\n")).collect();
        std::fs::write(&path, &content).unwrap();

        let out = tail(path.to_str().unwrap(), 5);
        let got: Vec<&str> = out.lines().collect();
        assert_eq!(got.len(), 5);
        assert_eq!(got[0], "line-996");
        assert_eq!(got[4], "line-1000");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn tail_handles_missing_file() {
        let out = tail("/nonexistent/dfy_ops/nope.log", 10);
        assert!(out.contains("无法读取日志"));
    }

    #[test]
    fn tail_small_file_returns_all() {
        let dir = std::env::temp_dir().join("dfy_ops_logs_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tail_small.log");
        std::fs::write(&path, "a\nb\nc").unwrap();

        let out = tail(path.to_str().unwrap(), 10);
        assert_eq!(out, "a\nb\nc");
        std::fs::remove_file(&path).ok();
    }
}
