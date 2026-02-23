import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { TitleBar } from "./components/common/TitleBar";
import { TranslationPanel } from "./components/translation/TranslationPanel";
import { useSettingsStore } from "./stores/settingsStore";
import { openSettingsWindow } from "./stores/settingsStore";
import { useTranslationStore } from "./stores/translationStore";
import { appLog, openDebugWindow, setupMainWindowLogListeners } from "./stores/logStore";
import { useScreenshot } from "./hooks/useScreenshot";
import { useTranslation } from "./hooks/useTranslation";
import { captureRegion, recognizeText, copyImageToClipboard, getSettings } from "./lib/invoke";
import type { RegionSelectEvent } from "./types";

export default function App() {
  const { setSettings } = useSettingsStore();
  const { startRegion } = useScreenshot();
  const { translate, setSourceText } = useTranslation();
  const { sourceLang } = useTranslationStore();

  // Keep a ref to latest sourceLang to avoid stale closure in event listener
  const sourceLangRef = useRef(sourceLang);
  sourceLangRef.current = sourceLang;

  useEffect(() => {
    appLog.info("[App] 主窗口初始化");

    // Load settings into store on startup
    getSettings()
      .then((s) => {
        setSettings(s);
        appLog.info("[App] 初始配置加载完成, base_url=" + s.base_url);
      })
      .catch((e) => appLog.error("[App] 初始配置加载失败: " + String(e)));

    const appWindow = getCurrentWindow();

    // Hide window when it loses focus (unless pinned/always-on-top)
    const unlistenBlur = appWindow.onFocusChanged(async ({ payload: focused }) => {
      if (!focused) {
        const isOnTop = await appWindow.isAlwaysOnTop();
        if (!isOnTop) {
          // Small delay to let the OS settle focus on the target window
          await new Promise((r) => setTimeout(r, 80));
          // Don't hide if screenshot overlay is active (main window will be restored when overlay closes)
          const overlayWin = await WebviewWindow.getByLabel("screenshot-overlay");
          if (overlayWin) return;
          // Don't hide if focus went to our debug-log or settings window
          const debugWin = await WebviewWindow.getByLabel("debug-log");
          const settingsWin = await WebviewWindow.getByLabel("settings");
          if ((debugWin && await debugWin.isFocused()) || (settingsWin && await settingsWin.isFocused())) return;
          // 关闭子窗口
          if (debugWin) await debugWin.close();
          if (settingsWin) await settingsWin.close();
          appLog.info("[App] 主窗口失焦，自动隐藏");
          await appWindow.hide();
        }
      }
    });

    // Listen for region selection events from overlay
    const unlistenRegion = listen<RegionSelectEvent>("region-selected", async (event) => {
      const { x, y, width, height, mode } = event.payload;
      appLog.info(`[App] 收到区域选择事件: region=(${x},${y},${width}x${height}), mode=${mode}`);

      try {
        const imageBase64 = await captureRegion(x, y, width, height);
        appLog.info("[App] 区域裁切完成, base64 size=" + imageBase64.length);

        if (mode === "screenshot") {
          // Screenshot mode: copy to clipboard, don't show main window
          appLog.info("[App] screenshot 模式，复制图片到剪贴板...");
          await copyImageToClipboard(imageBase64);
          appLog.info("[App] 图片已复制到剪贴板");
        } else if (mode === "ocr_translate") {
          // OCR + Translate mode: OCR → set source text → translate → show window
          appLog.info("[App] ocr_translate 模式，开始 OCR...");
          const ocrText = await recognizeText(imageBase64, sourceLangRef.current);
          appLog.info("[App] OCR 完成, 文本长度=" + ocrText.length);

          if (ocrText.trim()) {
            setSourceText(ocrText);
            appLog.info("[App] 源文本已设置，开始翻译...");
            await translate(ocrText);
            appLog.info("[App] 翻译完成");
          } else {
            appLog.warn("[App] OCR 结果为空，跳过翻译");
          }
        }
      } catch (e) {
        appLog.error("[App] 区域处理失败: " + String(e));
      }
    });

    // Listen for tray actions
    const unlistenTray = listen<string>("tray-action", (event) => {
      appLog.info("[App] 托盘事件: " + event.payload);
      handleAction(event.payload);
    });

    // Listen for hotkey actions
    const unlistenHotkey = listen<string>("hotkey-action", (event) => {
      appLog.info("[App] 快捷键事件: " + event.payload);
      handleAction(event.payload);
    });

    // Listen for settings-saved event from settings window
    const unlistenSettingsSaved = listen("settings-saved", () => {
      appLog.info("[App] 收到 settings-saved 事件，重新加载配置");
      getSettings()
        .then((s) => {
          setSettings(s);
          appLog.info("[App] 配置已刷新");
        })
        .catch((e) => appLog.error("[App] 配置刷新失败: " + String(e)));
    });

    // Set up debug log event listeners (main window side)
    const cleanupLogListeners = setupMainWindowLogListeners();

    return () => {
      unlistenBlur.then((fn) => fn());
      unlistenRegion.then((fn) => fn());
      unlistenTray.then((fn) => fn());
      unlistenHotkey.then((fn) => fn());
      unlistenSettingsSaved.then((fn) => fn());
      cleanupLogListeners();
    };
  }, []);

  const handleAction = async (action: string) => {
    switch (action) {
      case "screenshot":
        startRegion("screenshot");
        break;
      case "ocr_translate":
        startRegion("ocr_translate");
        break;
    }
  };

  return (
    <div className="flex flex-col h-screen rounded-xl overflow-hidden" style={{ backgroundColor: "var(--color-bg)" }}>
      <TitleBar
        onScreenshot={() => startRegion("screenshot")}
        onOcrTranslate={() => startRegion("ocr_translate")}
        onDebugLog={() => openDebugWindow()}
        onSettings={() => openSettingsWindow()}
      />

      <TranslationPanel />
    </div>
  );
}
