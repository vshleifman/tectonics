import { clamp } from "../math/scalar";
import { colourForPlate, toCssRgb } from "../render/colour";
import {
  AGE_BIN_MYR,
  AGE_BINS,
  EARTH,
  type WorldMetrics,
} from "../sim/metrics";

/**
 * The one data hue used where a chart has a single series — the panel accent.
 * Earth's reference is a dashed line rather than a second hue: a target is an
 * annotation, not a series of its own.
 */
const CHART_INK = "#4f8ff0";
const CHART_REFERENCE_INK = "rgba(230, 236, 245, 0.45)";

/** Distinct tints for a chart that draws several series, legend-keyed. */
const SERIES_COLOURS = [
  "#4f8ff0",
  "#e39b3b",
  "#3dbe8b",
  "#e06b8a",
  "#b48cf0",
  "#7ec8e3",
];

const SVG_NS = "http://www.w3.org/2000/svg";
const LINE_WIDTH = 360;
const LINE_HEIGHT = 96;
const HISTOGRAM_HEIGHT = 52;
/** Gap (in chart units) between neighbouring bars, so fills never touch. */
const BAR_GAP = 2;
/** How many samples to keep before halving the history. */
const HISTORY_LIMIT = 2000;

/** One series on a time chart. `null` from `get` leaves a gap in the line. */
interface ChartSeries {
  label: string;
  get: (metrics: WorldMetrics) => number | null;
  /** Horizontal Earth reference, drawn dashed in this series' tint. */
  earth?: number;
}

interface LineChartOptions {
  title: string;
  series: readonly ChartSeries[];
  /** A single Earth reference when the series don't carry their own. */
  earth?: number;
  unit?: string;
  format?: (value: number) => string;
}

interface ChartView {
  element: HTMLElement;
  update: (samples: readonly WorldMetrics[]) => void;
}

interface TabView {
  id: string;
  label: string;
  button: HTMLButtonElement;
  body: HTMLElement;
  update: (samples: readonly WorldMetrics[]) => void;
}

/** The diagnostics dialog: opened from the simulation panel, updated live. */
export interface DiagnosticsPanel {
  dialog: HTMLDialogElement;
  /** Live readings pinned out of the dialog, hidden while nothing is pinned. */
  pinned: HTMLElement;
  open: () => void;
  update: (metrics: WorldMetrics) => void;
}

/** A control that pins one reading onto {@link DiagnosticsPanel.pinned}. */
interface PinBoard {
  control: (
    id: string,
    label: string,
    read: (metrics: WorldMetrics) => string,
  ) => HTMLButtonElement;
}

interface PinBoardHandle extends PinBoard {
  panel: HTMLElement;
  refresh: (metrics: WorldMetrics | undefined) => void;
}

const pinId = (title: string, label: string): string =>
  `${title}:${label}`
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");

const makePinButton = (): HTMLButtonElement => {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "Pin";
  Object.assign(button.style, {
    flex: "0 0 auto",
    padding: "0 6px",
    background: "transparent",
    color: "#e6ecf5",
    border: "1px solid rgba(120, 140, 180, 0.35)",
    borderRadius: "4px",
    font: "10px/1.6 ui-sans-serif, system-ui, sans-serif",
    cursor: "pointer",
    opacity: "0.8",
  } satisfies Partial<CSSStyleDeclaration>);
  return button;
};

/**
 * Session memory of which readings are pinned. The first registration of an id
 * owns the label and the formatter, so a chart series and the matching
 * overview row share one pin.
 */
const createPinBoard = (): PinBoardHandle => {
  const items = new Map<
    string,
    { label: string; read: (metrics: WorldMetrics) => string }
  >();
  const buttons = new Map<string, HTMLButtonElement[]>();
  const pinned = new Set<string>();
  let latest: WorldMetrics | undefined;

  const panel = document.createElement("div");
  Object.assign(panel.style, {
    display: "none",
    marginTop: "8px",
    paddingTop: "6px",
    borderTop: "1px solid rgba(120, 140, 180, 0.18)",
  } satisfies Partial<CSSStyleDeclaration>);

  const paintButton = (id: string, button: HTMLButtonElement): void => {
    const on = pinned.has(id);
    button.textContent = on ? "Pinned" : "Pin";
    button.setAttribute("aria-pressed", String(on));
    button.style.borderColor = on ? "#4f8ff0" : "rgba(120, 140, 180, 0.35)";
    button.style.color = on ? "#4f8ff0" : "#e6ecf5";
  };

  const render = (): void => {
    panel.replaceChildren();
    if (pinned.size === 0) {
      panel.style.display = "none";
      return;
    }
    panel.style.display = "";
    const heading = document.createElement("div");
    heading.textContent = "Pinned";
    Object.assign(heading.style, {
      fontWeight: "600",
      marginBottom: "4px",
      fontSize: "12px",
    } satisfies Partial<CSSStyleDeclaration>);
    panel.append(heading);
    for (const id of pinned) {
      const item = items.get(id);
      if (!item) continue;
      const row = document.createElement("div");
      Object.assign(row.style, {
        display: "flex",
        justifyContent: "space-between",
        gap: "10px",
        fontSize: "12px",
        fontVariantNumeric: "tabular-nums",
        padding: "2px 0",
      } satisfies Partial<CSSStyleDeclaration>);
      const name = document.createElement("span");
      name.textContent = item.label;
      name.style.opacity = "0.8";
      const value = document.createElement("span");
      value.textContent = latest ? item.read(latest) : "—";
      row.append(name, value);
      panel.append(row);
    }
  };

  return {
    panel,
    refresh: (metrics) => {
      latest = metrics;
      render();
    },
    control: (id, label, read) => {
      if (!items.has(id)) items.set(id, { label, read });
      const button = makePinButton();
      button.setAttribute("aria-label", `Pin ${label}`);
      const list = buttons.get(id) ?? [];
      list.push(button);
      buttons.set(id, list);
      button.addEventListener("click", (event) => {
        event.stopPropagation();
        if (pinned.has(id)) pinned.delete(id);
        else pinned.add(id);
        for (const entry of buttons.get(id) ?? []) paintButton(id, entry);
        render();
      });
      paintButton(id, button);
      return button;
    },
  };
};

