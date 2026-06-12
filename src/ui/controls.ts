/** Callbacks the control panel reports user input through. */
export interface ControlsCallbacks {
    onLevelChange: (level: number) => void;
}

/** Imperative handle for pushing live readouts back into the panel. */
export interface Controls {
    readonly level: number;
    setCellCount: (count: number) => void;
    setFps: (fps: number) => void;
}

const MIN_LEVEL = 2;
const MAX_LEVEL = 8;

/**
 * Minimal framework-free overlay: a subdivision-level slider plus live FPS and
 * cell-count readouts. The slider reports through `onLevelChange`; the host
 * decides when to rebuild.
 */
export const createControls = (
    parent: HTMLElement,
    initialLevel: number,
    callbacks: ControlsCallbacks,
): Controls => {
    let level = clampLevel(initialLevel);

    const panel = document.createElement("div");
    Object.assign(panel.style, {
        position: "fixed",
        top: "12px",
        left: "12px",
        padding: "12px 14px",
        background: "rgba(10, 14, 22, 0.78)",
        border: "1px solid rgba(120, 140, 180, 0.25)",
        borderRadius: "10px",
        color: "#e6ecf5",
        font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
        backdropFilter: "blur(6px)",
        userSelect: "none",
        minWidth: "200px",
    } satisfies Partial<CSSStyleDeclaration>);

    const title = document.createElement("div");
    title.textContent = "Tectonic World-Builder";
    Object.assign(title.style, {
        fontWeight: "600",
        marginBottom: "10px",
        letterSpacing: "0.02em",
    } satisfies Partial<CSSStyleDeclaration>);

    const levelLabel = document.createElement("label");
    Object.assign(levelLabel.style, {
        display: "block",
        marginBottom: "4px",
        opacity: "0.85",
    } satisfies Partial<CSSStyleDeclaration>);

    const slider = document.createElement("input");
    slider.type = "range";
    slider.min = String(MIN_LEVEL);
    slider.max = String(MAX_LEVEL);
    slider.step = "1";
    slider.value = String(level);
    Object.assign(slider.style, {
        width: "100%",
        marginBottom: "10px",
        accentColor: "#4f8ff0",
    } satisfies Partial<CSSStyleDeclaration>);

    const readout = document.createElement("div");
    Object.assign(readout.style, {
        display: "flex",
        justifyContent: "space-between",
        gap: "12px",
        fontVariantNumeric: "tabular-nums",
        opacity: "0.95",
    } satisfies Partial<CSSStyleDeclaration>);

    const fpsEl = document.createElement("span");
    const cellEl = document.createElement("span");
    readout.append(fpsEl, cellEl);

    const renderLevelLabel = (): void => {
        levelLabel.textContent = `Subdivision level: ${level}`;
    };
    renderLevelLabel();
    fpsEl.textContent = "FPS: --";
    cellEl.textContent = "Cells: --";

    slider.addEventListener("input", () => {
        level = clampLevel(Number(slider.value));
        renderLevelLabel();
        callbacks.onLevelChange(level);
    });

    panel.append(title, levelLabel, slider, readout);
    parent.appendChild(panel);

    return {
        get level() {
            return level;
        },
        setCellCount: (count: number) => {
            cellEl.textContent = `Cells: ${count.toLocaleString()}`;
        },
        setFps: (fps: number) => {
            fpsEl.textContent = `FPS: ${fps.toFixed(0)}`;
        },
    };
};

const clampLevel = (value: number): number =>
    Math.max(MIN_LEVEL, Math.min(MAX_LEVEL, Math.round(value)));
