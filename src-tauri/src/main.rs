// 发布版隐藏控制台窗口（调试版保留终端以查看日志）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dfy_ops_lib::run()
}
