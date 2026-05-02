import { useState, useCallback, useEffect, useRef } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
import { getFrozenScreenshot } from "../../lib/invoke";
import { appLog } from "../../stores/logStore";
import type { WindowRect, MonitorInfo } from "../../types";

// --- Annotation types ---

interface Point {
  x: number;
  y: number;
}

type Shape =
  | { type: "rect"; x: number; y: number; w: number; h: number; color: string; strokeWidth: number }
  | { type: "arrow"; x1: number; y1: number; x2: number; y2: number; color: string; strokeWidth: number }
  | { type: "pen"; points: Point[]; color: string; strokeWidth: number }
  | { type: "text"; x: number; y: number; text: string; color: string; fontSize: number };

type Tool = "rect" | "arrow" | "pen" | "text";

const PRESET_COLORS = ["#ef4444", "#3b82f6", "#22c55e", "#eab308", "#ffffff"];
const DEFAULT_STROKE_WIDTH = 3;
const ANNOTATION_TOOLBAR_HEIGHT = 52;
const ANNOTATION_TOOLBAR_MIN_WIDTH = 360;
const ANNOTATION_PICKER_HEIGHT = 188;
const COLOR_TOOLTIP_OFFSET = 14;

// --- Selection types ---

interface Selection {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
}

export function ScreenshotOverlay() {
  // --- Phase ---
  const [phase, setPhase] = useState<"select" | "annotate">("select");

  // --- Select phase state ---
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
  const [hoverColor, setHoverColor] = useState<{ x: number; y: number; hex: string; copied: boolean } | null>(null);

  // --- Annotate phase state ---
  const [croppedImageEl, setCroppedImageEl] = useState<HTMLImageElement | null>(null);
  const croppedBlobUrlRef = useRef<string | null>(null);
  const [tool, setTool] = useState<Tool>("rect");
  const [color, setColor] = useState("#ef4444");
  const [strokeWidth, setStrokeWidth] = useState(DEFAULT_STROKE_WIDTH);
  const [showStylePicker, setShowStylePicker] = useState(false);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [currentShape, setCurrentShape] = useState<Shape | null>(null);
  const [textInput, setTextInput] = useState<{
    x: number;
    y: number;
    value: string;
  } | null>(null);
  const [canvasDisplaySize, setCanvasDisplaySize] = useState<{ width: number; height: number } | null>(null);

  // --- Refs for select phase ---
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
  const screenshotCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const screenshotCanvasCtxRef = useRef<CanvasRenderingContext2D | null>(null);
  const hoverColorRef = useRef<typeof hoverColor>(null);
  const screenshotImageSizeRef = useRef<{ width: number; height: number } | null>(null);

  // --- Refs for annotate phase ---
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const shapesRef = useRef<Shape[]>([]);
  const toolRef = useRef<Tool>("rect");
  const colorRef = useRef("#ef4444");
  const strokeWidthRef = useRef(DEFAULT_STROKE_WIDTH);
  const drawingRef = useRef(false);
  const penPointsRef = useRef<Point[]>([]);
  const croppedImageElRef = useRef<HTMLImageElement | null>(null);
  const textInputRef = useRef<typeof textInput>(null);
  const annotateSourceRectRef = useRef<{ left: number; top: number; width: number; height: number } | null>(null);

  // Keep refs in sync
  useEffect(() => { windowRectsRef.current = windowRects; }, [windowRects]);
  useEffect(() => { hoveredRectRef.current = hoveredRect; }, [hoveredRect]);
  useEffect(() => { modeRef.current = mode; }, [mode]);
  useEffect(() => { monitorRef.current = monitor; }, [monitor]);
  useEffect(() => { monitorIndexRef.current = monitorIndex; }, [monitorIndex]);
  useEffect(() => { hoverColorRef.current = hoverColor; }, [hoverColor]);
  useEffect(() => { shapesRef.current = shapes; }, [shapes]);
  useEffect(() => { toolRef.current = tool; }, [tool]);
  useEffect(() => { colorRef.current = color; }, [color]);
  useEffect(() => { strokeWidthRef.current = strokeWidth; }, [strokeWidth]);
  useEffect(() => { croppedImageElRef.current = croppedImageEl; }, [croppedImageEl]);
  useEffect(() => { textInputRef.current = textInput; }, [textInput]);

  // --- Load frozen screenshot on mount ---
  useEffect(() => {
    appLog.info("[Overlay] 截图覆盖层已挂载，获取冻结截图...");

    let cancelled = false;

    const loadScreenshot = async () => {
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
            "[Overlay] 冻结截图获取成功, attempt=" + attempt +
            ", monitor_index=" + myMonitorIndex +
            ", mode=" + data.mode +
            ", image size=" + data.image.length +
            ", window_rects=" + (data.window_rects?.length ?? 0) +
            ", monitor=" + (myMonitor?.name ?? "unknown")
          );

          const binaryStr = atob(data.image);
          const bytes = new Uint8Array(binaryStr.length);
          for (let i = 0; i < binaryStr.length; i++) {
            bytes[i] = binaryStr.charCodeAt(i);
          }
          const blob = new Blob([bytes], { type: "image/png" });
          const url = URL.createObjectURL(blob);

          const imgEl = await new Promise<HTMLImageElement>((resolve, reject) => {
            const img = new Image();
            img.onload = () => resolve(img);
            img.onerror = () => reject(new Error("Image decode failed"));
            img.src = url;
          });

          const sampleCanvas = document.createElement("canvas");
          sampleCanvas.width = imgEl.naturalWidth;
          sampleCanvas.height = imgEl.naturalHeight;
          const sampleCtx = sampleCanvas.getContext("2d", { willReadFrequently: true });
          if (sampleCtx) {
            sampleCtx.drawImage(imgEl, 0, 0);
            screenshotCanvasRef.current = sampleCanvas;
            screenshotCanvasCtxRef.current = sampleCtx;
            screenshotImageSizeRef.current = { width: imgEl.naturalWidth, height: imgEl.naturalHeight };
          } else {
            appLog.warn("[Overlay] 取色 canvas context 创建失败");
          }

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

          const currentWin = getCurrentWindow();
          await currentWin.show();
          await currentWin.setFocus();
          appLog.info("[Overlay] 覆盖层窗口已显示, label=" + label);
          return;
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
      if (textInputRef.current) return;

      if (e.key === "Escape") {
        emit("close-all-overlays");
      }
    };
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      cancelled = true;
      window.removeEventListener("keydown", handleKeyDown);
      if (blobUrlRef.current) {
        URL.revokeObjectURL(blobUrlRef.current);
      }
      if (croppedBlobUrlRef.current) {
        URL.revokeObjectURL(croppedBlobUrlRef.current);
      }
      screenshotCanvasRef.current = null;
      screenshotCanvasCtxRef.current = null;
      screenshotImageSizeRef.current = null;
    };
  }, []);

  // --- Coordinate helpers ---

  const findWindowAtPoint = useCallback(
    (localX: number, localY: number): WindowRect | null => {
      const mon = monitorRef.current;
      if (!mon) return null;
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

  const getMonitorScale = useCallback(() => {
    return monitorRef.current?.scale_factor || window.devicePixelRatio || 1;
  }, []);

  const getImageScale = useCallback(() => {
    const imageSize = screenshotImageSizeRef.current;
    if (!imageSize || window.innerWidth <= 0 || window.innerHeight <= 0) {
      const fallback = getMonitorScale();
      return { scaleX: fallback, scaleY: fallback };
    }

    return {
      scaleX: imageSize.width / window.innerWidth,
      scaleY: imageSize.height / window.innerHeight,
    };
  }, [getMonitorScale]);

  const getColorAtPoint = useCallback((localX: number, localY: number): string | null => {
    const canvas = screenshotCanvasRef.current;
    const ctx = screenshotCanvasCtxRef.current;
    if (!canvas || !ctx) return null;

    const { scaleX, scaleY } = getImageScale();
    const imageX = Math.min(canvas.width - 1, Math.max(0, Math.round(localX * scaleX)));
    const imageY = Math.min(canvas.height - 1, Math.max(0, Math.round(localY * scaleY)));

    const [r, g, b] = ctx.getImageData(imageX, imageY, 1, 1).data;
    return "#" + [r, g, b].map((value) => value.toString(16).padStart(2, "0")).join("").toUpperCase();
  }, [getImageScale]);

  const updateHoverColor = useCallback((sampleX: number, sampleY: number, displayX = sampleX, displayY = sampleY) => {
    const hex = getColorAtPoint(sampleX, sampleY);
    if (!hex) return;
    setHoverColor({ x: displayX, y: displayY, hex, copied: false });
  }, [getColorAtPoint]);

  const copyHoverColor = useCallback(async () => {
    const currentHoverColor = hoverColorRef.current;
    if (!currentHoverColor) return;

    try {
      await navigator.clipboard.writeText(currentHoverColor.hex);
      appLog.info("[Overlay] 已复制取色值: " + currentHoverColor.hex);
      setHoverColor({ ...currentHoverColor, copied: true });
    } catch (e) {
      appLog.error("[Overlay] 复制取色值失败: " + String(e));
    }
  }, []);

  useEffect(() => {
    const handleColorCopyKey = (e: KeyboardEvent) => {
      if (textInputRef.current) return;
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.key.toLowerCase() !== "c" || !hoverColorRef.current) return;

      e.preventDefault();
      copyHoverColor();
    };

    window.addEventListener("keydown", handleColorCopyKey);
    return () => window.removeEventListener("keydown", handleColorCopyKey);
  }, [copyHoverColor]);

  // --- Crop the selected region from the frozen screenshot ---
  const cropRegion = useCallback(async (sel: { left: number; top: number; width: number; height: number }) => {
    const { scaleX, scaleY } = getImageScale();
    const srcX = Math.round(sel.left * scaleX);
    const srcY = Math.round(sel.top * scaleY);
    const srcW = Math.round(sel.width * scaleX);
    const srcH = Math.round(sel.height * scaleY);

    const img = new Image();
    img.src = backgroundUrl;
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error("Failed to load background image"));
    });

    const canvas = document.createElement("canvas");
    canvas.width = srcW;
    canvas.height = srcH;
    const ctx = canvas.getContext("2d")!;
    ctx.drawImage(img, srcX, srcY, srcW, srcH, 0, 0, srcW, srcH);

    const blob = await new Promise<Blob>((resolve) =>
      canvas.toBlob((b) => resolve(b!), "image/png")
    );
    const url = URL.createObjectURL(blob);
    return url;
  }, [backgroundUrl, getImageScale]);

  const toImageRect = useCallback((rect: { left: number; top: number; width: number; height: number }) => {
    const { scaleX, scaleY } = getImageScale();
    return {
      x: Math.round(rect.left * scaleX),
      y: Math.round(rect.top * scaleY),
      width: Math.max(1, Math.round(rect.width * scaleX)),
      height: Math.max(1, Math.round(rect.height * scaleY)),
      scaleX,
      scaleY,
    };
  }, [getImageScale]);

  // --- Enter annotate phase ---
  const enterAnnotate = useCallback(async (sel: { left: number; top: number; width: number; height: number }) => {
    try {
      const url = await cropRegion(sel);
      annotateSourceRectRef.current = sel;
      const img = new Image();
      img.src = url;
      await new Promise<void>((resolve, reject) => {
        img.onload = () => resolve();
        img.onerror = () => reject(new Error("Failed to load cropped image"));
      });
      croppedBlobUrlRef.current = url;
      setCroppedImageEl(img);
      setCanvasDisplaySize({ width: sel.width, height: sel.height });
      const mon = monitorRef.current;
      if (mon) {
        const currentWindow = getCurrentWindow();
        const monitorLogicalX = mon.x / mon.scale_factor;
        const monitorLogicalY = mon.y / mon.scale_factor;
        const windowWidth = Math.max(sel.width, ANNOTATION_TOOLBAR_MIN_WIDTH);
        const windowTop = Math.max(monitorLogicalY, monitorLogicalY + sel.top - ANNOTATION_PICKER_HEIGHT);
        const monitorLogicalWidth = mon.width / mon.scale_factor;
        const maxWindowLeft = monitorLogicalX + Math.max(0, monitorLogicalWidth - windowWidth);
        const desiredWindowLeft = monitorLogicalX + sel.left;
        const windowLeft = Math.min(Math.max(desiredWindowLeft, monitorLogicalX), maxWindowLeft);
        const contentTop = monitorLogicalY + sel.top - windowTop;
        const contentLeft = desiredWindowLeft - windowLeft;
        await currentWindow.setPosition(new LogicalPosition(windowLeft, windowTop));
        await currentWindow.setSize(new LogicalSize(windowWidth, contentTop + sel.height + ANNOTATION_TOOLBAR_HEIGHT));
        setSelRect({ left: contentLeft, top: contentTop, width: sel.width, height: sel.height });
      }
      setPhase("annotate");
      setShapes([]);
      setCurrentShape(null);
      appLog.info("[Overlay] 进入标注模式, cropped size=" + img.naturalWidth + "x" + img.naturalHeight);
    } catch (e) {
      appLog.error("[Overlay] 裁切图片失败: " + String(e));
    }
  }, [cropRegion]);

  // --- Canvas rendering ---
  const renderCanvas = useCallback(() => {
    const canvas = canvasRef.current;
    const img = croppedImageElRef.current;
    if (!canvas || !img) return;

    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const ctx = canvas.getContext("2d")!;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(img, 0, 0);

    const allShapes = [...shapesRef.current];
    if (currentShape) allShapes.push(currentShape);

    for (const shape of allShapes) {
      ctx.save();
      switch (shape.type) {
        case "rect": {
          ctx.strokeStyle = shape.color;
          ctx.lineWidth = shape.strokeWidth;
          ctx.strokeRect(shape.x, shape.y, shape.w, shape.h);
          break;
        }
        case "arrow": {
          const { x1, y1, x2, y2, color: c, strokeWidth: sw } = shape;
          ctx.strokeStyle = c;
          ctx.fillStyle = c;
          ctx.lineWidth = sw;
          ctx.lineCap = "round";
          // Line
          ctx.beginPath();
          ctx.moveTo(x1, y1);
          ctx.lineTo(x2, y2);
          ctx.stroke();
          // Arrowhead
          const angle = Math.atan2(y2 - y1, x2 - x1);
          const headLen = Math.max(sw * 5, 14);
          ctx.beginPath();
          ctx.moveTo(x2, y2);
          ctx.lineTo(
            x2 - headLen * Math.cos(angle - Math.PI / 6),
            y2 - headLen * Math.sin(angle - Math.PI / 6)
          );
          ctx.lineTo(
            x2 - headLen * Math.cos(angle + Math.PI / 6),
            y2 - headLen * Math.sin(angle + Math.PI / 6)
          );
          ctx.closePath();
          ctx.fill();
          break;
        }
        case "pen": {
          if (shape.points.length < 2) break;
          ctx.strokeStyle = shape.color;
          ctx.lineWidth = shape.strokeWidth;
          ctx.lineCap = "round";
          ctx.lineJoin = "round";
          ctx.beginPath();
          ctx.moveTo(shape.points[0].x, shape.points[0].y);
          for (let i = 1; i < shape.points.length; i++) {
            ctx.lineTo(shape.points[i].x, shape.points[i].y);
          }
          ctx.stroke();
          break;
        }
        case "text": {
          ctx.fillStyle = shape.color;
          ctx.font = `${shape.fontSize}px sans-serif`;
          ctx.textBaseline = "top";
          ctx.fillText(shape.text, shape.x, shape.y);
          break;
        }
      }
      ctx.restore();
    }
  }, [currentShape]);

  useEffect(() => {
    if (phase !== "annotate") return;
    renderCanvas();
  }, [phase, shapes, currentShape, renderCanvas]);

  // --- Confirm: render final image and emit ---
  const handleConfirm = useCallback(async () => {
    const img = croppedImageElRef.current;
    if (!img) return;

    const canvas = document.createElement("canvas");
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const ctx = canvas.getContext("2d")!;
    ctx.drawImage(img, 0, 0);

    for (const shape of shapesRef.current) {
      ctx.save();
      switch (shape.type) {
        case "rect": {
          ctx.strokeStyle = shape.color;
          ctx.lineWidth = shape.strokeWidth;
          ctx.strokeRect(shape.x, shape.y, shape.w, shape.h);
          break;
        }
        case "arrow": {
          const { x1, y1, x2, y2, color: c, strokeWidth: sw } = shape;
          ctx.strokeStyle = c;
          ctx.fillStyle = c;
          ctx.lineWidth = sw;
          ctx.lineCap = "round";
          ctx.beginPath();
          ctx.moveTo(x1, y1);
          ctx.lineTo(x2, y2);
          ctx.stroke();
          const angle = Math.atan2(y2 - y1, x2 - x1);
          const headLen = Math.max(sw * 5, 14);
          ctx.beginPath();
          ctx.moveTo(x2, y2);
          ctx.lineTo(x2 - headLen * Math.cos(angle - Math.PI / 6), y2 - headLen * Math.sin(angle - Math.PI / 6));
          ctx.lineTo(x2 - headLen * Math.cos(angle + Math.PI / 6), y2 - headLen * Math.sin(angle + Math.PI / 6));
          ctx.closePath();
          ctx.fill();
          break;
        }
        case "pen": {
          if (shape.points.length < 2) break;
          ctx.strokeStyle = shape.color;
          ctx.lineWidth = shape.strokeWidth;
          ctx.lineCap = "round";
          ctx.lineJoin = "round";
          ctx.beginPath();
          ctx.moveTo(shape.points[0].x, shape.points[0].y);
          for (let i = 1; i < shape.points.length; i++) {
            ctx.lineTo(shape.points[i].x, shape.points[i].y);
          }
          ctx.stroke();
          break;
        }
        case "text": {
          ctx.fillStyle = shape.color;
          ctx.font = `${shape.fontSize}px sans-serif`;
          ctx.textBaseline = "top";
          ctx.fillText(shape.text, shape.x, shape.y);
          break;
        }
      }
      ctx.restore();
    }

    const dataUrl = canvas.toDataURL("image/png");
    const base64 = dataUrl.split(",")[1];

    appLog.info("[Overlay] 标注确认, shapes=" + shapesRef.current.length + ", base64 size=" + base64.length);

    const currentMonitorIndex = monitorIndexRef.current;
    const currentMode = modeRef.current;

    await emit("region-selected", {
      x: 0,
      y: 0,
      width: canvas.width,
      height: canvas.height,
      mode: currentMode,
      monitor_index: currentMonitorIndex,
      annotatedImage: base64,
    });

    await emit("close-all-overlays");
  }, []);

  // --- Annotate phase: keyboard shortcuts ---
  useEffect(() => {
    if (phase !== "annotate") return;

    const handleKey = (e: KeyboardEvent) => {
      if (textInput) return;

      if (e.key === "Escape") {
        emit("close-all-overlays");
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "z") {
        e.preventDefault();
        setShapes((prev) => prev.slice(0, -1));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        handleConfirm();
        return;
      }
      if (!e.ctrlKey && !e.metaKey && !e.altKey) {
        if (e.key === "1") setTool("rect");
        if (e.key === "2") setTool("arrow");
        if (e.key === "3") setTool("pen");
        if (e.key === "4") setTool("text");
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [phase, textInput, handleConfirm]);

  const commitTextInput = useCallback((fontSize: number) => {
    const pendingTextInput = textInputRef.current;
    if (!pendingTextInput) return;

    textInputRef.current = null;
    setTextInput(null);

    const value = pendingTextInput.value.trim();
    if (!value) return;

    setShapes((prev) => [
      ...prev,
      {
        type: "text",
        x: pendingTextInput.x,
        y: pendingTextInput.y,
        text: value,
        color: colorRef.current,
        fontSize,
      },
    ]);
  }, []);

  // --- Select phase mouse handlers ---

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
      updateHoverColor(e.clientX, e.clientY);

      if (!isSelectingRef.current) {
        const rect = findWindowAtPoint(e.clientX, e.clientY);
        setHoveredRect(rect);
        return;
      }

      if (!selectionRef.current) return;

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
    [findWindowAtPoint, updateHoverColor]
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
    const dx = selection.endX - downPos.x;
    const dy = selection.endY - downPos.y;
    const distance = Math.sqrt(dx * dx + dy * dy);

    const monLogicalX = mon.x / mon.scale_factor;
    const monLogicalY = mon.y / mon.scale_factor;

    if (distance < 5) {
      const currentHovered = hoveredRectRef.current;
      selectionRef.current = null;
      mouseDownPosRef.current = null;
      isDraggingRef.current = false;

      if (currentHovered) {
        if (currentMode === "screenshot") {
          // Window click for screenshot: set selRect for annotation positioning, then enter annotate
          const local = toLocalRect(currentHovered);
          if (local) {
            setSelRect({ left: local.x, top: local.y, width: local.width, height: local.height });
            await enterAnnotate({ left: local.x, top: local.y, width: local.width, height: local.height });
          }
        } else {
          // ocr_translate: emit directly
          const imgRect = toImageRect({
            left: currentHovered.x - monLogicalX,
            top: currentHovered.y - monLogicalY,
            width: currentHovered.width,
            height: currentHovered.height,
          });

          appLog.info(
            `[Overlay] 窗口点击选中: monitor=${currentMonitorIndex}, global_logical=(${currentHovered.x},${currentHovered.y},${currentHovered.width}x${currentHovered.height}), image=(${imgRect.x},${imgRect.y},${imgRect.width}x${imgRect.height}), scale=(${imgRect.scaleX.toFixed(3)},${imgRect.scaleY.toFixed(3)}), mode=${currentMode}`
          );

          await emit("region-selected", {
            x: imgRect.x, y: imgRect.y, width: imgRect.width, height: imgRect.height,
            mode: currentMode, monitor_index: currentMonitorIndex,
          });
          await emit("close-all-overlays");
          setSelRect(null);
        }
      } else {
        appLog.warn("[Overlay] 点击位置无窗口，已忽略");
        setSelRect(null);
      }
      return;
    }

    // Drag selection
    const x = Math.min(selection.startX, selection.endX);
    const y = Math.min(selection.startY, selection.endY);
    const width = Math.abs(selection.endX - selection.startX);
    const height = Math.abs(selection.endY - selection.startY);

    if (width < 5 || height < 5) {
      appLog.warn("[Overlay] 选区太小 (" + width + "x" + height + ")，已忽略");
      selectionRef.current = null;
      mouseDownPosRef.current = null;
      setSelRect(null);
      isDraggingRef.current = false;
      return;
    }

    if (currentMode === "screenshot") {
      // Enter annotation mode
      appLog.info(
        `[Overlay] 选区完成(screenshot), local_css=(${x},${y},${width}x${height}), 进入标注模式`
      );
      selectionRef.current = null;
      mouseDownPosRef.current = null;
      isDraggingRef.current = false;
      await enterAnnotate({ left: x, top: y, width, height });
    } else {
      // ocr_translate: emit directly
      const imgRect = toImageRect({ left: x, top: y, width, height });

      appLog.info(
        `[Overlay] 选区完成: monitor=${currentMonitorIndex}, local_css=(${x},${y},${width}x${height}), image=(${imgRect.x},${imgRect.y},${imgRect.width}x${imgRect.height}), scale=(${imgRect.scaleX.toFixed(3)},${imgRect.scaleY.toFixed(3)}), mode=${currentMode}`
      );

      await emit("region-selected", {
        x: imgRect.x, y: imgRect.y, width: imgRect.width, height: imgRect.height,
        mode: currentMode, monitor_index: currentMonitorIndex,
      });
      await emit("close-all-overlays");
      selectionRef.current = null;
      mouseDownPosRef.current = null;
      setSelRect(null);
      isDraggingRef.current = false;
    }
  }, [toLocalRect, enterAnnotate, toImageRect]);

  // --- Annotate phase: canvas mouse handlers ---

  const canvasToImageCoords = useCallback((e: React.MouseEvent, canvas: HTMLCanvasElement): Point => {
    const rect = canvas.getBoundingClientRect();
    const scaleX = canvas.width / rect.width;
    const scaleY = canvas.height / rect.height;
    return {
      x: (e.clientX - rect.left) * scaleX,
      y: (e.clientY - rect.top) * scaleY,
    };
  }, []);

  const handleCanvasMouseDown = useCallback((e: React.MouseEvent) => {
    if (textInput) return; // Don't draw while text input is open

    const canvas = canvasRef.current;
    if (!canvas) return;
    const pos = canvasToImageCoords(e, canvas);
    const sourceRect = annotateSourceRectRef.current;
    if (sourceRect) {
      const { scaleX, scaleY } = getImageScale();
      updateHoverColor(sourceRect.left + pos.x / scaleX, sourceRect.top + pos.y / scaleY, e.clientX, e.clientY);
    }

    if (toolRef.current === "text") {
      setTextInput({ x: pos.x, y: pos.y, value: "" });
      return;
    }

    drawingRef.current = true;

    if (toolRef.current === "pen") {
      penPointsRef.current = [pos];
      setCurrentShape({
        type: "pen",
        points: [pos],
        color: colorRef.current,
        strokeWidth: strokeWidthRef.current,
      });
    } else if (toolRef.current === "rect") {
      setCurrentShape({
        type: "rect",
        x: pos.x, y: pos.y, w: 0, h: 0,
        color: colorRef.current,
        strokeWidth: strokeWidthRef.current,
      });
      penPointsRef.current = [pos]; // Store start point
    } else if (toolRef.current === "arrow") {
      setCurrentShape({
        type: "arrow",
        x1: pos.x, y1: pos.y, x2: pos.x, y2: pos.y,
        color: colorRef.current,
        strokeWidth: strokeWidthRef.current,
      });
      penPointsRef.current = [pos];
    }
  }, [textInput, canvasToImageCoords, updateHoverColor, getImageScale]);

  const handleCanvasMouseMove = useCallback((e: React.MouseEvent) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const pos = canvasToImageCoords(e, canvas);
    const sourceRect = annotateSourceRectRef.current;
    if (sourceRect) {
      const { scaleX, scaleY } = getImageScale();
      updateHoverColor(sourceRect.left + pos.x / scaleX, sourceRect.top + pos.y / scaleY, e.clientX, e.clientY);
    }

    if (!drawingRef.current) return;

    if (toolRef.current === "pen") {
      penPointsRef.current.push(pos);
      setCurrentShape({
        type: "pen",
        points: [...penPointsRef.current],
        color: colorRef.current,
        strokeWidth: strokeWidthRef.current,
      });
    } else if (toolRef.current === "rect") {
      const start = penPointsRef.current[0];
      setCurrentShape({
        type: "rect",
        x: Math.min(start.x, pos.x),
        y: Math.min(start.y, pos.y),
        w: Math.abs(pos.x - start.x),
        h: Math.abs(pos.y - start.y),
        color: colorRef.current,
        strokeWidth: strokeWidthRef.current,
      });
    } else if (toolRef.current === "arrow") {
      const start = penPointsRef.current[0];
      setCurrentShape({
        type: "arrow",
        x1: start.x, y1: start.y, x2: pos.x, y2: pos.y,
        color: colorRef.current,
        strokeWidth: strokeWidthRef.current,
      });
    }
  }, [canvasToImageCoords, updateHoverColor, getImageScale]);

  const handleCanvasMouseUp = useCallback(() => {
    if (!drawingRef.current) return;
    drawingRef.current = false;

    if (currentShape) {
      setShapes((prev) => [...prev, currentShape]);
      setCurrentShape(null);
    }
  }, [currentShape]);

  const renderColorTooltip = () => {
    if (!hoverColor) return null;

    return (
      <div
        className="fixed pointer-events-none rounded-lg border border-white/15 bg-neutral-900/90 px-2 py-1 text-xs text-white shadow-lg"
        style={{
          left: Math.min(window.innerWidth - 132, Math.max(8, hoverColor.x + COLOR_TOOLTIP_OFFSET)),
          top: Math.min(window.innerHeight - 48, Math.max(8, hoverColor.y + COLOR_TOOLTIP_OFFSET)),
          zIndex: 80,
          backdropFilter: "blur(8px)",
        }}
      >
        <div className="flex items-center gap-2 whitespace-nowrap">
          <span
            className="h-4 w-4 rounded border border-white/30"
            style={{ background: hoverColor.hex }}
          />
          <span className="font-mono">{hoverColor.hex}</span>
          <span className="text-white/45">{hoverColor.copied ? "已复制" : "C 复制"}</span>
        </div>
      </div>
    );
  };

  // --- Render ---

  const displayScale = getImageScale();

  if (phase === "annotate" && selRect) {
    return (
      <div
        className="screenshot-overlay-root fixed inset-0 select-none"
      >
        {renderColorTooltip()}
        {/* Annotation group: canvas + text input + toolbar, anchored at selection top-left */}
        <div
          className="absolute flex flex-col items-start"
          style={{
            left: selRect.left,
            top: selRect.top,
          }}
        >
          {/* Transparent annotation layer over the original frozen background */}
          <div className="relative inline-block shrink-0 border border-white/30">
            <canvas
              ref={canvasRef}
              style={{
                cursor: tool === "text" ? "text" : "crosshair",
                width: canvasDisplaySize?.width ?? selRect.width,
                height: canvasDisplaySize?.height ?? selRect.height,
                display: "block",
              }}
              onMouseDown={handleCanvasMouseDown}
              onMouseMove={handleCanvasMouseMove}
              onMouseUp={handleCanvasMouseUp}
              onMouseLeave={handleCanvasMouseUp}
            />

            {/* Text input overlay — positioned in canvas pixel space, scaled by CSS */}
            {textInput && (() => {
              const canvas = canvasRef.current;
              if (!canvas) return null;
              const displayW = canvas.clientWidth;
              const displayH = canvas.clientHeight;
              const scaleX = displayW / canvas.width;
              const scaleY = displayH / canvas.height;
              const fontSize = Math.round((croppedImageEl?.naturalHeight ?? 600) / 20);
              return (
                <input
                  autoFocus
                  type="text"
                  value={textInput.value}
                  onChange={(e) => setTextInput({ ...textInput, value: e.target.value })}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      e.stopPropagation();
                      commitTextInput(fontSize);
                    } else if (e.key === "Escape") {
                      e.preventDefault();
                      e.stopPropagation();
                      textInputRef.current = null;
                      setTextInput(null);
                    }
                  }}
                  onBlur={() => commitTextInput(fontSize)}
                  className="absolute text-white bg-black/60 border border-white/40 rounded px-2 py-1 outline-none"
                  style={{
                    left: textInput.x * scaleX,
                    top: textInput.y * scaleY,
                    fontSize: `${fontSize * scaleX}px`,
                    minWidth: "120px",
                    zIndex: 60,
                  }}
                />
              );
            })()}
          </div>

          {/* Toolbar — directly below the canvas */}
          <div
            className="flex items-center gap-1.5 px-3 py-2 rounded-xl shadow-lg mt-1.5"
            style={{
              background: "rgba(30,30,30,0.9)",
              backdropFilter: "blur(8px)",
              minWidth: ANNOTATION_TOOLBAR_MIN_WIDTH,
            }}
          >
            {/* Tools */}
            {[
              { id: "rect" as Tool, label: "□", title: "矩形 (1)" },
              { id: "arrow" as Tool, label: "→", title: "箭头 (2)" },
              { id: "pen" as Tool, label: "✏", title: "画笔 (3)" },
              { id: "text" as Tool, label: "T", title: "文字 (4)" },
            ].map((t) => (
              <button
                key={t.id}
                title={t.title}
                onClick={() => setTool(t.id)}
                className="w-8 h-8 flex items-center justify-center rounded-md text-sm font-medium transition-colors"
                style={{
                  background: tool === t.id ? "rgba(255,255,255,0.2)" : "transparent",
                  color: tool === t.id ? "#fff" : "rgba(255,255,255,0.6)",
                }}
              >
                {t.label}
              </button>
            ))}

            {/* Divider */}
            <div className="w-px h-5 bg-white/20 mx-1" />

            {/* Color and stroke picker */}
            <div className="relative">
              <button
                title="颜色和线宽"
                onClick={() => setShowStylePicker((prev) => !prev)}
                className="w-8 h-8 flex items-center justify-center rounded-md hover:bg-white/10 transition-colors"
              >
                <span
                  className="w-5 h-5 rounded-full border-2 border-white/80 shadow-sm"
                  style={{ background: color }}
                />
              </button>

              {showStylePicker && (
                <div
                  className="absolute bottom-11 left-0 w-56 rounded-xl p-3 shadow-xl border border-white/10 z-70"
                  style={{
                    background: "rgba(30,30,30,0.96)",
                    backdropFilter: "blur(10px)",
                  }}
                  onMouseDown={(e) => e.stopPropagation()}
                >
                  <div className="text-white/60 text-xs mb-2">颜色</div>
                  <div className="flex items-center gap-2 mb-3">
                    {PRESET_COLORS.map((presetColor) => (
                      <button
                        key={presetColor}
                        title={presetColor}
                        onClick={() => setColor(presetColor)}
                        className="w-6 h-6 rounded-full border-2 transition-transform"
                        style={{
                          background: presetColor,
                          borderColor: color === presetColor ? "#fff" : "transparent",
                          transform: color === presetColor ? "scale(1.15)" : "scale(1)",
                        }}
                      />
                    ))}
                    <label
                      className="w-7 h-7 rounded-md border border-white/20 overflow-hidden cursor-pointer"
                      title="自定义颜色"
                      style={{ background: color }}
                    >
                      <input
                        type="color"
                        value={color}
                        onChange={(e) => setColor(e.target.value)}
                        className="w-10 h-10 opacity-0 cursor-pointer"
                      />
                    </label>
                  </div>

                  <div className="flex items-center justify-between text-white/60 text-xs mb-2">
                    <span>线宽</span>
                    <span>{strokeWidth}px</span>
                  </div>
                  <input
                    type="range"
                    min="1"
                    max="12"
                    step="1"
                    value={strokeWidth}
                    onChange={(e) => setStrokeWidth(Number(e.target.value))}
                    className="w-full accent-blue-500"
                  />
                  <div className="mt-3 h-8 rounded bg-white/5 flex items-center justify-center">
                    <div
                      className="rounded-full"
                      style={{
                        width: 120,
                        height: strokeWidth,
                        background: color,
                      }}
                    />
                  </div>
                </div>
              )}
            </div>

            {/* Divider */}
            <div className="w-px h-5 bg-white/20 mx-1" />

            {/* Confirm / Cancel */}
            <button
              title="确认 (Enter)"
              onClick={handleConfirm}
              className="w-8 h-8 flex items-center justify-center rounded-md text-green-400 hover:bg-green-400/20 text-lg"
            >
              ✓
            </button>
            <button
              title="取消 (Esc)"
              onClick={() => emit("close-all-overlays")}
              className="w-8 h-8 flex items-center justify-center rounded-md text-red-400 hover:bg-red-400/20 text-lg"
            >
              ✗
            </button>

            {/* Undo hint */}
            <span className="text-white/40 text-xs ml-1 select-none">Ctrl+Z</span>
          </div>
        </div>
      </div>
    );
  }

  // --- Select phase ---
  return (
    <div
      className="screenshot-overlay-root fixed inset-0 cursor-crosshair select-none"
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
          <div
            className="absolute text-xs text-white bg-blue-500 px-2 py-0.5 rounded"
            style={{
              left: selRect.left,
              top: selRect.top - 24,
            }}
          >
            {Math.round(selRect.width * displayScale.scaleX)} x{" "}
            {Math.round(selRect.height * displayScale.scaleY)}
          </div>
        </>
      )}

      {/* Instructions */}
      {!selRect && !hoveredRect && (
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-white text-lg font-medium pointer-events-none">
          Drag to select region · ESC to cancel
        </div>
      )}

      {renderColorTooltip()}
    </div>
  );
}
