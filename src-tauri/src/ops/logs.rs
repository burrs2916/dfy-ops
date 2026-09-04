use std::io::{Read, Seek, SeekFrom};

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
    let mut buf: Vec<u8> = Vec::new();
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
        // 前插：保持 buf 为从 start 到原末尾的连续字节
        let mut merged = tmp;
        merged.extend_from_slice(&buf);
        buf = merged;
        pos = start;
        reached_start = pos == 0;
        // 凑够 lines+1 个换行即可停止（最后一行可能无换行符）
        if buf.iter().filter(|&&b| b == b'\n').count() > lines {
            break;
        }
    }

    let text = String::from_utf8_lossy(&buf);
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
