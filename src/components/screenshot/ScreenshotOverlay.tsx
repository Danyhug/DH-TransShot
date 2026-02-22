import { useState, useCallback, useEffect, useRef } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getFrozenScreenshot } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";
import type { WindowRect } from "../../types";

interface Selection {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
}

export function ScreenshotOverlay() {
  const [backgroundImage, setBackgroundImage] = useState<string>("");
  const [mode, setMode] = useState<string>("region");
  const [windowRects, setWindowRects] = useState<WindowRect[]>([]);
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
    // Pull frozen screenshot data from backend on mount
    appLog.info("[Overlay] 截图覆盖层已挂载，获取冻结截图...");
    getFrozenScreenshot()
      .then((data) => {
        appLog.info(
          "[Overlay] 冻结截图获取成功, mode=" +
            data.mode +
            ", image size=" +
            data.image.length +
            ", window_rects=" +
            (data.window_rects?.length ?? 0)
        );
        setBackgroundImage(data.image);
        setMode(data.mode);
        setWindowRects(data.window_rects ?? []);
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

  // Find the topmost window rect that contains the given point
  const findWindowAtPoint = useCallback(
    (x: number, y: number): WindowRect | null => {
      for (const rect of windowRectsRef.current) {
        if (
          x >= rect.x &&
          x <= rect.x + rect.width &&
          y >= rect.y &&
          y <= rect.y + rect.height
        ) {
          return rect;
        }
      }
      return null;
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

    if (!downPos || !selection) {
      isDraggingRef.current = false;
      return;
    }

    const currentMode = modeRef.current;
    const dx = selection.endX - downPos.x;
    const dy = selection.endY - downPos.y;
    const distance = Math.sqrt(dx * dx + dy * dy);

    if (distance < 5) {
      // Click (not drag) — use hovered window rect
      const currentHovered = hoveredRectRef.current;
      if (currentHovered) {
        const dpr = window.devicePixelRatio || 1;
        appLog.info(
          `[Overlay] 窗口点击选中: logical=(${currentHovered.x},${currentHovered.y},${currentHovered.width}x${currentHovered.height}), dpr=${dpr}, mode=${currentMode}`
        );

        await emit("region-selected", {
          x: Math.round(currentHovered.x * dpr),
          y: Math.round(currentHovered.y * dpr),
          width: Math.round(currentHovered.width * dpr),
          height: Math.round(currentHovered.height * dpr),
          mode: currentMode,
        });

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

    // Drag selection
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

    // Scale by devicePixelRatio for physical pixels
    const dpr = window.devicePixelRatio || 1;
    appLog.info(
      `[Overlay] 选区完成: logical=(${x},${y},${width}x${height}), dpr=${dpr}, physical=(${Math.round(x * dpr)},${Math.round(y * dpr)},${Math.round(width * dpr)}x${Math.round(height * dpr)}), mode=${currentMode}`
    );

    await emit("region-selected", {
      x: Math.round(x * dpr),
      y: Math.round(y * dpr),
      width: Math.round(width * dpr),
      height: Math.round(height * dpr),
      mode: currentMode,
    });

    getCurrentWindow().close();
  }, []);

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

      {/* Window hover highlight */}
      {hoveredRect && !selRect && (
        <div
          className="absolute pointer-events-none"
          style={{
            left: hoveredRect.x,
            top: hoveredRect.y,
            width: hoveredRect.width,
            height: hoveredRect.height,
            border: "2px solid #22c55e",
            background: "rgba(34, 197, 94, 0.08)",
            boxShadow: "0 0 0 9999px rgba(0, 0, 0, 0.3)",
            zIndex: 10,
          }}
        />
      )}

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
      {!selRect && !hoveredRect && (
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-white text-lg font-medium pointer-events-none">
          Drag to select region · ESC to cancel
        </div>
      )}
    </div>
  );
}
