/**
 * How cells render: elevation colour ("colour"), greyscale with per-cell
 * id/elevation labels ("data"), solid per-plate colours ("plate"), crust type
 * ("crust"), or density ramp ("density").
 */
export type ViewMode = "colour" | "data" | "plate" | "crust" | "density";

/** Callbacks the control panel reports user input through. */
export interface ControlsCallbacks {
    onLevelChange: (level: number) => void;
    onSeaLevelChange: (seaLevel: number) => void;
    onAutoRotateChange: (enabled: boolean) => void;
    onViewModeChange: (mode: ViewMode) => void;
    onSeedModeChange: (enabled: boolean) => void;
    onBranchesChange: (branches: number) => void;
    onVelocitiesChange: (enabled: boolean) => void;
    onClearFaults: () => void;
}

/** Initial values the panel opens with. */
export interface ControlsOptions {
    level: number;
    seaLevel: number;
    autoRotate: boolean;
    viewMode: ViewMode;
    seedMode: boolean;
    branchesPerSeed: number;
    showVelocities: boolean;
}

/** Imperative handle for pushing live readouts back into the panel. */
export interface Controls {
    readonly level: number;
    readonly seaLevel: number;
    readonly autoRotate: boolean;
    readonly viewMode: ViewMode;
    readonly seedMode: boolean;
    readonly branchesPerSeed: number;
    readonly showVelocities: boolean;
    setCellCount: (count: number) => void;
    setPlateCount: (count: number) => void;
    setFps: (fps: number) => void;
    setNote: (text: string) => void;
}

const MIN_LEVEL = 2;
const MAX_LEVEL = 8;
const MIN_SEA_LEVEL = -8;
const MAX_SEA_LEVEL = 8;
const MIN_BRANCHES = 1;
const MAX_BRANCHES = 5;

/**
 * Minimal framework-free overlay: subdivision-level and sea-level sliders, an
 * auto-rotate toggle, plus live FPS and cell-count readouts. Inputs report
 * through the callbacks; the host decides what to rebuild / recolour.
 */
export const createControls = (
    parent: HTMLElement,
    options: ControlsOptions,
    callbacks: ControlsCallbacks,
): Controls => {
    let level = clamp(options.level, MIN_LEVEL, MAX_LEVEL);
    let seaLevel = clamp(options.seaLevel, MIN_SEA_LEVEL, MAX_SEA_LEVEL);
    let autoRotate = options.autoRotate;
    let viewMode = options.viewMode;
    let seedMode = options.seedMode;
    let branchesPerSeed = clamp(
        Math.round(options.branchesPerSeed),
        MIN_BRANCHES,
        MAX_BRANCHES,
    );
    let showVelocities = options.showVelocities;

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
        minWidth: "220px",
    } satisfies Partial<CSSStyleDeclaration>);

    const title = document.createElement("div");
    title.textContent = "Tectonic World-Builder";
    Object.assign(title.style, {
        fontWeight: "600",
        marginBottom: "10px",
        letterSpacing: "0.02em",
    } satisfies Partial<CSSStyleDeclaration>);

    // Subdivision-level slider.
    const levelLabel = makeLabel();
    const levelSlider = makeSlider(MIN_LEVEL, MAX_LEVEL, 1, level);

    // Sea-level slider.
    const seaLabel = makeLabel();
    const seaSlider = makeSlider(MIN_SEA_LEVEL, MAX_SEA_LEVEL, 0.5, seaLevel);

    // Auto-rotate toggle.
    const rotateToggle = makeToggle("Auto-rotate", autoRotate);

    // View-mode selector (colour / data / plate).
    const viewLabel = makeLabel();
    viewLabel.textContent = "View";
    const viewSelect = makeSelect(
        [
            { value: "colour", label: "Elevation colour" },
            { value: "data", label: "Data (all properties)" },
            { value: "plate", label: "Plates" },
            { value: "crust", label: "Crust type" },
            { value: "density", label: "Density" },
        ],
        viewMode,
    );

    // Seed-placement mode: clicks drop plate seeds instead of orbiting.
    const seedToggle = makeToggle("Place plate seeds (click globe)", seedMode);

    // Plate-velocity arrows overlay toggle.
    const velocityToggle = makeToggle("Show plate velocities", showVelocities);

    // Branches-per-seed slider (how many faults each seed fans out).
    const branchesLabel = makeLabel();
    const branchesSlider = makeSlider(
        MIN_BRANCHES,
        MAX_BRANCHES,
        1,
        branchesPerSeed,
    );

    // Clear-faults button.
    const clearButton = makeButton("Clear faults");

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
    const plateEl = document.createElement("span");
    readout.append(fpsEl, cellEl, plateEl);

    const note = document.createElement("div");
    Object.assign(note.style, {
        marginTop: "6px",
        opacity: "0.7",
        fontSize: "11px",
        minHeight: "13px",
    } satisfies Partial<CSSStyleDeclaration>);

    const renderLevelLabel = (): void => {
        levelLabel.textContent = `Subdivision level: ${level}`;
    };
    const renderSeaLabel = (): void => {
        seaLabel.textContent = `Sea level: ${seaLevel.toFixed(1)}`;
    };
    const renderBranchesLabel = (): void => {
        branchesLabel.textContent = `Branches per seed: ${branchesPerSeed}`;
    };
    renderLevelLabel();
    renderSeaLabel();
    renderBranchesLabel();
    fpsEl.textContent = "FPS: --";
    cellEl.textContent = "Cells: --";
    plateEl.textContent = "Plates: --";

    levelSlider.addEventListener("input", () => {
        level = clamp(Math.round(Number(levelSlider.value)), MIN_LEVEL, MAX_LEVEL);
        renderLevelLabel();
        callbacks.onLevelChange(level);
    });

    seaSlider.addEventListener("input", () => {
        seaLevel = clamp(Number(seaSlider.value), MIN_SEA_LEVEL, MAX_SEA_LEVEL);
        renderSeaLabel();
        callbacks.onSeaLevelChange(seaLevel);
    });

    rotateToggle.input.addEventListener("change", () => {
        autoRotate = rotateToggle.input.checked;
        callbacks.onAutoRotateChange(autoRotate);
    });

    viewSelect.addEventListener("change", () => {
        viewMode = viewSelect.value as ViewMode;
        callbacks.onViewModeChange(viewMode);
    });

    seedToggle.input.addEventListener("change", () => {
        seedMode = seedToggle.input.checked;
        callbacks.onSeedModeChange(seedMode);
    });

    velocityToggle.input.addEventListener("change", () => {
        showVelocities = velocityToggle.input.checked;
        callbacks.onVelocitiesChange(showVelocities);
    });

    branchesSlider.addEventListener("input", () => {
        branchesPerSeed = clamp(
            Math.round(Number(branchesSlider.value)),
            MIN_BRANCHES,
            MAX_BRANCHES,
        );
        renderBranchesLabel();
        callbacks.onBranchesChange(branchesPerSeed);
    });

    clearButton.addEventListener("click", () => {
        callbacks.onClearFaults();
    });

    panel.append(
        title,
        levelLabel,
        levelSlider,
        seaLabel,
        seaSlider,
        rotateToggle.row,
        viewLabel,
        viewSelect,
        seedToggle.row,
        velocityToggle.row,
        branchesLabel,
        branchesSlider,
        clearButton,
        readout,
        note,
    );
    parent.appendChild(panel);

    return {
        get level() {
            return level;
        },
        get seaLevel() {
            return seaLevel;
        },
        get autoRotate() {
            return autoRotate;
        },
        get viewMode() {
            return viewMode;
        },
        get seedMode() {
            return seedMode;
        },
        get branchesPerSeed() {
            return branchesPerSeed;
        },
        get showVelocities() {
            return showVelocities;
        },
        setCellCount: (count: number) => {
            cellEl.textContent = `Cells: ${count.toLocaleString()}`;
        },
        setPlateCount: (count: number) => {
            plateEl.textContent = `Plates: ${count.toLocaleString()}`;
        },
        setFps: (fps: number) => {
            fpsEl.textContent = `FPS: ${fps.toFixed(0)}`;
        },
        setNote: (text: string) => {
            note.textContent = text;
        },
    };
};

