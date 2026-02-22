import { useState, useCallback, useEffect, useRef } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getFrozenScreenshot } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";

interface Selection {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
}

export function ScreenshotOverlay() {
  const [backgroundImage, setBackgroundImage] = useState<string>("");
  const [mode, setMode] = useState<string>("region");
  const [isSelecting, setIsSelecting] = useState(false);
  const selectionRef = useRef<Selection | null>(null);
  const [selRect, setSelRect] = useState<{ left: number; top: number; width: number; height: number } | null>(null);

  useEffect(() => {
    // Pull frozen screenshot data from backend on mount
    appLog.info("[Overlay] 截图覆盖层已挂载，获取冻结截图...");
    getFrozenScreenshot()
      .then((data) => {
        appLog.info("[Overlay] 冻结截图获取成功, mode=" + data.mode + ", image size=" + data.image.length);
        setBackgroundImage(data.image);
        setMode(data.mode);
      })
      .catch((e) => {
        appLog.error("[Overlay] 获取冻结截图失败: " + String(e));
      });

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        getCurrentWindow().close();
      }
    };
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    setIsSelecting(true);
    selectionRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      endX: e.clientX,
      endY: e.clientY,
    };
    setSelRect(null);
  }, []);

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!isSelecting || !selectionRef.current) return;
      selectionRef.current = {
        ...selectionRef.current,
        endX: e.clientX,
        endY: e.clientY,
      };
      const sel = selectionRef.current;
      setSelRect({
        left: Math.min(sel.startX, sel.endX),
        top: Math.min(sel.startY, sel.endY),
        width: Math.abs(sel.endX - sel.startX),
        height: Math.abs(sel.endY - sel.startY),
      });
    },
    [isSelecting]
  );

  const handleMouseUp = useCallback(async () => {
    if (!isSelecting || !selectionRef.current) return;
    setIsSelecting(false);

    const selection = selectionRef.current;
    const x = Math.min(selection.startX, selection.endX);
    const y = Math.min(selection.startY, selection.endY);
    const width = Math.abs(selection.endX - selection.startX);
    const height = Math.abs(selection.endY - selection.startY);

    if (width < 5 || height < 5) {
      appLog.warn("[Overlay] 选区太小 (" + width + "x" + height + ")，已忽略");
      selectionRef.current = null;
      setSelRect(null);
      return;
    }

    // Scale by devicePixelRatio for physical pixels
    const dpr = window.devicePixelRatio || 1;
    appLog.info(`[Overlay] 选区完成: logical=(${x},${y},${width}x${height}), dpr=${dpr}, physical=(${Math.round(x * dpr)},${Math.round(y * dpr)},${Math.round(width * dpr)}x${Math.round(height * dpr)}), mode=${mode}`);

    await emit("region-selected", {
      x: Math.round(x * dpr),
      y: Math.round(y * dpr),
      width: Math.round(width * dpr),
      height: Math.round(height * dpr),
      mode,
    });

    getCurrentWindow().close();
  }, [isSelecting, mode]);

  return (
    <div
      className="fixed inset-0 cursor-crosshair select-none"
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      style={{
        backgroundImage: backgroundImage
          ? `url(data:image/png;base64,${backgroundImage})`
          : undefined,
        backgroundSize: "cover",
        backgroundPosition: "center",
      }}
    >
      {/* Dark overlay */}
      <div className="absolute inset-0 bg-black/30" />

      {/* Selection rectangle */}
      {selRect && selRect.width > 0 && selRect.height > 0 && (
        <>
          {/* Clear the selected region */}
          <div
            className="absolute border-2 border-blue-500 bg-transparent"
            style={{
              left: selRect.left,
              top: selRect.top,
              width: selRect.width,
              height: selRect.height,
              boxShadow: "0 0 0 9999px rgba(0, 0, 0, 0.3)",
            }}
          />
          {/* Size indicator */}
          <div
            className="absolute text-xs text-white bg-blue-500 px-2 py-0.5 rounded"
            style={{
              left: selRect.left,
              top: selRect.top - 24,
            }}
          >
            {Math.round(selRect.width * (window.devicePixelRatio || 1))} x{" "}
            {Math.round(selRect.height * (window.devicePixelRatio || 1))}
          </div>
        </>
      )}

      {/* Instructions */}
      {!selRect && (
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-white text-lg font-medium pointer-events-none">
          Drag to select region · ESC to cancel
        </div>
      )}
    </div>
  );
}