/**
 * Samples of {@link WorldMetrics} in simulated time.
 *
 * A repeated `elapsedMyr` is the same instant published twice, so it is
 * dropped. Time running backwards means the world was rebuilt, and the old
 * trajectory no longer belongs to it. Past {@link HISTORY_LIMIT} samples every
 * other point is dropped, which keeps the chart's shape and its latest point.
 */
const createMetricsHistory = (): {
  push: (metrics: WorldMetrics) => void;
  samples: () => readonly WorldMetrics[];
} => {
  let samples: WorldMetrics[] = [];

  const push = (metrics: WorldMetrics): void => {
    const last = samples[samples.length - 1];
    if (last) {
      if (metrics.elapsedMyr < last.elapsedMyr) samples = [];
      else if (metrics.elapsedMyr === last.elapsedMyr) return;
    }
    samples.push(metrics);
    if (samples.length <= HISTORY_LIMIT) return;
    const thinned: WorldMetrics[] = [];
    for (let i = 0; i < samples.length; i += 2) thinned.push(samples[i]);
    const newest = samples[samples.length - 1];
    if (thinned[thinned.length - 1] !== newest) thinned.push(newest);
    samples = thinned;
  };

  return { push, samples: () => samples };
};

const formatNumber = (value: number): string => {
  const abs = Math.abs(value);
  if (abs !== 0 && (abs < 0.01 || abs >= 1000)) return value.toExponential(1);
  return Number.isInteger(value) ? String(value) : value.toFixed(2);
};

const formatPercent = (value: number): string => `${(value * 100).toFixed(1)}%`;

const formatCount = (value: number): string => String(Math.round(value));

const formatMyr = (value: number): string => String(Math.round(value));

const formatKm = (value: number): string => value.toFixed(1);

/** A correlation with its sign always shown, so -0.1 and +0.1 do not read alike. */
const formatSigned = (value: number): string =>
  `${value >= 0 ? "+" : ""}${value.toFixed(2)}`;

const formatRate = (value: number): string =>
  `${(value * 100).toExponential(1)}`;

/**
 * Earth's sea-floor age-area distribution, as a share per 20 Myr bin.
 *
 * Ocean floor is created at ridges at a roughly steady rate and consumed at
 * trenches at a rate proportional to how much of it there is, which gives an
 * age-area distribution that falls off linearly to zero at the oldest surviving
 * crust (~180 Myr). Derived here from that straight line rather than typed in,
 * so it stays tied to {@link EARTH.maxOceanAge}.
 */
const earthAgeHistogram = (bins: number): number[] => {
  const weights: number[] = [];
  let total = 0;
  for (let i = 0; i < bins; i++) {
    const centre = (i + 0.5) * AGE_BIN_MYR;
    const weight = Math.max(0, 1 - centre / EARTH.maxOceanAge);
    weights.push(weight);
    total += weight;
  }
  return weights.map((weight) => (total > 0 ? weight / total : 0));
};

const chartTitle = (text: string): HTMLDivElement => {
  const title = document.createElement("div");
  title.textContent = text;
  Object.assign(title.style, {
    fontWeight: "600",
    marginBottom: "4px",
  } satisfies Partial<CSSStyleDeclaration>);
  return title;
};

const titled = (text: string, pin: HTMLButtonElement): HTMLDivElement => {
  const row = document.createElement("div");
  Object.assign(row.style, {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    gap: "8px",
    marginBottom: "4px",
  } satisfies Partial<CSSStyleDeclaration>);
  const title = chartTitle(text);
  title.style.marginBottom = "0";
  row.append(title, pin);
  return row;
};

const axisRow = (): {
  row: HTMLDivElement;
  start: HTMLSpanElement;
  end: HTMLSpanElement;
} => {
  const row = document.createElement("div");
  Object.assign(row.style, {
    display: "flex",
    justifyContent: "space-between",
    gap: "8px",
    opacity: "0.45",
    fontSize: "10px",
    fontVariantNumeric: "tabular-nums",
    marginTop: "2px",
  } satisfies Partial<CSSStyleDeclaration>);
  const start = document.createElement("span");
  const end = document.createElement("span");
  row.append(start, end);
  return { row, start, end };
};

const readoutLine = (): HTMLDivElement => {
  const readout = document.createElement("div");
  Object.assign(readout.style, {
    minHeight: "16px",
    marginTop: "4px",
    fontSize: "11px",
    opacity: "0.75",
    fontVariantNumeric: "tabular-nums",
  } satisfies Partial<CSSStyleDeclaration>);
  return readout;
};

const legendSwatch = (colour: string, dashed: boolean): HTMLSpanElement => {
  const swatch = document.createElement("span");
  Object.assign(swatch.style, {
    width: "14px",
    height: "0",
    borderTop: dashed ? `2px dashed ${colour}` : `2px solid ${colour}`,
    flex: "0 0 auto",
  } satisfies Partial<CSSStyleDeclaration>);
  return swatch;
};

const legendItem = (
  colour: string,
  label: string,
  dashed: boolean,
): HTMLSpanElement => {
  const item = document.createElement("span");
  Object.assign(item.style, {
    display: "flex",
    alignItems: "center",
    gap: "5px",
  } satisfies Partial<CSSStyleDeclaration>);
  const text = document.createElement("span");
  text.textContent = label;
  item.append(legendSwatch(colour, dashed), text);
  return item;
};

interface ChartDomain {
  xMin: number;
  xMax: number;
  yLo: number;
  yHi: number;
}

/**
 * A polyline per series against `elapsedMyr`, with a dashed Earth line where
 * one exists, min/max axis labels, and a hover readout of the nearest sample.
 */
