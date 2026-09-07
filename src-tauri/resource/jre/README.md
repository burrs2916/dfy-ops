# resource/jre — 内置 JRE（绿色交付）

本目录由 **CI 构建时自动填充**（GitHub Actions 下载 Adoptium Temurin 8 JRE 并解压到此处，
随 NSIS 安装包交付），安装后位于**程序 exe 同级目录**下。

- dfy-backend 按声明使用本 JRE 启动（`{java}` 占位符解析为 `bin/java.exe`），免安装、
  不写系统环境变量、不需要管理员权限。
- 本目录缺失时，环境检查会报「内置 JRE 缺失」并提示从交付包恢复本目录即可。
- 仓库中仅保留本说明文件作为占位（保证打包 glob 永远有匹配）。
