/** 通用按钮 sx：统一去掉大写、禁止换行 */
export const btnSx = { textTransform: "none" as const, whiteSpace: "nowrap" as const };

/** 系统库清单：备份范围下拉排除（与后端 SYS_DATABASES 同口径） */
export const SYS_DBS = ["information_schema", "mysql", "performance_schema", "sys"];

/** 主页面右栏 Tab 类型 */
export type RightTab = "data" | "schema" | "sql" | "users" | "server";