const makeLineChart = (
  options: LineChartOptions,
  pins: PinBoard,
): ChartView => {
  const format = options.format ?? formatNumber;
  const show = (value: number): string =>
    options.unit ? `${format(value)} ${options.unit}` : format(value);

  const series = options.series.map((entry) => ({ ...entry }));
  const root = document.createElement("div");
  root.append(chartTitle(options.title));

  const legend = document.createElement("div");
  Object.assign(legend.style, {
    display: "flex",
    flexWrap: "wrap",
    gap: "6px 12px",
    marginBottom: "6px",
    fontSize: "11px",
    opacity: "0.85",
  } satisfies Partial<CSSStyleDeclaration>);
  series.forEach((entry, index) => {
    const colour = SERIES_COLOURS[index % SERIES_COLOURS.length] ?? CHART_INK;
    const item = legendItem(colour, entry.label, false);
    const pinLabel =
      series.length === 1 ? options.title : `${options.title} · ${entry.label}`;
    item.append(
      pins.control(pinId(options.title, entry.label), pinLabel, (metrics) => {
        const value = entry.get(metrics);
        const text = value === null ? "—" : show(value);
        return entry.earth === undefined
          ? text
          : `${text} · Earth ${show(entry.earth)}`;
      }),
    );
    legend.append(item);
    if (entry.earth !== undefined) {
      legend.append(legendItem(colour, `Earth ${show(entry.earth)}`, true));
    }
  });
  if (options.earth !== undefined) {
    legend.append(
      legendItem(CHART_REFERENCE_INK, `Earth ${show(options.earth)}`, true),
    );
  }
  root.append(legend);

  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", `0 0 ${LINE_WIDTH} ${LINE_HEIGHT}`);
  svg.setAttribute("preserveAspectRatio", "none");
  Object.assign(svg.style, {
    display: "block",
    width: "100%",
    height: `${LINE_HEIGHT}px`,
    overflow: "visible",
    touchAction: "none",
  } satisfies Partial<CSSStyleDeclaration>);

  const earthLines: SVGLineElement[] = [];
  for (const entry of series) {
    if (entry.earth === undefined) continue;
    const colour =
      SERIES_COLOURS[series.indexOf(entry) % SERIES_COLOURS.length] ??
      CHART_INK;
    earthLines.push(referenceLine(svg, colour));
  }
  const sharedEarth =
    options.earth === undefined
      ? null
      : referenceLine(svg, CHART_REFERENCE_INK);

  const seriesGroup = document.createElementNS(SVG_NS, "g");
  svg.append(seriesGroup);

  const hoverLine = document.createElementNS(SVG_NS, "line");
  hoverLine.setAttribute("y1", "0");
  hoverLine.setAttribute("y2", String(LINE_HEIGHT));
  hoverLine.setAttribute("stroke", "rgba(230, 236, 245, 0.45)");
  hoverLine.setAttribute("stroke-width", "1");
  hoverLine.setAttribute("vector-effect", "non-scaling-stroke");
  hoverLine.style.display = "none";
  svg.append(hoverLine);

  const axis = axisRow();
  const readout = readoutLine();
  readout.textContent = "Hover the chart.";
  root.append(svg, axis.row, readout);

  let plotted: readonly WorldMetrics[] = [];
  let domain: ChartDomain = { xMin: 0, xMax: 1, yLo: 0, yHi: 1 };

  const yOf = (value: number): number =>
    LINE_HEIGHT -
    ((value - domain.yLo) / (domain.yHi - domain.yLo)) * LINE_HEIGHT;

  const update = (samples: readonly WorldMetrics[]): void => {
    plotted = samples;
    seriesGroup.replaceChildren();
    hoverLine.style.display = "none";
    const earthVisible = samples.length === 0 ? "none" : "";
    for (const line of earthLines) line.style.display = earthVisible;
    if (sharedEarth) sharedEarth.style.display = earthVisible;
    if (samples.length === 0) {
      readout.textContent = "No samples yet.";
      axis.start.textContent = "";
      axis.end.textContent = "";
      return;
    }

    let yMin = Number.POSITIVE_INFINITY;
    let yMax = Number.NEGATIVE_INFINITY;
    const consider = (value: number): void => {
      yMin = Math.min(yMin, value);
      yMax = Math.max(yMax, value);
    };
    for (const sample of samples) {
      for (const entry of series) {
        const value = entry.get(sample);
        if (value !== null) consider(value);
      }
    }
    for (const entry of series) {
      if (entry.earth !== undefined) consider(entry.earth);
    }
    if (options.earth !== undefined) consider(options.earth);
    if (!Number.isFinite(yMin) || !Number.isFinite(yMax)) {
      yMin = 0;
      yMax = 1;
    }
    if (yMin === yMax) {
      yMin -= 1;
      yMax += 1;
    }
    const pad = (yMax - yMin) * 0.08;
    const first = samples[0];
    const last = samples[samples.length - 1];
    domain = {
      xMin: first.elapsedMyr,
      xMax: last.elapsedMyr,
      yLo: yMin - pad,
      yHi: yMax + pad,
    };

    const xOf = (time: number): number =>
      domain.xMax === domain.xMin
        ? LINE_WIDTH / 2
        : ((time - domain.xMin) / (domain.xMax - domain.xMin)) * LINE_WIDTH;

    let earthIndex = 0;
    for (const entry of series) {
      if (entry.earth === undefined) continue;
      const y = String(yOf(entry.earth));
      const line = earthLines[earthIndex];
      if (line) {
        line.setAttribute("y1", y);
        line.setAttribute("y2", y);
      }
      earthIndex += 1;
    }
    if (sharedEarth && options.earth !== undefined) {
      const y = String(yOf(options.earth));
      sharedEarth.setAttribute("y1", y);
      sharedEarth.setAttribute("y2", y);
    }

    series.forEach((entry, index) => {
      const colour = SERIES_COLOURS[index % SERIES_COLOURS.length] ?? CHART_INK;
      let points: string[] = [];
      const flush = (): void => {
        if (points.length >= 2) {
          const line = document.createElementNS(SVG_NS, "polyline");
          line.setAttribute("fill", "none");
          line.setAttribute("stroke", colour);
          line.setAttribute("stroke-width", "1.6");
          line.setAttribute("stroke-linejoin", "round");
          line.setAttribute("stroke-linecap", "round");
          line.setAttribute("vector-effect", "non-scaling-stroke");
          line.setAttribute("points", points.join(" "));
          seriesGroup.append(line);
        } else if (points.length === 1) {
          const [x, y] = points[0].split(",");
          const dot = document.createElementNS(SVG_NS, "circle");
          dot.setAttribute("cx", x);
          dot.setAttribute("cy", y);
          dot.setAttribute("r", "2.5");
          dot.setAttribute("fill", colour);
          seriesGroup.append(dot);
        }
        points = [];
      };
      for (const sample of samples) {
        const value = entry.get(sample);
        if (value === null) {
          flush();
          continue;
        }
        points.push(`${xOf(sample.elapsedMyr)},${yOf(value)}`);
      }
      flush();
    });

    axis.start.textContent = `${show(yMin)} – ${show(yMax)}`;
    axis.end.textContent = `${Math.round(domain.xMin)}–${Math.round(domain.xMax)} Myr`;
    readout.textContent = "Hover the chart.";
  };

  svg.addEventListener("pointermove", (event) => {
    if (plotted.length === 0) return;
    const rect = svg.getBoundingClientRect();
    if (rect.width === 0) return;
    const fraction = clamp((event.clientX - rect.left) / rect.width, 0, 1);
    const time =
      domain.xMin +
      fraction * (domain.xMax === domain.xMin ? 0 : domain.xMax - domain.xMin);
    let nearest = plotted[0];
    let best = Number.POSITIVE_INFINITY;
    for (const sample of plotted) {
      const distance = Math.abs(sample.elapsedMyr - time);
      if (distance < best) {
        best = distance;
        nearest = sample;
      }
    }
    const x =
      domain.xMax === domain.xMin
        ? LINE_WIDTH / 2
        : ((nearest.elapsedMyr - domain.xMin) / (domain.xMax - domain.xMin)) *
          LINE_WIDTH;
    hoverLine.setAttribute("x1", String(x));
    hoverLine.setAttribute("x2", String(x));
    hoverLine.style.display = "";
    const parts = series.map((entry) => {
      const value = entry.get(nearest);
      return `${entry.label} ${value === null ? "—" : show(value)}`;
    });
    readout.textContent = `${Math.round(nearest.elapsedMyr)} Myr · ${parts.join(" · ")}`;
  });
  svg.addEventListener("pointerleave", () => {
    hoverLine.style.display = "none";
    if (plotted.length > 0) readout.textContent = "Hover the chart.";
  });

  return { element: root, update };
};

