import { FACIES, NEVER_ARC, RESOURCE_NAMES } from "../sim/SimWorld";

/** The per-cell fields the readout prints. */
export interface ReadoutFields {
  plateId: ArrayLike<number>;
  elevation: ArrayLike<number>;
  crustType: ArrayLike<number>;
  age: ArrayLike<number>;
  density: ArrayLike<number>;
  thickness: ArrayLike<number>;
  keel: ArrayLike<number>;
  lithosphere: ArrayLike<number>;
  sediment: ArrayLike<number>;
  mafic: ArrayLike<number>;
  felsic: ArrayLike<number>;
  coverFelsic: ArrayLike<number>;
  coverMafic: ArrayLike<number>;
  arcAge: ArrayLike<number>;
  exhumed: ArrayLike<number>;
  peakTemp: ArrayLike<number>;
  peakPressure: ArrayLike<number>;
  facies: ArrayLike<number>;
  resources: ArrayLike<number>;
  heatFlow: ArrayLike<number>;
  mantleTemp: ArrayLike<number>;
  lowerSlab: ArrayLike<number>;
  dynTopo: ArrayLike<number>;
}

/** The three likeliest resources at cell `i`, if any are over 0.2. */
const resourceLines = (data: ReadoutFields, i: number): string => {
  const cells = data.thickness.length;
  const top = RESOURCE_NAMES.map((name, r) => ({
    name,
    potential: data.resources[r * cells + i],
  }))
    .filter((x) => x.potential > 0.2)
    .sort((a, b) => b.potential - a.potential)
    .slice(0, 3);
  return top
    .map((x) => `${x.name} ${(x.potential * 100).toFixed(0)}%\n`)
    .join("");
};

/** Multi-line debug dump of every per-cell property for a given cell. */
const readoutText = (data: ReadoutFields, i: number): string =>
  `id ${i}\n` +
  `plate ${data.plateId[i]}\n` +
  `elev ${data.elevation[i].toFixed(2)}\n` +
  `crust ${data.crustType[i] === 1 ? "cont" : "ocean"}\n` +
  `age ${data.age[i].toFixed(1)}\n` +
  `dens ${data.density[i].toFixed(2)}\n` +
  `thick ${data.thickness[i].toFixed(1)}\n` +
  `keel ${data.keel[i].toFixed(2)}\n` +
  `lith ${data.lithosphere[i].toFixed(0)} km\n` +
  `silt ${(data.sediment[i] * 100).toFixed(0)}%\n` +
  `maf/fel ${(data.mafic[i] * 100).toFixed(0)}/${(data.felsic[i] * 100).toFixed(0)}%\n` +
  `cover ${((data.coverFelsic[i] + data.coverMafic[i]) * data.thickness[i]).toFixed(2)} km (fel ${(data.coverFelsic[i] * 100).toFixed(0)}%, maf ${(data.coverMafic[i] * 100).toFixed(0)}%)\n` +
  (data.arcAge[i] < NEVER_ARC
    ? `arc ${data.arcAge[i].toFixed(0)} Myr ago\n`
    : "") +
  `${FACIES[data.facies[i]] ?? "?"} ${data.peakTemp[i].toFixed(0)} °C ${data.peakPressure[i].toFixed(1)} kbar\n` +
  `exhumed ${data.exhumed[i].toFixed(1)} km\n` +
  resourceLines(data, i) +
  `heat ${data.heatFlow[i].toFixed(1)} mW/m²\n` +
  `mantle ${data.mantleTemp[i] >= 0 ? "+" : ""}${data.mantleTemp[i].toFixed(0)} K\n` +
  `pile ${data.lowerSlab[i].toFixed(1)}\n` +
  `dyn ${data.dynTopo[i] >= 0 ? "+" : ""}${data.dynTopo[i].toFixed(2)} km`;

/** Gap (px) between the pointer and the readout. */
const READOUT_OFFSET = 14;

/**
 * A box beside the pointer listing every property of the cell under it, for
 * reading the world one cell at a time over whichever view is shown.
 */
export class CellReadout {
  private readonly element = document.createElement("div");

  constructor(container: HTMLElement) {
    Object.assign(this.element.style, {
      position: "fixed",
      display: "none",
      padding: "6px 8px",
      background: "rgba(10, 14, 22, 0.92)",
      color: "#e6ecf5",
      border: "1px solid rgba(120, 140, 180, 0.35)",
      borderRadius: "6px",
      font: "11px/1.35 ui-monospace, monospace",
      whiteSpace: "pre",
      pointerEvents: "none",
      zIndex: "10",
    } satisfies Partial<CSSStyleDeclaration>);
    container.appendChild(this.element);
  }

  /**
   * Show cell `i`'s properties beside the client point, kept on screen by
   * flipping to the pointer's other side near the right or bottom edge.
   */
  show(data: ReadoutFields, i: number, clientX: number, clientY: number): void {
    const element = this.element;
    element.textContent = readoutText(data, i);
    element.style.display = "block";
    const { offsetWidth: width, offsetHeight: height } = element;
    const left =
      clientX + READOUT_OFFSET + width > window.innerWidth
        ? clientX - READOUT_OFFSET - width
        : clientX + READOUT_OFFSET;
    const top =
      clientY + READOUT_OFFSET + height > window.innerHeight
        ? clientY - READOUT_OFFSET - height
        : clientY + READOUT_OFFSET;
    element.style.left = `${Math.max(0, left)}px`;
    element.style.top = `${Math.max(0, top)}px`;
  }

  hide(): void {
    this.element.style.display = "none";
  }
}
