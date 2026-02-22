import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

interface DockedWindowConfig {
  label: string;
  url: string;
  title: string;
  width: number;
  /** "left" = dock to the left of main window, "right" = dock to the right */
  side: "left" | "right";
  /** Gap in logical pixels between main window and docked window */
  gap?: number;
}

/**
 * Open (or focus) a window docked beside the main window.
 * Shared logic for debug-log and settings windows.
 */
export async function openDockedWindow(config: DockedWindowConfig) {
  const { label, url, title, width, side, gap = 8 } = config;

  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    console.log(`[WindowUtils] ${label} 窗口已存在，focus`);
    await existing.setFocus();
    return;
  }

  const mainWindow = getCurrentWindow();
  const position = await mainWindow.outerPosition();
  const size = await mainWindow.outerSize();
  const factor = await mainWindow.scaleFactor();

  const mainX = position.x / factor;
  const mainY = position.y / factor;
  const mainW = size.width / factor;
  const mainH = size.height / factor;

  const x = side === "right" ? mainX + mainW + gap : mainX - width - gap;

  console.log(`[WindowUtils] 创建 ${label} 窗口, 位置=(${x}, ${mainY}), 尺寸=(${width}x${mainH})`);

  const webview = new WebviewWindow(label, {
    url,
    title,
    x,
    y: mainY,
    width,
    height: mainH,
    decorations: false,
    transparent: true,
    resizable: true,
  });

  webview.once("tauri://error", (e) => {
    console.error(`Failed to create ${label} window:`, e);
  });
}