const referenceLine = (svg: SVGSVGElement, colour: string): SVGLineElement => {
  const line = document.createElementNS(SVG_NS, "line");
  line.setAttribute("x1", "0");
  line.setAttribute("x2", String(LINE_WIDTH));
  line.setAttribute("y1", "0");
  line.setAttribute("y2", "0");
  line.setAttribute("stroke", colour);
  line.setAttribute("stroke-width", "1.25");
  line.setAttribute("stroke-dasharray", "4 3");
  line.setAttribute("vector-effect", "non-scaling-stroke");
  svg.append(line);
  return line;
};

/** The current sea-floor age-area histogram, against Earth's linear falloff. */
const makeAgeHistogram = (
  pins: PinBoard,
): {
  element: HTMLElement;
  update: (metrics: WorldMetrics | undefined) => void;
} => {
  const root = document.createElement("div");
  root.append(
    titled(
      "Sea floor by age",
      pins.control("sea-floor-by-age", "Sea floor <20 Myr", (metrics) => {
        const share = metrics.ageHistogram[0] ?? 0;
        return `${(share * 100).toFixed(1)}% of ocean`;
      }),
    ),
  );

  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", `0 0 ${LINE_WIDTH} ${HISTOGRAM_HEIGHT}`);
  svg.setAttribute("preserveAspectRatio", "none");
  Object.assign(svg.style, {
    display: "block",
    width: "100%",
    height: `${HISTOGRAM_HEIGHT}px`,
    overflow: "visible",
  } satisfies Partial<CSSStyleDeclaration>);

  const reference = earthAgeHistogram(AGE_BINS);
  const barWidth = LINE_WIDTH / AGE_BINS;
  const bars: SVGRectElement[] = [];
  const labels: SVGTitleElement[] = [];
  for (let i = 0; i < AGE_BINS; i++) {
    const rect = document.createElementNS(SVG_NS, "rect");
    rect.setAttribute("x", String(i * barWidth + BAR_GAP / 2));
    rect.setAttribute("width", String(Math.max(1, barWidth - BAR_GAP)));
    rect.setAttribute("rx", "2");
    rect.setAttribute("fill", CHART_INK);
    const title = document.createElementNS(SVG_NS, "title");
    rect.append(title);
    svg.append(rect);
    bars.push(rect);
    labels.push(title);
  }

  const referenceLineEl = document.createElementNS(SVG_NS, "polyline");
  referenceLineEl.setAttribute("fill", "none");
  referenceLineEl.setAttribute("stroke", CHART_REFERENCE_INK);
  referenceLineEl.setAttribute("stroke-width", "1.5");
  referenceLineEl.setAttribute("stroke-dasharray", "4 3");
  referenceLineEl.setAttribute("vector-effect", "non-scaling-stroke");
  const referenceTitle = document.createElementNS(SVG_NS, "title");
  referenceTitle.textContent = "Earth: linear falloff to ~180 Myr";
  referenceLineEl.append(referenceTitle);
  svg.append(referenceLineEl);

  const axis = axisRow();
  axis.start.textContent = "0";
  axis.end.textContent = `${AGE_BIN_MYR * AGE_BINS}+ Myr`;
  const key = document.createElement("span");
  key.textContent = "- - Earth";
  axis.row.insertBefore(key, axis.end);

  root.append(svg, axis.row);

  const update = (metrics: WorldMetrics | undefined): void => {
    const histogram = metrics?.ageHistogram ?? [];
    const peak = Math.max(...histogram, ...reference, 0.01);
    const scale = HISTOGRAM_HEIGHT / (peak * 1.1);
    for (let i = 0; i < AGE_BINS; i++) {
      const share = histogram[i] ?? 0;
      const height = share * scale;
      bars[i].setAttribute("y", String(HISTOGRAM_HEIGHT - height));
      bars[i].setAttribute("height", String(Math.max(0, height)));
      const from = i * AGE_BIN_MYR;
      labels[i].textContent =
        `${from}–${from + AGE_BIN_MYR} Myr: ${(share * 100).toFixed(1)}% of ocean floor`;
    }
    referenceLineEl.setAttribute(
      "points",
      reference
        .map(
          (share, i) =>
            `${i * barWidth + barWidth / 2},${HISTOGRAM_HEIGHT - share * scale}`,
        )
        .join(" "),
    );
  };

  return { element: root, update };
};

