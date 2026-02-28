import { useState, useCallback, useEffect, useRef } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getFrozenScreenshot } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";
import type { WindowRect, MonitorInfo } from "../../types";

interface Selection {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
}

export function ScreenshotOverlay() {
  const [backgroundUrl, setBackgroundUrl] = useState<string>("");
  const [mode, setMode] = useState<string>("region");
  const [windowRects, setWindowRects] = useState<WindowRect[]>([]);
  const [monitor, setMonitor] = useState<MonitorInfo | null>(null);
  const [monitorIndex, setMonitorIndex] = useState<number>(0);
  const [selRect, setSelRect] = useState<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);
  const [hoveredRect, setHoveredRect] = useState<WindowRect | null>(null);

  // Use refs for values accessed in mouse event handlers to avoid stale closures
  const isSelectingRef = useRef(false);
  const isDraggingRef = useRef(false);
  const selectionRef = useRef<Selection | null>(null);
  const mouseDownPosRef = useRef<{ x: number; y: number } | null>(null);
  const windowRectsRef = useRef<WindowRect[]>([]);
  const hoveredRectRef = useRef<WindowRect | null>(null);
  const modeRef = useRef(mode);
  const monitorRef = useRef<MonitorInfo | null>(null);
  const monitorIndexRef = useRef(0);
  const blobUrlRef = useRef<string | null>(null);

  // Keep refs in sync with state
  useEffect(() => {
    windowRectsRef.current = windowRects;
  }, [windowRects]);
  useEffect(() => {
    hoveredRectRef.current = hoveredRect;
  }, [hoveredRect]);
  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);
  useEffect(() => {
    monitorRef.current = monitor;
  }, [monitor]);
  useEffect(() => {
    monitorIndexRef.current = monitorIndex;
  }, [monitorIndex]);

  useEffect(() => {
    // Pull frozen screenshot data from backend on mount
    appLog.info("[Overlay] 截图覆盖层已挂载，获取冻结截图...");

    let cancelled = false;

    const loadScreenshot = async () => {
      // Determine which monitor this overlay corresponds to
      const currentWindow = getCurrentWindow();
      const label = currentWindow.label;
      const myMonitorIndex = parseInt(label.split("-").pop() || "0");

      const maxRetries = 3;
      for (let attempt = 1; attempt <= maxRetries; attempt++) {
        if (cancelled) return;
        try {
          const data = await getFrozenScreenshot(myMonitorIndex);
          if (cancelled) return;

          const monitors: MonitorInfo[] = data.monitors ?? [];
          const myMonitor = monitors[myMonitorIndex] ?? null;

          appLog.info(
            "[Overlay] 冻结截图获取成功, attempt=" +
              attempt +
              ", monitor_index=" +
              myMonitorIndex +
              ", mode=" +
              data.mode +
              ", image size=" +
              data.image.length +
              ", window_rects=" +
              (data.window_rects?.length ?? 0) +
              ", monitor=" +
              (myMonitor?.name ?? "unknown")
          );

          // Convert base64 to Blob URL — avoids WebKit issues with huge inline data URLs
          const binaryStr = atob(data.image);
          const bytes = new Uint8Array(binaryStr.length);
          for (let i = 0; i < binaryStr.length; i++) {
            bytes[i] = binaryStr.charCodeAt(i);
          }
          const blob = new Blob([bytes], { type: "image/png" });
          const url = URL.createObjectURL(blob);

          // Preload: ensure image is fully decoded before displaying
          const imgEl = await new Promise<HTMLImageElement>((resolve, reject) => {
            const img = new Image();
            img.onload = () => resolve(img);
            img.onerror = () => reject(new Error("Image decode failed"));
            img.src = url;
          });

          if (cancelled) {
            URL.revokeObjectURL(url);
            return;
          }

          appLog.info(
            "[Overlay] 图片已加载, monitor_index=" + myMonitorIndex +
              ", image=" + imgEl.naturalWidth + "x" + imgEl.naturalHeight +
              ", dpr=" + window.devicePixelRatio
          );

          blobUrlRef.current = url;
          setBackgroundUrl(url);
          setMode(data.mode);
          setWindowRects(data.window_rects ?? []);
          setMonitor(myMonitor);
          setMonitorIndex(myMonitorIndex);

          // Image is ready — now show the overlay window
          await getCurrentWindow().show();
          await getCurrentWindow().setFocus();
          appLog.info("[Overlay] 覆盖层窗口已显示, label=" + label);
          return; // success
        } catch (e) {
          appLog.warn(
            "[Overlay] 获取冻结截图失败 (attempt " + attempt + "/" + maxRetries + "): " + String(e)
          );
          if (attempt < maxRetries) {
            await new Promise((r) => setTimeout(r, 150));
          }
        }
      }
      appLog.error("[Overlay] 获取冻结截图最终失败，关闭覆盖层");
      getCurrentWindow().close();
    };

    loadScreenshot();

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Close ALL overlay windows via event, then close self
        emit("close-all-overlays");
        getCurrentWindow().close();
      }
    };
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      cancelled = true;
      window.removeEventListener("keydown", handleKeyDown);
      if (blobUrlRef.current) {
        URL.revokeObjectURL(blobUrlRef.current);
      }
    };
  }, []);

  // Find the topmost window rect that contains the given point.
  // Converts local window coordinates to global logical coordinates for comparison.
  const findWindowAtPoint = useCallback(
    (localX: number, localY: number): WindowRect | null => {
      const mon = monitorRef.current;
      if (!mon) return null;
      // Convert local CSS coords to global logical coords
      const globalX = localX + mon.x / mon.scale_factor;
      const globalY = localY + mon.y / mon.scale_factor;

      for (const rect of windowRectsRef.current) {
        if (
          globalX >= rect.x &&
          globalX <= rect.x + rect.width &&
          globalY >= rect.y &&
          globalY <= rect.y + rect.height
        ) {
          return rect;
        }
      }
      return null;
    },
    []
  );

  // Convert a global logical WindowRect to local coordinates for this monitor's overlay
  const toLocalRect = useCallback(
    (rect: WindowRect): { x: number; y: number; width: number; height: number } | null => {
      const mon = monitorRef.current;
      if (!mon) return null;
      const monLogicalX = mon.x / mon.scale_factor;
      const monLogicalY = mon.y / mon.scale_factor;
      return {
        x: rect.x - monLogicalX,
        y: rect.y - monLogicalY,
        width: rect.width,
        height: rect.height,
      };
    },
    []
  );

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    isSelectingRef.current = true;
    isDraggingRef.current = false;
    mouseDownPosRef.current = { x: e.clientX, y: e.clientY };
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
      if (!isSelectingRef.current) {
        // Hover mode: detect window under cursor
        const rect = findWindowAtPoint(e.clientX, e.clientY);
        setHoveredRect(rect);
        return;
      }

      if (!selectionRef.current) return;

      // Check if we've moved enough to be considered a drag (>= 5px)
      const downPos = mouseDownPosRef.current;
      if (downPos && !isDraggingRef.current) {
        const dx = e.clientX - downPos.x;
        const dy = e.clientY - downPos.y;
        if (Math.sqrt(dx * dx + dy * dy) >= 5) {
          isDraggingRef.current = true;
          setHoveredRect(null);
        }
      }

      selectionRef.current = {
        ...selectionRef.current,
        endX: e.clientX,
        endY: e.clientY,
      };

      if (isDraggingRef.current) {
        const sel = selectionRef.current;
        setSelRect({
          left: Math.min(sel.startX, sel.endX),
          top: Math.min(sel.startY, sel.endY),
          width: Math.abs(sel.endX - sel.startX),
          height: Math.abs(sel.endY - sel.startY),
        });
      }
    },
    [findWindowAtPoint]
  );

  const handleMouseUp = useCallback(async () => {
    if (!isSelectingRef.current) return;
    isSelectingRef.current = false;

    const downPos = mouseDownPosRef.current;
    const selection = selectionRef.current;
    const mon = monitorRef.current;

    if (!downPos || !selection || !mon) {
      isDraggingRef.current = false;
      return;
    }

    const currentMode = modeRef.current;
    const currentMonitorIndex = monitorIndexRef.current;
    const dpr = window.devicePixelRatio || 1;
    const dx = selection.endX - downPos.x;
    const dy = selection.endY - downPos.y;
    const distance = Math.sqrt(dx * dx + dy * dy);

    // Monitor logical origin for coordinate conversion
    const monLogicalX = mon.x / mon.scale_factor;
    const monLogicalY = mon.y / mon.scale_factor;

    if (distance < 5) {
      // Click (not drag) — use hovered window rect (global logical coords)
      const currentHovered = hoveredRectRef.current;
      if (currentHovered) {
        // Convert global logical rect to local image pixel coords
        const imgX = Math.round((currentHovered.x - monLogicalX) * dpr);
        const imgY = Math.round((currentHovered.y - monLogicalY) * dpr);
        const imgW = Math.round(currentHovered.width * dpr);
        const imgH = Math.round(currentHovered.height * dpr);

        appLog.info(
          `[Overlay] 窗口点击选中: monitor=${currentMonitorIndex}, global_logical=(${currentHovered.x},${currentHovered.y},${currentHovered.width}x${currentHovered.height}), image=(${imgX},${imgY},${imgW}x${imgH}), mode=${currentMode}`
        );

        await emit("region-selected", {
          x: imgX,
          y: imgY,
          width: imgW,
          height: imgH,
          mode: currentMode,
          monitor_index: currentMonitorIndex,
        });

        await emit("close-all-overlays");
        getCurrentWindow().close();
      } else {
        appLog.warn("[Overlay] 点击位置无窗口，已忽略");
      }
      selectionRef.current = null;
      mouseDownPosRef.current = null;
      setSelRect(null);
      isDraggingRef.current = false;
      return;
    }

    // Drag selection (local CSS coordinates)
    const x = Math.min(selection.startX, selection.endX);
    const y = Math.min(selection.startY, selection.endY);
    const width = Math.abs(selection.endX - selection.startX);
    const height = Math.abs(selection.endY - selection.startY);

    if (width < 5 || height < 5) {
      appLog.warn(
        "[Overlay] 选区太小 (" + width + "x" + height + ")，已忽略"
      );
      selectionRef.current = null;
      mouseDownPosRef.current = null;
      setSelRect(null);
      isDraggingRef.current = false;
      return;
    }

    // Convert local CSS coords to image pixel coords (per-monitor native resolution)
    const imgX = Math.round(x * dpr);
    const imgY = Math.round(y * dpr);
    const imgW = Math.round(width * dpr);
    const imgH = Math.round(height * dpr);

    appLog.info(
      `[Overlay] 选区完成: monitor=${currentMonitorIndex}, local_css=(${x},${y},${width}x${height}), image=(${imgX},${imgY},${imgW}x${imgH}), dpr=${dpr}, mode=${currentMode}`
    );

    await emit("region-selected", {
      x: imgX,
      y: imgY,
      width: imgW,
      height: imgH,
      mode: currentMode,
      monitor_index: currentMonitorIndex,
    });

    await emit("close-all-overlays");
    getCurrentWindow().close();
  }, []);

  return (
    <div
      className="fixed inset-0 cursor-crosshair select-none"
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      style={{
        backgroundImage: backgroundUrl ? `url(${backgroundUrl})` : undefined,
        backgroundSize: "cover",
        backgroundPosition: "center",
      }}
    >
      {/* Dark overlay */}
      <div className="absolute inset-0 bg-black/30" />

      {/* Window hover highlight */}
      {hoveredRect && !selRect && (() => {
        const local = toLocalRect(hoveredRect);
        if (!local) return null;
        return (
          <div
            className="absolute pointer-events-none"
            style={{
              left: local.x,
              top: local.y,
              width: local.width,
              height: local.height,
              border: "2px solid #22c55e",
              background: "rgba(34, 197, 94, 0.08)",
              boxShadow: "0 0 0 9999px rgba(0, 0, 0, 0.3)",
              zIndex: 10,
            }}
          />
        );
      })()}

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
          {/* Size indicator — show image pixel dimensions */}
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
      {!selRect && !hoveredRect && (
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-white text-lg font-medium pointer-events-none">
          Drag to select region · ESC to cancel
        </div>
      )}
    </div>
  );
}
