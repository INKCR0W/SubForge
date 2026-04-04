import type { WindowCloseBehavior } from "../../types/core";

export const CLOSE_BEHAVIOR_OPTIONS: Array<{
  value: WindowCloseBehavior;
  label: string;
  description: string;
}> = [
  {
    value: "tray_minimize",
    label: "最小化到托盘",
    description: "点击窗口关闭按钮时仅隐藏窗口，GUI 进程保持运行。",
  },
  {
    value: "close_gui",
    label: "仅关闭 GUI",
    description: "关闭管理界面进程，Core 守护进程继续运行。",
  },
  {
    value: "close_gui_and_stop_core",
    label: "关闭 GUI 并停止 Core",
    description: "关闭管理界面时同时停止 Core 进程。",
  },
];