interface ScatterDot {
  id: number;
  x: number;
  y: number;
  speed: number;
  trenchFraction: number;
  areaShare: number;
}

/** The current plates: surface speed against the trench share of the perimeter. */
const makeDriveScatter = (
  pins: PinBoard,
): {
  element: HTMLElement;
  update: (metrics: WorldMetrics | undefined) => void;
} => {
  const root = document.createElement("div");
  root.append(
    titled(
      "Speed against trench fraction",
      pins.control("plate-drives", "Plates in the drive plot", (metrics) =>
        String(metrics.drives.length),
      ),
    ),
  );
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", `0 0 ${LINE_WIDTH} ${LINE_HEIGHT}`);
  svg.setAttribute("preserveAspectRatio", "none");
  Object.assign(svg.style, {
    display: "block",
    width: "100%",
    height: `${LINE_HEIGHT}px`,
    overflow: "visible",
  } satisfies Partial<CSSStyleDeclaration>);
  const plot = document.createElementNS(SVG_NS, "g");
  svg.append(plot);
  const xAxis = axisRow();
  const yAxis = axisRow();
  const readout = readoutLine();
  readout.textContent = "Hover a plate.";
  root.append(svg, yAxis.row, xAxis.row, readout);

  let dots: ScatterDot[] = [];

  const update = (metrics: WorldMetrics | undefined): void => {
    plot.replaceChildren();
    const drives = metrics?.drives ?? [];
    dots = [];
    if (drives.length === 0) {
      readout.textContent = "No plates to plot.";
      xAxis.start.textContent = "";
      xAxis.end.textContent = "";
      yAxis.start.textContent = "";
      yAxis.end.textContent = "";
      return;
    }
    let maxSpeed = 0;
    for (const drive of drives) maxSpeed = Math.max(maxSpeed, drive.speed);
    maxSpeed = Math.max(maxSpeed, 0.1) * 1.1;
    const maxTrench = 1;
    for (const drive of drives) {
      const x = (drive.trenchFraction / maxTrench) * LINE_WIDTH;
      const y = LINE_HEIGHT - (drive.speed / maxSpeed) * LINE_HEIGHT;
      const circle = document.createElementNS(SVG_NS, "circle");
      circle.setAttribute("cx", String(x));
      circle.setAttribute("cy", String(y));
      circle.setAttribute("r", "4");
      circle.setAttribute("fill", toCssRgb(colourForPlate(drive.id)));
      circle.setAttribute("stroke", "rgba(0, 0, 0, 0.45)");
      circle.setAttribute("stroke-width", "1");
      plot.append(circle);
      dots.push({
        id: drive.id,
        x,
        y,
        speed: drive.speed,
        trenchFraction: drive.trenchFraction,
        areaShare: drive.areaShare,
      });
    }
    xAxis.start.textContent = "0% trench";
    xAxis.end.textContent = "100% trench";
    yAxis.start.textContent = "0 cm/yr";
    yAxis.end.textContent = `${maxSpeed.toFixed(1)} cm/yr`;
    readout.textContent = "Hover a plate.";
  };

  svg.addEventListener("pointermove", (event) => {
    if (dots.length === 0) return;
    const rect = svg.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    const x = ((event.clientX - rect.left) / rect.width) * LINE_WIDTH;
    const y = ((event.clientY - rect.top) / rect.height) * LINE_HEIGHT;
    let nearest = dots[0];
    let best = Number.POSITIVE_INFINITY;
    for (const dot of dots) {
      const distance = (dot.x - x) ** 2 + (dot.y - y) ** 2;
      if (distance < best) {
        best = distance;
        nearest = dot;
      }
    }
    readout.textContent =
      `Plate ${nearest.id} · ${nearest.speed.toFixed(2)} cm/yr · ` +
      `trench ${(nearest.trenchFraction * 100).toFixed(0)}% · ` +
      `${(nearest.areaShare * 100).toFixed(1)}% of globe`;
  });
  svg.addEventListener("pointerleave", () => {
    if (dots.length > 0) readout.textContent = "Hover a plate.";
  });

  return { element: root, update };
};

const stack = (...children: HTMLElement[]): HTMLDivElement => {
  const column = document.createElement("div");
  Object.assign(column.style, {
    display: "flex",
    flexDirection: "column",
    gap: "16px",
  } satisfies Partial<CSSStyleDeclaration>);
  column.append(...children);
  return column;
};

const latestOf = (samples: readonly WorldMetrics[]): WorldMetrics | undefined =>
  samples[samples.length - 1];

