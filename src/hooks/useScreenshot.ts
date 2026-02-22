import { useCallback } from "react";
import { startRegionSelect } from "../lib/invoke";
import { appLog } from "../stores/logStore";

export function useScreenshot() {
  const startRegion = useCallback(async (mode: string = "screenshot") => {
    try {
      appLog.info("[Screenshot] 启动区域选择, mode=" + mode);
      await startRegionSelect(mode);
      appLog.info("[Screenshot] 区域选择窗口已创建");
    } catch (e) {
      appLog.error("[Screenshot] 区域选择启动失败: " + String(e));
    }
  }, []);

  return { startRegion };
}
