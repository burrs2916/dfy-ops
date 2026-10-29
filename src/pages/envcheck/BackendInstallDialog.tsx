import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  TextField,
  Stack,
  Typography,
  Box,
  Divider,
} from "@mui/material";
import { useState } from "react";
import SettingsInputComponentIcon from "@mui/icons-material/SettingsInputComponent";

/** dfy-backend 外部配置（与后端 ExternalConfig serde 字段一一对应，camelCase）。 */
export type ExternalConfigPayload = {
  serverPort: string;
  mysqlMasterUrl: string;
  mysqlUsername: string;
  mysqlPassword: string;
  mysqlSlaveUrl: string;
  redisHost: string;
  redisPort: string;
  redisPassword: string;
  vernemqHostUrl: string;
  vernemqUsername: string;
  vernemqPassword: string;
};

/** 生产默认参数（与后端 external_config::TEMPLATE 同源：dfy_base_db / dfy123456 / 48085 / 18830）。 */
export const BACKEND_PROD_DEFAULTS: ExternalConfigPayload = {
  serverPort: "48085",
  mysqlMasterUrl:
    "jdbc:mysql://127.0.0.1:3306/dfy_base_db?useSSL=false&characterEncoding=utf8&serverTimezone=Asia/Shanghai",
  mysqlUsername: "root",
  mysqlPassword: "dfy123456",
  mysqlSlaveUrl:
    "jdbc:mysql://127.0.0.1:3306/dfy_base_db?characterEncoding=utf8&serverTimezone=Asia/Shanghai",
  redisHost: "127.0.0.1",
  redisPort: "6379",
  redisPassword: "dfy123456",
  vernemqHostUrl: "tcp://127.0.0.1:18830",
  vernemqUsername: "cloud",
  vernemqPassword: "key@00",
};

interface Props {
  open: boolean;
  /** 确认安装：带用户确认后的配置 */
  onConfirm: (cfg: ExternalConfigPayload) => void;
  onClose: () => void;
}

/** DFY 后端安装前配置确认弹窗：
 *  一键安装 dfy-backend 前先展示/编辑连接参数（生产默认值预填），
 *  用户点「确认并安装」才真正开始安装——配置写入 install_dir/config/application-local.yaml，
 *  Spring Boot 启动时自动覆盖 jar 内默认配置。 */
export default function BackendInstallDialog({ open, onConfirm, onClose }: Props) {
  const [cfg, setCfg] = useState<ExternalConfigPayload>(BACKEND_PROD_DEFAULTS);

  const set = (k: keyof ExternalConfigPayload) => (
    e: React.ChangeEvent<HTMLInputElement>,
  ) => setCfg((c) => ({ ...c, [k]: e.target.value }));

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <SettingsInputComponentIcon color="primary" />
        DFY 后端安装前配置确认
      </DialogTitle>
      <DialogContent dividers>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
          以下参数会写入 <b>config/application-local.yaml</b>（外部配置，优先级高于
          jar 内置配置，可随时在服务详情页修改）。已预填生产默认值，确认无误后开始安装。
        </Typography>
        <Stack spacing={2}>
          <TextField
            size="small"
            label="服务端口（server.port）"
            value={cfg.serverPort}
            onChange={set("serverPort")}
          />
          <Divider>
            <Typography variant="caption" color="text.secondary">
              MySQL 数据库
            </Typography>
          </Divider>
          <TextField
            size="small"
            label="主库连接（jdbc url）"
            value={cfg.mysqlMasterUrl}
            onChange={set("mysqlMasterUrl")}
            multiline
            minRows={2}
          />
          <Box sx={{ display: "flex", gap: 2 }}>
            <TextField
              size="small"
              label="用户名"
              value={cfg.mysqlUsername}
              onChange={set("mysqlUsername")}
              sx={{ flex: 1 }}
            />
            <TextField
              size="small"
              label="密码"
              value={cfg.mysqlPassword}
              onChange={set("mysqlPassword")}
              type="password"
              sx={{ flex: 1 }}
            />
          </Box>
          <TextField
            size="small"
            label="从库连接（可留空表示同主库）"
            value={cfg.mysqlSlaveUrl}
            onChange={set("mysqlSlaveUrl")}
            multiline
            minRows={2}
          />
          <Divider>
            <Typography variant="caption" color="text.secondary">
              Redis
            </Typography>
          </Divider>
          <Box sx={{ display: "flex", gap: 2 }}>
            <TextField
              size="small"
              label="主机"
              value={cfg.redisHost}
              onChange={set("redisHost")}
              sx={{ flex: 1 }}
            />
            <TextField
              size="small"
              label="端口"
              value={cfg.redisPort}
              onChange={set("redisPort")}
              sx={{ flex: 1 }}
            />
            <TextField
              size="small"
              label="密码"
              value={cfg.redisPassword}
              onChange={set("redisPassword")}
              type="password"
              sx={{ flex: 1 }}
            />
          </Box>
          <Divider>
            <Typography variant="caption" color="text.secondary">
              VerneMQ（MQTT 消息）
            </Typography>
          </Divider>
          <TextField
            size="small"
            label="连接地址"
            value={cfg.vernemqHostUrl}
            onChange={set("vernemqHostUrl")}
          />
          <Box sx={{ display: "flex", gap: 2 }}>
            <TextField
              size="small"
              label="用户名"
              value={cfg.vernemqUsername}
              onChange={set("vernemqUsername")}
              sx={{ flex: 1 }}
            />
            <TextField
              size="small"
              label="密码"
              value={cfg.vernemqPassword}
              onChange={set("vernemqPassword")}
              sx={{ flex: 1 }}
            />
          </Box>
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} sx={{ textTransform: "none" }}>
          取消
        </Button>
        <Button
          variant="contained"
          onClick={() =>
            onConfirm({
              ...cfg,
              // 从库留空 = 与主库同源（dynamic-datasource 空值会启动报错）
              mysqlSlaveUrl: cfg.mysqlSlaveUrl.trim() || cfg.mysqlMasterUrl,
            })
          }
          sx={{ textTransform: "none" }}
        >
          确认并安装
        </Button>
      </DialogActions>
    </Dialog>
  );
}