/** World value beside the Earth value it is trying to reach. */
const makeOverview = (
  pins: PinBoard,
): {
  element: HTMLElement;
  update: (metrics: WorldMetrics | undefined) => void;
} => {
  const grid = document.createElement("div");
  Object.assign(grid.style, {
    display: "grid",
    gridTemplateColumns: "auto 1fr auto auto",
    columnGap: "10px",
    rowGap: "3px",
    alignItems: "center",
    fontVariantNumeric: "tabular-nums",
  } satisfies Partial<CSSStyleDeclaration>);

  const heading = (text: string, align: string): HTMLDivElement => {
    const el = document.createElement("div");
    el.textContent = text;
    Object.assign(el.style, {
      opacity: "0.5",
      fontSize: "11px",
      textAlign: align,
    } satisfies Partial<CSSStyleDeclaration>);
    return el;
  };
  grid.append(
    heading("", "left"),
    heading("", "left"),
    heading("world", "right"),
    heading("Earth", "right"),
  );

  const addRow = (
    label: string,
    earth: string,
    read: (metrics: WorldMetrics) => string,
    id = pinId("Overview", label),
  ): HTMLDivElement => {
    const name = document.createElement("div");
    name.textContent = label;
    name.style.opacity = "0.8";
    const value = document.createElement("div");
    value.style.textAlign = "right";
    value.textContent = "—";
    const target = document.createElement("div");
    target.textContent = earth;
    Object.assign(target.style, {
      textAlign: "right",
      opacity: "0.45",
    } satisfies Partial<CSSStyleDeclaration>);
    grid.append(pins.control(id, label, read), name, value, target);
    return value;
  };

  const elapsedValue = addRow(
    "Elapsed",
    "—",
    (metrics) => `${Math.round(metrics.elapsedMyr)} Myr`,
  );
  const plateValue = addRow(
    "Plates (major)",
    `${EARTH.totalPlates} (${EARTH.majorPlates})`,
    (metrics) =>
      `${metrics.plateCount} (${metrics.majorPlateCount}) · Earth ${EARTH.totalPlates} (${EARTH.majorPlates})`,
  );
  const continentValue = addRow(
    "Continental",
    `${Math.round(EARTH.continentalArea * 100)}%`,
    (metrics) =>
      `${(metrics.continentalArea * 100).toFixed(1)}% · Earth ${Math.round(EARTH.continentalArea * 100)}%`,
    pinId("Continental area", "Continental"),
  );
  const ageValue = addRow(
    "Ocean age mean/max",
    `${EARTH.meanOceanAge}/${EARTH.maxOceanAge}`,
    (metrics) =>
      `${Math.round(metrics.meanOceanAge)}/${Math.round(metrics.maxOceanAge)} · Earth ${EARTH.meanOceanAge}/${EARTH.maxOceanAge}`,
  );
  const recycleValue = addRow(
    "Ocean recycling",
    `${EARTH.oceanRecyclingMyr} Myr`,
    (metrics) =>
      metrics.oceanRecyclingMyr === null
        ? "never"
        : `${Math.round(metrics.oceanRecyclingMyr)} Myr · Earth ${EARTH.oceanRecyclingMyr} Myr`,
  );
  const boundaryValue = addRow("Conv/div/trans %", "", (metrics) => {
    const [conv, div, trans] = metrics.boundaryShare;
    return `${Math.round(conv * 100)}/${Math.round(div * 100)}/${Math.round(trans * 100)}`;
  });
  const driveValue = addRow(
    "Speed vs trench",
    `+${EARTH.speedTrenchCorrelation.toFixed(1)}`,
    (metrics) =>
      metrics.speedTrenchCorrelation === null
        ? "—"
        : `${formatSigned(metrics.speedTrenchCorrelation)} · Earth +${EARTH.speedTrenchCorrelation.toFixed(1)}`,
  );

  const update = (metrics: WorldMetrics | undefined): void => {
    if (!metrics) return;
    elapsedValue.textContent = `${Math.round(metrics.elapsedMyr)} Myr`;
    plateValue.textContent = `${metrics.plateCount} (${metrics.majorPlateCount})`;
    continentValue.textContent = `${(metrics.continentalArea * 100).toFixed(1)}%`;
    ageValue.textContent = `${Math.round(metrics.meanOceanAge)}/${Math.round(metrics.maxOceanAge)}`;
    recycleValue.textContent =
      metrics.oceanRecyclingMyr === null
        ? "never"
        : `${Math.round(metrics.oceanRecyclingMyr)} Myr`;
    const [conv, div, trans] = metrics.boundaryShare;
    boundaryValue.textContent = `${Math.round(conv * 100)}/${Math.round(div * 100)}/${Math.round(trans * 100)}`;
    driveValue.textContent =
      metrics.speedTrenchCorrelation === null
        ? "—"
        : formatSigned(metrics.speedTrenchCorrelation);
  };

  return { element: grid, update };
};

const makeTab = (
  id: string,
  label: string,
  body: HTMLElement,
  update: (samples: readonly WorldMetrics[]) => void,
): TabView => {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = label;
  Object.assign(button.style, {
    flex: "0 0 auto",
    padding: "4px 8px",
    background: "transparent",
    color: "#e6ecf5",
    border: "1px solid transparent",
    borderRadius: "6px",
    font: "12px/1.3 ui-sans-serif, system-ui, sans-serif",
    cursor: "pointer",
  } satisfies Partial<CSSStyleDeclaration>);
  return { id, label, button, body, update };
};

/**
 * A non-modal, draggable diagnostics dialog. `show` rather than `showModal`
 * leaves the globe interactive. History is recorded whether or not the dialog
 * is open; only the visible tab is redrawn. Pinned readings refresh either way.
 */
