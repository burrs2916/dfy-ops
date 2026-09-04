fn main() {
    // 图标变化时触发重编译，让 tauri dev 自动重启应用（运行时 Dock 图标读的是这个目录）
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
