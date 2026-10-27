import { ArrayCellMesh } from "../mesh/ArrayCellMesh";
import type { PlateSummary } from "../plates/PlateData";
import type { WorldMetrics } from "../sim/metrics";
import type { MassReadout, SimWorld, WorldFields } from "../sim/SimWorld";
import type { SimOptions, Tuning } from "../sim/tuning";
import type { World } from "./pkg/tectonics_sim.js";

/**
 * The `World` class of either build, single-threaded (`pkg`) or threaded
 * (`pkg-threads`): the two export the same API.
 */
export type WorldClass = typeof World;

/**
 * The Rust simulation behind the {@link SimWorld} interface. Per-tick fields
 * are views straight into WASM memory, rebuilt on every call because the front
 * buffer changes with each tick and a memory growth would detach old views (or,
 * for the threaded build's shared memory, leave them short of the new end).
 */
export class WasmWorld implements SimWorld {
  readonly mesh: ArrayCellMesh;
  private readonly world: World;

  /**
   * A new world at the given subdivision level, or, from the bytes of a
   * {@link saveState}, that world as it was saved (throws if this build
   * refuses them).
   */
  constructor(
    source: number | Uint8Array,
    private readonly memory: WebAssembly.Memory,
    WorldOf: WorldClass,
  ) {
    const w =
      typeof source === "number"
        ? new WorldOf(source)
        : WorldOf.fromState(source);
    this.world = w;
    this.mesh = new ArrayCellMesh({
      cellCount: w.cell_count(),
      centres: w.mesh_centres(),
      areas: w.mesh_areas(),
      neighbourOffsets: w.mesh_neighbour_offsets(),
      neighbourList: w.mesh_neighbour_list(),
      neighbourArcList: w.mesh_neighbour_arcs(),
      polyOffsets: w.mesh_poly_offsets(),
      polyCorners: w.mesh_poly_corners(),
      junctionPos: w.mesh_junction_pos(),
      arcEnds: w.mesh_arc_ends(),
      arcCells: w.mesh_arc_cells(),
      junctionArcOffsets: w.mesh_junction_arc_offsets(),
      junctionArcs: w.mesh_junction_arcs(),
    });
  }

  get elapsedMyr(): number {
    return this.world.elapsed_myr();
  }

  reassignPlates(cracked: Uint8Array, initCrust: boolean): number {
    return this.world.reassign_plates(cracked, initCrust);
  }

  tick(): void {
    this.world.tick();
  }

  setTuning(tuning: Tuning): void {
    this.world.set_tuning(tuning);
  }

  refreshSurface(): void {
    this.world.refresh_surface();
  }

  setSimOptions(options: SimOptions): void {
    this.world.set_options(options);
  }

  fields(): WorldFields {
    const { buffer } = this.memory;
    const w = this.world;
    const cells = this.mesh.cellCount;
    const arcs = this.mesh.boundaryGraph().arcCount;
    return {
      elevation: new Float32Array(buffer, w.elevation_ptr(), cells),
      plateId: new Int32Array(buffer, w.plate_id_ptr(), cells),
      crustType: new Uint8Array(buffer, w.crust_type_ptr(), cells),
      age: new Float32Array(buffer, w.age_ptr(), cells),
      density: new Float32Array(buffer, w.density_ptr(), cells),
      thickness: new Float32Array(buffer, w.thickness_ptr(), cells),
      keel: new Float32Array(buffer, w.keel_ptr(), cells),
      lithosphere: new Float32Array(buffer, w.lithosphere_ptr(), cells),
      sediment: new Float32Array(buffer, w.sediment_ptr(), cells),
      heatFlow: new Float32Array(buffer, w.heat_flow_ptr(), cells),
      mantleTemp: new Float32Array(buffer, w.mantle_temp_ptr(), cells),
      lowerSlab: new Float32Array(buffer, w.lower_slab_ptr(), cells),
      dynTopo: new Float32Array(buffer, w.dyn_topo_ptr(), cells),
      cellKind: new Uint8Array(buffer, w.cell_kind_ptr(), cells),
      arcKind: new Int8Array(buffer, w.arc_kind_ptr(), arcs),
    };
  }

  saveState(): Uint8Array {
    return this.world.save_state();
  }

  plateCount(): number {
    return this.world.plate_count();
  }

  plateOmegas(): Float64Array {
    return this.world.plate_omegas();
  }

  metrics(): WorldMetrics {
    return this.world.metrics() as WorldMetrics;
  }

  summaries(): PlateSummary[] {
    return this.world.summaries() as PlateSummary[];
  }

  mass(): MassReadout {
    return { liveMass: this.world.live_mass(), drift: this.world.mass_drift() };
  }

  passTimings(): Float64Array {
    return this.world.pass_timings();
  }

  dispose(): void {
    this.world.free();
  }
}