interface SelectOption {
    value: string;
    label: string;
}

const makeSelect = (
    options: SelectOption[],
    selected: string,
): HTMLSelectElement => {
    const select = document.createElement("select");
    Object.assign(select.style, {
        width: "100%",
        marginBottom: "10px",
        padding: "4px 6px",
        background: "rgba(20, 26, 38, 0.9)",
        color: "#e6ecf5",
        border: "1px solid rgba(120, 140, 180, 0.3)",
        borderRadius: "6px",
        font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
        cursor: "pointer",
    } satisfies Partial<CSSStyleDeclaration>);
    for (const option of options) {
        const element = document.createElement("option");
        element.value = option.value;
        element.textContent = option.label;
        if (option.value === selected) element.selected = true;
        select.append(element);
    }
    return select;
};

const makeButton = (labelText: string): HTMLButtonElement => {
    const button = document.createElement("button");
    button.textContent = labelText;
    Object.assign(button.style, {
        width: "100%",
        marginBottom: "10px",
        padding: "6px 8px",
        background: "rgba(79, 143, 240, 0.18)",
        color: "#e6ecf5",
        border: "1px solid rgba(120, 140, 180, 0.35)",
        borderRadius: "6px",
        font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
        cursor: "pointer",
    } satisfies Partial<CSSStyleDeclaration>);
    return button;
};

interface Toggle {
    row: HTMLLabelElement;
    input: HTMLInputElement;
}

const makeToggle = (labelText: string, checked: boolean): Toggle => {
    const row = document.createElement("label");
    Object.assign(row.style, {
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "10px",
        marginBottom: "8px",
        cursor: "pointer",
        opacity: "0.95",
    } satisfies Partial<CSSStyleDeclaration>);

    const text = document.createElement("span");
    text.textContent = labelText;

    const input = document.createElement("input");
    input.type = "checkbox";
    input.checked = checked;
    Object.assign(input.style, {
        width: "32px",
        height: "18px",
        cursor: "pointer",
        accentColor: "#4f8ff0",
    } satisfies Partial<CSSStyleDeclaration>);

    row.append(text, input);
    return { row, input };
};

const makeLabel = (): HTMLLabelElement => {
    const label = document.createElement("label");
    Object.assign(label.style, {
        display: "block",
        marginBottom: "4px",
        opacity: "0.85",
    } satisfies Partial<CSSStyleDeclaration>);
    return label;
};

const makeSlider = (
    min: number,
    max: number,
    stepValue: number,
    value: number,
): HTMLInputElement => {
    const slider = document.createElement("input");
    slider.type = "range";
    slider.min = String(min);
    slider.max = String(max);
    slider.step = String(stepValue);
    slider.value = String(value);
    Object.assign(slider.style, {
        width: "100%",
        marginBottom: "10px",
        accentColor: "#4f8ff0",
    } satisfies Partial<CSSStyleDeclaration>);
    return slider;
};

const clamp = (value: number, min: number, max: number): number =>
    Math.max(min, Math.min(max, value));
