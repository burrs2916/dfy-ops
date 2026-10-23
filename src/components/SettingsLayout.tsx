import { useCallback, useRef, useState } from "react";
import { Box, List, ListItemButton, ListItemIcon, ListItemText, Typography } from "@mui/material";
import type { ReactNode } from "react";

/** 一个设置分区：左导航项 + 右侧标题/描述/内容块。 */
export interface SettingSection {
  key: string;
  icon: ReactNode;
  /** 左导航显示名 */
  label: string;
  /** 右侧分区标题 */
  title: string;
  /** 右侧分区描述 */
  description?: string;
  content: ReactNode;
}

/**
 * 设置类页面统一布局——1:1 复刻 web-craft SettingsPage：
 * 根容器占满视口（抵消外层 main 的 p:3，导航贴边），左侧 220px 通栏导航
 * （borderRight 分隔线、uppercase 小标题、图标 20px），右侧内容独立滚动
 * （px4 py3，内层 maxWidth 680），滚动时按 offsetTop 同步高亮当前分区。
 * 「设置」与「AI助手」共用，保证两页与参考项目风格完全一致。
 */
export function SettingsLayout({
  navTitle,
  header,
  sections,
}: {
  navTitle: string;
  /** 渲染在内容区顶部的可选项（如错误提示） */
  header?: ReactNode;
  sections: SettingSection[];
}) {
  const [activeSection, setActiveSection] = useState<string>(sections[0]?.key ?? "");
  const contentRef = useRef<HTMLDivElement>(null);
  const sectionRefs = useRef<Record<string, HTMLDivElement | null>>({});

  const scrollToSection = (key: string) => {
    sectionRefs.current[key]?.scrollIntoView({ behavior: "smooth", block: "start" });
    setActiveSection(key);
  };

  const handleScroll = useCallback(() => {
    const container = contentRef.current;
    if (!container) return;
    const scrollTop = container.scrollTop + 80;
    for (let i = sections.length - 1; i >= 0; i--) {
      const el = sectionRefs.current[sections[i].key];
      if (el && el.offsetTop <= scrollTop) {
        setActiveSection(sections[i].key);
        break;
      }
    }
  }, [sections]);

  return (
    <Box
      sx={{
        display: "flex",
        // 视口高减顶部 AppBar(64)；负 margin 抵消外层 main 的 p:3，使导航贴边通栏
        height: "calc(100vh - 64px)",
        mx: -3,
        my: -3,
        overflow: "hidden",
      }}
    >
      {/* 左侧分区导航（web-craft：220px + borderRight，无卡片包裹） */}
      <Box
        sx={{
          width: 220,
          flexShrink: 0,
          borderRight: "1px solid",
          borderColor: "divider",
          display: "flex",
          flexDirection: "column",
          pt: 2,
        }}
      >
        <Typography
          variant="subtitle2"
          sx={{
            px: 2.5,
            pb: 1.5,
            color: "text.secondary",
            textTransform: "uppercase",
            letterSpacing: "0.06em",
            fontSize: "0.7rem",
          }}
        >
          {navTitle}
        </Typography>
        <List sx={{ px: 1 }}>
          {sections.map((section) => (
            <ListItemButton
              key={section.key}
              selected={activeSection === section.key}
              onClick={() => scrollToSection(section.key)}
              sx={{
                borderRadius: 1,
                mb: 0.25,
                py: 1,
                "&.Mui-selected": {
                  backgroundColor: "action.selected",
                  "&:hover": { backgroundColor: "action.selected" },
                  "& .MuiListItemIcon-root": { color: "primary.main" },
                  "& .MuiListItemText-primary": { color: "primary.main", fontWeight: 600 },
                },
              }}
            >
              <ListItemIcon
                sx={{
                  minWidth: 36,
                  color: activeSection === section.key ? "primary.main" : "text.secondary",
                  "& .MuiSvgIcon-root": { fontSize: 20 },
                }}
              >
                {section.icon}
              </ListItemIcon>
              <ListItemText
                primary={section.label}
                sx={{
                  "& .MuiListItemText-primary": {
                    fontSize: "0.875rem",
                    fontWeight: activeSection === section.key ? 600 : 400,
                  },
                }}
              />
            </ListItemButton>
          ))}
        </List>
      </Box>

      {/* 右侧内容（独立滚动，web-craft：px4 py3 + 内层 maxWidth 680） */}
      <Box
        ref={contentRef}
        onScroll={handleScroll}
        sx={{ flex: 1, overflow: "auto", px: 4, py: 3, position: "relative" }}
      >
        <Box sx={{ maxWidth: 680 }}>
          {header}
          {sections.map((s) => (
            <Box
              key={s.key}
              ref={(el: HTMLDivElement | null) => {
                sectionRefs.current[s.key] = el;
              }}
              sx={{ mb: 5 }}
            >
              <Typography variant="h6" sx={{ mb: 0.5, fontWeight: 600 }}>
                {s.title}
              </Typography>
              {s.description && (
                <Typography variant="body2" color="text.secondary" sx={{ mb: 2.5 }}>
                  {s.description}
                </Typography>
              )}
              {s.content}
            </Box>
          ))}
        </Box>
      </Box>
    </Box>
  );
}