export const createDiagnostics = (parent: HTMLElement): DiagnosticsPanel => {
  const history = createMetricsHistory();
  const pins = createPinBoard();

  const overview = makeOverview(pins);
  const plates = makeLineChart(
    {
      title: "Plate count",
      series: [
        { label: "Plates", get: (m) => m.plateCount, earth: EARTH.totalPlates },
        {
          label: "Major",
          get: (m) => m.majorPlateCount,
          earth: EARTH.majorPlates,
        },
      ],
      format: formatCount,
    },
    pins,
  );
  const continentalArea = makeLineChart(
    {
      title: "Continental area",
      series: [
        {
          label: "Continental",
          get: (m) => m.continentalArea,
          earth: EARTH.continentalArea,
        },
        { label: "Thin", get: (m) => m.thinContinentalArea },
      ],
      format: formatPercent,
    },
    pins,
  );
  const landmasses = makeLineChart(
    {
      title: "Landmasses",
      series: [{ label: "Continents", get: (m) => m.continentCount }],
      format: formatCount,
    },
    pins,
  );
  const continentShare = makeLineChart(
    {
      title: "Largest landmass and craton",
      series: [
        { label: "Largest share", get: (m) => m.largestContinentShare },
        { label: "Craton", get: (m) => m.cratonShare },
      ],
      format: formatPercent,
    },
    pins,
  );
  const thickness = makeLineChart(
    {
      title: "Mean continental thickness",
      unit: "km",
      series: [{ label: "Thickness", get: (m) => m.meanContinentalThickness }],
      format: formatKm,
    },
    pins,
  );
  const growth = makeLineChart(
    {
      title: "Continental growth",
      unit: "%/Myr",
      series: [
        { label: "Accretion", get: (m) => m.continentalisedByAccretion },
        { label: "Piling", get: (m) => m.continentalisedByPiling },
        { label: "Spreading", get: (m) => m.continentalisedBySpreading },
        { label: "Infill", get: (m) => m.continentalisedByInfill },
        { label: "Plume", get: (m) => m.continentalisedByPlume },
        { label: "Recycled", get: (m) => m.continentalRecycled },
        { label: "Growth", get: (m) => m.continentalGrowth },
      ],
      format: formatRate,
    },
    pins,
  );
  const loss = makeLineChart(
    {
      title: "Continental loss",
      unit: "%/Myr",
      series: [
        { label: "Eroded", get: (m) => m.continentalEroded },
        { label: "Delaminated", get: (m) => m.delaminated },
        { label: "Foundered", get: (m) => m.foundered },
        { label: "Decontinentalised", get: (m) => m.decontinentalised },
        { label: "Dripped (ocean)", get: (m) => m.dripped },
      ],
      format: formatRate,
    },
    pins,
  );
  const primordial = makeLineChart(
    {
      title: "Primordial era",
      series: [
        { label: "Mantle heat", get: (m) => m.mantleExcess },
        { label: "Plumes", get: (m) => m.activePlumes },
      ],
      format: formatNumber,
    },
    pins,
  );
  const oceanAge = makeLineChart(
    {
      title: "Ocean age",
      unit: "Myr",
      series: [
        {
          label: "Mean",
          get: (m) => m.meanOceanAge,
          earth: EARTH.meanOceanAge,
        },
        {
          label: "Oldest",
          get: (m) => m.maxOceanAge,
          earth: EARTH.maxOceanAge,
        },
      ],
      format: formatMyr,
    },
    pins,
  );
  const recycling = makeLineChart(
    {
      title: "Ocean recycling time",
      unit: "Myr",
      series: [
        {
          label: "Recycling",
          get: (m) => m.oceanRecyclingMyr,
          earth: EARTH.oceanRecyclingMyr,
        },
      ],
      format: formatMyr,
    },
    pins,
  );
  const histogram = makeAgeHistogram(pins);
  const boundaryLabels = ["Convergent", "Divergent", "Transform", "Inactive"];
  const boundaries = makeLineChart(
    {
      title: "Boundary length",
      series: boundaryLabels.map((label, index) => ({
        label,
        get: (m: WorldMetrics) => m.boundaryShare[index] ?? 0,
      })),
      format: formatPercent,
    },
    pins,
  );
  const correlation = makeLineChart(
    {
      title: "Speed against trench fraction",
      series: [
        {
          label: "Correlation",
          get: (m) => m.speedTrenchCorrelation,
          earth: EARTH.speedTrenchCorrelation,
        },
      ],
      format: formatSigned,
    },
    pins,
  );
  const scatter = makeDriveScatter(pins);
  const holes = makeLineChart(
    {
      title: "Interior holes",
      unit: "%/Myr",
      series: [
        { label: "Opened", get: (m) => m.interiorHoles },
        { label: "Unfilled", get: (m) => m.interiorHolesUnfilled },
      ],
      format: formatRate,
    },
    pins,
  );
  const pocketCount = makeLineChart(
    {
      title: "Ocean pockets",
      series: [{ label: "Pockets", get: (m) => m.oceanPocketCount }],
      format: formatCount,
    },
    pins,
  );
  const pocketArea = makeLineChart(
    {
      title: "Ocean pocket area",
      series: [{ label: "Area", get: (m) => m.oceanPocketArea }],
      format: formatPercent,
    },
    pins,
  );
  const cohesion = makeLineChart(
    {
      title: "Continental cohesion",
      unit: "%/Myr",
      series: [
        { label: "Torn", get: (m) => m.tornContinent },
        { label: "Rejoined", get: (m) => m.rejoined },
      ],
      format: formatRate,
    },
    pins,
  );
  const coast = makeLineChart(
    {
      title: "Coastline share",
      series: [{ label: "Coast", get: (m) => m.coastShare }],
      format: formatPercent,
    },
    pins,
  );
  const crumbs = makeLineChart(
    {
      title: "Small landmasses",
      series: [{ label: "Crumbs", get: (m) => m.smallLandmassCount }],
      format: formatCount,
    },
    pins,
  );

  const tabs: TabView[] = [
    makeTab("overview", "Overview", overview.element, (samples) => {
      overview.update(latestOf(samples));
    }),
    makeTab("plates", "Plates", plates.element, plates.update),
    makeTab(
      "continents",
      "Continents",
      stack(
        continentalArea.element,
        landmasses.element,
        continentShare.element,
        thickness.element,
      ),
      (samples) => {
        continentalArea.update(samples);
        landmasses.update(samples);
        continentShare.update(samples);
        thickness.update(samples);
      },
    ),
    makeTab(
      "budget",
      "Continental budget",
      stack(growth.element, loss.element, primordial.element),
      (samples) => {
        growth.update(samples);
        loss.update(samples);
        primordial.update(samples);
      },
    ),
    makeTab(
      "ocean",
      "Ocean floor",
      stack(oceanAge.element, recycling.element, histogram.element),
      (samples) => {
        oceanAge.update(samples);
        recycling.update(samples);
        histogram.update(latestOf(samples));
      },
    ),
    makeTab("boundaries", "Boundaries", boundaries.element, boundaries.update),
    makeTab(
      "drive",
      "Plate drive",
      stack(correlation.element, scatter.element),
      (samples) => {
        correlation.update(samples);
        scatter.update(latestOf(samples));
      },
    ),
    makeTab(
      "defects",
      "Defects",
      stack(
        holes.element,
        cohesion.element,
        coast.element,
        crumbs.element,
        pocketCount.element,
        pocketArea.element,
      ),
      (samples) => {
        holes.update(samples);
        cohesion.update(samples);
        coast.update(samples);
        crumbs.update(samples);
        pocketCount.update(samples);
        pocketArea.update(samples);
      },
    ),
  ];

  const dialog = document.createElement("dialog");
  Object.assign(dialog.style, {
    position: "fixed",
    inset: "auto",
    top: "64px",
    left: "50%",
    transform: "translateX(-50%)",
    width: "min(440px, calc(100vw - 24px))",
    maxHeight: "calc(100vh - 88px)",
    margin: "0",
    padding: "0",
    border: "1px solid rgba(120, 140, 180, 0.25)",
    borderRadius: "10px",
    background: "rgba(10, 14, 22, 0.94)",
    color: "#e6ecf5",
    font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
    boxShadow: "0 12px 40px rgba(0, 0, 0, 0.45)",
  } satisfies Partial<CSSStyleDeclaration>);

  const header = document.createElement("div");
  Object.assign(header.style, {
    display: "flex",
    alignItems: "center",
    gap: "8px",
    padding: "10px 12px 6px",
    cursor: "grab",
    userSelect: "none",
  } satisfies Partial<CSSStyleDeclaration>);
  const title = document.createElement("div");
  title.textContent = "Diagnostics";
  Object.assign(title.style, {
    flex: "1 1 auto",
    fontWeight: "600",
    letterSpacing: "0.02em",
  } satisfies Partial<CSSStyleDeclaration>);
  const close = document.createElement("button");
  close.type = "button";
  close.textContent = "×";
  close.setAttribute("aria-label", "Close diagnostics");
  Object.assign(close.style, {
    width: "28px",
    height: "28px",
    padding: "0",
    background: "transparent",
    color: "#e6ecf5",
    border: "1px solid rgba(120, 140, 180, 0.35)",
    borderRadius: "6px",
    font: "16px/1 ui-sans-serif, system-ui, sans-serif",
    cursor: "pointer",
  } satisfies Partial<CSSStyleDeclaration>);
  close.addEventListener("click", () => {
    dialog.close();
  });
  header.append(title, close);

  const tabBar = document.createElement("div");
  Object.assign(tabBar.style, {
    display: "flex",
    flexWrap: "wrap",
    gap: "4px",
    padding: "0 12px 8px",
  } satisfies Partial<CSSStyleDeclaration>);

  const content = document.createElement("div");
  Object.assign(content.style, {
    overflowY: "auto",
    maxHeight: "calc(100vh - 180px)",
    padding: "4px 14px 14px",
  } satisfies Partial<CSSStyleDeclaration>);

  let active = 0;
  const showTab = (index: number): void => {
    active = index;
    for (let i = 0; i < tabs.length; i++) {
      const on = i === index;
      tabs[i].button.style.background = on ? "#4f8ff0" : "transparent";
      tabs[i].button.style.borderColor = on
        ? "#4f8ff0"
        : "rgba(120, 140, 180, 0.25)";
      tabs[i].body.style.display = on ? "block" : "none";
    }
    if (dialog.open) tabs[index].update(history.samples());
  };

  for (let i = 0; i < tabs.length; i++) {
    const index = i;
    tabs[i].button.addEventListener("click", () => {
      showTab(index);
    });
    tabBar.append(tabs[i].button);
    content.append(tabs[i].body);
  }

  dialog.append(header, tabBar, content);
  parent.append(dialog);
  showTab(0);

  let dragging = false;
  let originX = 0;
  let originY = 0;
  let originLeft = 0;
  let originTop = 0;
  header.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || close.contains(event.target as Node)) return;
    dragging = true;
    originX = event.clientX;
    originY = event.clientY;
    const rect = dialog.getBoundingClientRect();
    originLeft = rect.left;
    originTop = rect.top;
    header.setPointerCapture(event.pointerId);
    header.style.cursor = "grabbing";
  });
  header.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    const left = clamp(
      originLeft + event.clientX - originX,
      8,
      window.innerWidth - 48,
    );
    const top = clamp(
      originTop + event.clientY - originY,
      8,
      window.innerHeight - 48,
    );
    dialog.style.left = `${left}px`;
    dialog.style.top = `${top}px`;
    dialog.style.transform = "none";
  });
  const endDrag = (): void => {
    dragging = false;
    header.style.cursor = "grab";
  };
  header.addEventListener("pointerup", endDrag);
  header.addEventListener("pointercancel", endDrag);

  return {
    dialog,
    pinned: pins.panel,
    open: () => {
      if (!dialog.open) dialog.show();
      tabs[active].update(history.samples());
    },
    update: (metrics) => {
      history.push(metrics);
      pins.refresh(metrics);
      if (dialog.open) tabs[active].update(history.samples());
    },
  };
};
