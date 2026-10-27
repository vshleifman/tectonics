import * as THREE from "three";
import { clamp } from "../math/scalar";
import { cross, dot, length, normalise, sub, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import { BoundaryKind } from "../sim/boundaries";
import {
  type ColourFields,
  type ColourMode,
  type ElevationStyle,
  type Rgb,
  writeCellColours,
} from "./colour";
import { type HillshadeOptions, writeHillshade } from "./hillshade";
import { longitudeOf, type Projection, projectPoint } from "./projection";

export type { ColourMode } from "./colour";

/**
 * How far each overlay floats above the faces. On the sphere this scales the
 * radius (`1 + lift`); on the Mercator map it becomes a small +z offset toward
 * the camera. Edges sit just above the faces, faults above the wireframe, and
 * velocity arrows above everything else so they stay readable.
 */
const EDGE_LIFT = 0.003;
const FAULT_LIFT = 0.005;
const VELOCITY_LIFT = 0.012;
/**
 * Plate boundaries sit just above the coloured fill ({@link PLATE_LIFT}) and
 * the fault lines ({@link FAULT_LIFT}), and below the velocity arrows, so a
 * boundary stays readable and does not z-fight the fault drawn on the same arc.
 */
const PLATE_LINE_LIFT = 0.008;

/** How the elevation view shows relief, on top of its colours. */
export interface ReliefOptions extends HillshadeOptions {
  hillshade: boolean;
}

/**
 * How far the semi-transparent plates overlay floats above the faces. On the
 * sphere it scales the radius (the mesh is uniformly scaled by `1 + lift`); on
 * the flat Mercator map it becomes a small +z offset toward the camera. Kept
 * below {@link VELOCITY_LIFT} so arrows still read on top of it.
 */
const PLATE_LIFT = 0.006;

/**
 * Velocity arrows are drawn in pairs along the plate boundaries rather than as
 * one arrow per plate, the way a tectonic map does it: what a reader wants to
 * see is how the two plates move *at the line where they meet*, so that a ridge
 * reads as arrows flying apart and a trench as arrows driving together.
 *
 * Each arrow of a pair belongs to one plate and is kept wholly inside it, hard
 * against the boundary — an arrow that crossed the line would be claiming motion
 * for crust it does not belong to, and at a convergent boundary the two arrows
 * of a pair would run through each other.
 *
 * Boundaries are thousands of arcs long, so pairs are thinned to roughly one per
 * {@link ARROW_SPACING} of arc: dense enough to follow a boundary along its
 * length, sparse enough to stay readable.
 */

/** Minimum angular separation (radians) between neighbouring arrow pairs. */
const ARROW_SPACING = 0.18;

/** Clearance (world units) between the boundary and the nearest arrow corner. */
const ARROW_OFFSET = 0.008;

/**
 * Arrow length (world units) per unit surface speed (rad/Myr). Chosen so the
 * few-cm/yr band real plates live in — and the model reproduces — spreads across
 * most of the length range below, rather than every arrow pinning to one bound.
 *
 * Arrows are laid out in *projected* space, so these lengths are world units in
 * whichever projection is active, not radians of arc. That is deliberate: the
 * globe is 2 units across and so is the Mercator map, so one set of numbers
 * gives arrows the same on-screen size in both views. Building them on the
 * sphere and projecting afterwards instead shrank every map arrow by the ~pi
 * the Mercator x-axis packs a full turn of longitude into.
 */
const VELOCITY_SCALE = 15;

/** Arrow length bounds (world units): slow arrows stay visible, fast ones readable. */
const MIN_ARROW_LENGTH = 0.028;
const MAX_ARROW_LENGTH = 0.12;

/** Surface speed below which a plate is treated as still and gets no arrow. */
const STILL_SPEED = 1e-6;

/**
 * Arrow proportions, all as fractions of its length, so an arrow keeps its shape
 * at any speed. Arrows are filled triangles rather than lines because WebGL
 * ignores line widths beyond one pixel, and a one-pixel arrow is lost against
 * the plate overlay.
 */
const HEAD_FRACTION = 0.36;
const HEAD_HALF_WIDTH = 0.22;
const SHAFT_HALF_WIDTH = 0.09;

/**
 * Width (world units) of the black silhouette drawn behind each arrow, so an
 * arrow stays readable wherever it lands — a pale ice shelf, a bright ridge, or
 * a plate overlay in nearly its own colour.
 */
const OUTLINE_WIDTH = 0.0035;

/** Triangles, and so vertices, one arrow contributes: a shaft quad plus a head. */
const ARROW_VERTICES = 9;

/**
 * Arrow tint by boundary kind, brighter than the boundary *view*'s cell palette
 * so arrows stay legible when drawn on top of it: convergent red, divergent
 * blue, transform yellow.
 */
const arrowColour = (kind: number): Rgb => {
  switch (kind) {
    case BoundaryKind.CONVERGENT:
      return [1, 0.42, 0.34];
    case BoundaryKind.DIVERGENT:
      return [0.42, 0.72, 1];
    default:
      return [1, 0.86, 0.36];
  }
};

/**
 * An arrow's outline, in units of its own length: `back` and `tip` are where it
 * starts and ends along its heading, `neck` is where the shaft meets the head,
 * and `shaft` / `head` are the two half-widths. Growing all five by one margin
 * gives the silhouette drawn behind the arrow.
 */
interface ArrowShape {
  back: number;
  tip: number;
  neck: number;
  shaft: number;
  head: number;
}

/** The corners of an arrow shape, as (along, sideways) length fractions. */
const arrowCorners = (s: ArrowShape): ReadonlyArray<[number, number]> => [
  [s.back, s.shaft],
  [s.back, -s.shaft],
  [s.neck, s.head],
  [s.neck, -s.head],
  [s.tip, 0],
];

/**
 * The two layers of the arrow overlay, accumulated separately so that every
 * outline is drawn before every fill. They have to be layered by draw order
 * rather than by depth: the arrows write no depth (see the material), so within
 * the overlay the painter's order is the only thing separating them.
 */
interface ArrowLayers {
  outline: number[];
  fill: number[];
  fillColours: number[];
}

/** One boundary site an arrow pair is drawn at, already taken into projected space. */
interface ArrowSite {
  /** The site on the unit sphere, where plate velocities are sampled. */
  mid: Vec3;
  /** The site in projected space: where the pair's tails sit. */
  anchor: Vec3;
  /** Unit across-boundary axis in projected space, cell A -> cell B. */
  across: Vec3;
  /** Longitude the whole pair is unwrapped against, to survive the map seam. */
  refLon: number;
  /** Tint for both arrows, from the boundary's kind. */
  colour: Rgb;
}

/**
 * Anything that can say how fast a plate's surface moves at a point: the live
 * plate set in the simulation, or the per-plate angular velocities a snapshot
 * carries to the main thread.
 */
export interface PlateVelocities {
  linearVelocity(id: number, pos: Vec3): Vec3;
}

/** Width of the per-cell colour textures; rows are added as cells need them. */
const COLOUR_TEXTURE_WIDTH = 2048;

/**
 * A one-texel-per-cell RGBA8 colour table. At level 8 that is ~2.6 MB per
 * upload, against ~55 MB for a float colour per fan vertex.
 */
const makeColourTexture = (cellCount: number): THREE.DataTexture => {
  const width = Math.min(COLOUR_TEXTURE_WIDTH, Math.max(1, cellCount));
  const height = Math.ceil(cellCount / width);
  const texture = new THREE.DataTexture(
    new Uint8Array(width * height * 4),
    width,
    height,
    THREE.RGBAFormat,
    THREE.UnsignedByteType,
  );
  texture.minFilter = THREE.NearestFilter;
  texture.magFilter = THREE.NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
};

/**
 * Colours each fan vertex from its cell's texel. The lookup happens per vertex
 * so every vertex of a cell reads the same texel and faces stay flat. The
 * ramps are authored as linear colours, exactly as the vertex-colour material
 * treated them, so the output encoding is applied the same way.
 */
const makeCellMaterial = (
  texture: THREE.DataTexture,
  overlay: { opacity: number } | null,
): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    uniforms: {
      colours: { value: texture },
      texWidth: { value: texture.image.width },
      opacity: { value: overlay ? overlay.opacity : 1 },
    },
    vertexShader: /* glsl */ `
      attribute float cellIndex;
      uniform sampler2D colours;
      uniform float texWidth;
      varying vec3 vColour;
      void main() {
        int i = int(cellIndex + 0.5);
        int w = int(texWidth);
        vColour = texelFetch(colours, ivec2(i % w, i / w), 0).rgb;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */ `
      uniform float opacity;
      varying vec3 vColour;
      void main() {
        gl_FragColor = linearToOutputTexel(vec4(vColour, opacity));
      }
    `,
    side: THREE.DoubleSide,
    transparent: overlay !== null,
    depthWrite: overlay === null,
  });

/**
 * Builds a single static Three.js mesh for the whole cell grid and exposes a
 * cheap colour-only update path.
 *
 * Each cell is a triangle fan from its centre to its ordered corners, using its
 * OWN copy of every vertex (no sharing across cells) so faces stay flat and
 * colours never bleed across cell borders. Geometry is built once; recolouring
 * only rewrites a one-texel-per-cell colour texture, which the shader looks up
 * by each vertex's cell index.
 */
export class CellRenderer {
  /** Group holding the filled faces, the faint wireframe, faults and seeds. */
  readonly object3D = new THREE.Group();

  private readonly geometry: THREE.BufferGeometry;
  private readonly material: THREE.ShaderMaterial;
  private readonly colourTexture: THREE.DataTexture;

  /** Face vertex positions precomputed for both projections; swapped on toggle. */
  private readonly spherePositions: Float32Array;
  private readonly mercatorPositions: Float32Array;

  private readonly edgeGeometry: THREE.BufferGeometry;
  private readonly edgeMaterial: THREE.LineBasicMaterial;
  private readonly edges: THREE.LineSegments;

  /** Edge segment positions precomputed for both projections; swapped on toggle. */
  private readonly sphereEdgePositions: Float32Array;
  private readonly mercatorEdgePositions: Float32Array;

  /** Active projection. Overlays are re-derived through it on every update. */
  private projection: Projection = "sphere";

  /** Last overlay inputs, cached so a projection switch can re-place them. */
  private crackedCache: Uint8Array | null = null;
  private seedsCache: readonly number[] | null = null;
  private velocityPlates: PlateVelocities | null = null;
  private velocityPlateId: ArrayLike<number> | null = null;
  private velocityArcKind: ArrayLike<number> | null = null;
  /** True when the arrows' inputs changed while they were hidden. */
  private velocitiesStale = false;

  /** Bright overlay drawing only the cracked arcs (the fault network). */
  private readonly faultGeometry: THREE.BufferGeometry;
  private readonly faultMaterial: THREE.LineBasicMaterial;
  private readonly faultLines: THREE.LineSegments;
  private readonly faultPositions: Float32Array;

  /**
   * Light overlay of the arcs that separate two plates. Preallocated to one
   * segment per arc, like the faults, and redrawn from {@link plateIdCache}
   * when the projection changes.
   */
  private readonly plateLineGeometry: THREE.BufferGeometry;
  private readonly plateLineMaterial: THREE.LineBasicMaterial;
  private readonly plateLines: THREE.LineSegments;
  private readonly plateLinePositions: Float32Array;
  private plateIdCache: ArrayLike<number> | null = null;

  /** Markers at the seed junctions the player placed. */
  private readonly seedGeometry: THREE.BufferGeometry;
  private readonly seedMaterial: THREE.PointsMaterial;
  private readonly seedPoints: THREE.Points;

  /** Arrow pairs straddling each plate boundary, showing both sides' motion. */
  private readonly velocityGeometry: THREE.BufferGeometry;
  private readonly velocityMaterial: THREE.MeshBasicMaterial;
  private readonly velocityArrows: THREE.Mesh;

  /**
   * Semi-transparent plate-coloured overlay drawn over the base faces. Toggled
   * independently of the base view so any view (elevation, density, ...) can be
   * seen with plate regions blended on top. Shares the face geometry outright;
   * only its colour texture differs.
   */
  private readonly platesMaterial: THREE.ShaderMaterial;
  private readonly platesMesh: THREE.Mesh;
  private readonly platesTexture: THREE.DataTexture;

  private readonly cellMesh: CellMesh;

  /** Per-cell hillshade multipliers, allocated on first use. */
  private shadeBuffer: Float32Array | undefined;

  constructor(cellMesh: CellMesh) {
    this.cellMesh = cellMesh;
    const { cellCount } = cellMesh;

    // Count vertices/triangles: per cell = 1 centre + C corners, C triangles.
    const cellVertexStart = new Int32Array(cellCount + 1);
    let triangleCount = 0;
    let cornerTotal = 0;
    for (let i = 0; i < cellCount; i++) {
      const corners = cellMesh.polygon(i).length;
      cellVertexStart[i + 1] = cellVertexStart[i] + corners + 1;
      triangleCount += corners;
      cornerTotal += corners;
    }
    const vertexCount = cellVertexStart[cellCount];

    const positions = new Float32Array(vertexCount * 3);
    const mercatorPositions = new Float32Array(vertexCount * 3);
    const cellIndex = new Float32Array(vertexCount);
    for (let i = 0; i < cellCount; i++) {
      cellIndex.fill(i, cellVertexStart[i], cellVertexStart[i + 1]);
    }
    const indices = new Uint32Array(triangleCount * 3);

    // Edge line segments: one per polygon side, both endpoints per segment.
    const edgePositions = new Float32Array(cornerTotal * 2 * 3);
    const mercatorEdgePositions = new Float32Array(cornerTotal * 2 * 3);
    let edgeVertex = 0;

    const writeTriple = (
      target: Float32Array,
      index: number,
      p: readonly [number, number, number],
    ): void => {
      target[index * 3] = p[0];
      target[index * 3 + 1] = p[1];
      target[index * 3 + 2] = p[2];
    };

    let indexWrite = 0;
    for (let i = 0; i < cellCount; i++) {
      const centre = cellMesh.position(i);
      const corners = cellMesh.polygon(i);
      const base = cellVertexStart[i];
      // Unwrap every corner of this cell to the centre longitude so the
      // polygon stays contiguous across the antimeridian seam on the map.
      const refLon = longitudeOf(centre[0], centre[1], centre[2]);

      // Vertex 0 of the cell is the centre; 1..C are the corners.
      writeTriple(positions, base, centre);
      writeTriple(
        mercatorPositions,
        base,
        projectPoint(centre[0], centre[1], centre[2], "mercator", 0, refLon),
      );
      for (let k = 0; k < corners.length; k++) {
        const v = base + 1 + k;
        writeTriple(positions, v, corners[k]);
        writeTriple(
          mercatorPositions,
          v,
          projectPoint(
            corners[k][0],
            corners[k][1],
            corners[k][2],
            "mercator",
            0,
            refLon,
          ),
        );
      }

      // Fan triangles: (centre, corner k, corner k+1).
      for (let k = 0; k < corners.length; k++) {
        const next = (k + 1) % corners.length;
        indices[indexWrite++] = base;
        indices[indexWrite++] = base + 1 + k;
        indices[indexWrite++] = base + 1 + next;

        // Boundary segment corner[k] -> corner[next], lifted off the faces.
        const a = corners[k];
        const b = corners[next];
        writeTriple(
          edgePositions,
          edgeVertex,
          projectPoint(a[0], a[1], a[2], "sphere", EDGE_LIFT),
        );
        writeTriple(
          mercatorEdgePositions,
          edgeVertex,
          projectPoint(a[0], a[1], a[2], "mercator", EDGE_LIFT, refLon),
        );
        edgeVertex++;
        writeTriple(
          edgePositions,
          edgeVertex,
          projectPoint(b[0], b[1], b[2], "sphere", EDGE_LIFT),
        );
        writeTriple(
          mercatorEdgePositions,
          edgeVertex,
          projectPoint(b[0], b[1], b[2], "mercator", EDGE_LIFT, refLon),
        );
        edgeVertex++;
      }
    }

    this.spherePositions = positions;
    this.mercatorPositions = mercatorPositions;
    this.sphereEdgePositions = edgePositions;
    this.mercatorEdgePositions = mercatorEdgePositions;

    this.geometry = new THREE.BufferGeometry();
    this.geometry.setAttribute(
      "position",
      new THREE.BufferAttribute(positions.slice(), 3),
    );
    this.geometry.setAttribute(
      "cellIndex",
      new THREE.BufferAttribute(cellIndex, 1),
    );
    this.geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    this.geometry.computeBoundingSphere();

    // Double-sided: cell winding faces outward on the sphere, which flips to
    // back-facing once flattened onto the Mercator plane under a top-down camera.
    this.colourTexture = makeColourTexture(cellCount);
    this.material = makeCellMaterial(this.colourTexture, null);
    const mesh = new THREE.Mesh(this.geometry, this.material);

    this.edgeGeometry = new THREE.BufferGeometry();
    this.edgeGeometry.setAttribute(
      "position",
      new THREE.BufferAttribute(edgePositions.slice(), 3),
    );
    this.edgeMaterial = new THREE.LineBasicMaterial({ color: 0x000000 });
    this.edges = new THREE.LineSegments(this.edgeGeometry, this.edgeMaterial);
    this.edges.visible = false;
    this.edges.frustumCulled = false;

    // Fault overlay: preallocate room for every arc, draw only cracked ones.
    const graph = cellMesh.boundaryGraph();
    this.faultPositions = new Float32Array(graph.arcCount * 2 * 3);
    this.faultGeometry = new THREE.BufferGeometry();
    this.faultGeometry.setAttribute(
      "position",
      new THREE.BufferAttribute(this.faultPositions, 3),
    );
    this.faultGeometry.setDrawRange(0, 0);
    this.faultMaterial = new THREE.LineBasicMaterial({ color: 0xff5a3c });
    this.faultLines = new THREE.LineSegments(
      this.faultGeometry,
      this.faultMaterial,
    );
    this.faultLines.visible = false;
    this.faultLines.frustumCulled = false;

    this.plateLinePositions = new Float32Array(graph.arcCount * 2 * 3);
    this.plateLineGeometry = new THREE.BufferGeometry();
    this.plateLineGeometry.setAttribute(
      "position",
      new THREE.BufferAttribute(this.plateLinePositions, 3),
    );
    this.plateLineGeometry.setDrawRange(0, 0);
    this.plateLineMaterial = new THREE.LineBasicMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: 0.75,
      depthWrite: false,
    });
    this.plateLines = new THREE.LineSegments(
      this.plateLineGeometry,
      this.plateLineMaterial,
    );
    this.plateLines.visible = false;
    this.plateLines.frustumCulled = false;
    // Above the plates fill (renderOrder 1). Transparent so it joins the queue
    // drawn after the opaque faults and is not painted over by the fill.
    this.plateLines.renderOrder = 2;

    this.seedGeometry = new THREE.BufferGeometry();
    this.seedGeometry.setAttribute(
      "position",
      new THREE.BufferAttribute(new Float32Array(0), 3),
    );
    this.seedMaterial = new THREE.PointsMaterial({
      color: 0xffe14d,
      size: 9,
      sizeAttenuation: false,
    });
    this.seedPoints = new THREE.Points(this.seedGeometry, this.seedMaterial);
    this.seedPoints.visible = false;
    this.seedPoints.frustumCulled = false;

    this.velocityGeometry = new THREE.BufferGeometry();
    this.velocityGeometry.setAttribute(
      "position",
      new THREE.BufferAttribute(new Float32Array(0), 3),
    );
    this.velocityGeometry.setAttribute(
      "color",
      new THREE.BufferAttribute(new Float32Array(0), 3),
    );
    // `transparent` at full opacity looks pointless but is what puts the arrows
    // in Three's transparent queue, which is drawn after the opaque one and
    // sorted by renderOrder. Left opaque they would be painted over by the
    // (transparent) plates overlay and come out washed to a pastel.
    this.velocityMaterial = new THREE.MeshBasicMaterial({
      vertexColors: true,
      side: THREE.DoubleSide,
      transparent: true,
      depthWrite: false,
    });
    this.velocityArrows = new THREE.Mesh(
      this.velocityGeometry,
      this.velocityMaterial,
    );
    this.velocityArrows.visible = false;
    this.velocityArrows.frustumCulled = false;
    // Above the plates fill (1) and the plate boundaries (2).
    this.velocityArrows.renderOrder = 3;

    // Plates overlay: the base faces again, with their own colour texture,
    // blended semi-transparently on top. Lifted off the faces (radial scale on
    // the sphere, +z on the map; applied in setProjection).
    this.platesTexture = makeColourTexture(cellCount);
    this.platesMaterial = makeCellMaterial(this.platesTexture, {
      opacity: 0.7,
    });
    this.platesMesh = new THREE.Mesh(this.geometry, this.platesMaterial);
    this.platesMesh.visible = false;
    this.platesMesh.frustumCulled = false;
    this.platesMesh.renderOrder = 1;
    this.platesMesh.scale.setScalar(1 + PLATE_LIFT);

    this.object3D.add(
      mesh,
      this.edges,
      this.faultLines,
      this.seedPoints,
      this.velocityArrows,
      this.platesMesh,
      this.plateLines,
    );
  }

  /**
   * Rewrite the colour buffer for the given mode. Cheap: no geometry rebuild,
   * just one colour attribute upload.
   *
   * - `"elevation"`: ocean/land ramp split at sea level, 0. `"land"` flattens
   *   the ocean to one colour and stretches the continental colours.
   *   With `hillshade`, relief is lit from the north-west (elevation only).
   * - `"data"`: flat white faces (the black wireframe carries the structure).
   * - `"plate"`: a stable per-`plateId` hue so plates read as solid regions.
   * - `"crust"`: oceanic vs continental two-tone.
   * - `"density"`: a light-to-dark ramp over the crust density range.
   * - `"lithosphere"`: thickness of the cold mantle lid under the crust.
   * - `"sediment"`: share of the crust that is reworked sediment.
   * - `"heat-flow"`: surface heat loss from cold old plates to hot ridges.
   * - `"mantle-temp"`: upper-mantle anomaly, cold under trenches, hot under continents.
   * - `"slab-graveyard"`: slab piled in the lower mantle beneath old trenches.
   * - `"dyn-topo"`: the mantle's lift over hot upwellings and drag over downwellings.
   * - `"boundary"`: each cell tinted by its dominant boundary type (needs
   *   `boundaryKind`, the per-cell classification from {@link classifyBoundaries}).
   */
  updateColours(
    data: ColourFields,
    mode: ColourMode = "elevation",
    boundaryKind?: ArrayLike<number>,
    elevationStyle: ElevationStyle = "absolute",
    relief?: ReliefOptions,
  ): void {
    let shade: Float32Array | undefined;
    if (relief?.hillshade && mode === "elevation") {
      shade = this.shadeBuffer ??= new Float32Array(this.cellMesh.cellCount);
      writeHillshade(shade, this.cellMesh, data.elevation, relief);
    }
    writeCellColours(
      this.colourTexture.image.data as Uint8Array,
      this.cellMesh.cellCount,
      data,
      mode,
      boundaryKind,
      elevationStyle,
      shade,
    );
    this.colourTexture.needsUpdate = true;
  }

  /**
   * Rewrite the plates overlay colours from the per-cell plate ids, using the
   * same stable per-plate hue as the old "plate" view. Cheap: one small texture
   * upload, no geometry rebuild. Call whenever plate ids change.
   */
  updatePlateOverlay(data: ColourFields): void {
    writeCellColours(
      this.platesTexture.image.data as Uint8Array,
      this.cellMesh.cellCount,
      data,
      "plate",
    );
    this.platesTexture.needsUpdate = true;
  }

  /** Rebuild the bright fault overlay from the cracked-arc flags. */
  updateFaults(cracked: Uint8Array): void {
    this.crackedCache = cracked;
    const graph = this.cellMesh.boundaryGraph();
    const { arcEnds, junctionPos } = graph;
    let w = 0;
    for (let a = 0; a < graph.arcCount; a++) {
      if (!cracked[a]) continue;
      const j0 = arcEnds[a * 2];
      const j1 = arcEnds[a * 2 + 1];
      const x0 = junctionPos[j0 * 3];
      const y0 = junctionPos[j0 * 3 + 1];
      const z0 = junctionPos[j0 * 3 + 2];
      // Keep the arc contiguous: unwrap its far end to the near end's longitude.
      const refLon = longitudeOf(x0, y0, z0);
      const p0 = projectPoint(x0, y0, z0, this.projection, FAULT_LIFT, refLon);
      const p1 = projectPoint(
        junctionPos[j1 * 3],
        junctionPos[j1 * 3 + 1],
        junctionPos[j1 * 3 + 2],
        this.projection,
        FAULT_LIFT,
        refLon,
      );
      this.faultPositions[w++] = p0[0];
      this.faultPositions[w++] = p0[1];
      this.faultPositions[w++] = p0[2];
      this.faultPositions[w++] = p1[0];
      this.faultPositions[w++] = p1[1];
      this.faultPositions[w++] = p1[2];
    }
    this.faultGeometry.setDrawRange(0, w / 3);
    const attr = this.faultGeometry.getAttribute(
      "position",
    ) as THREE.BufferAttribute;
    attr.needsUpdate = true;
  }

  /**
   * Rebuild the plate-boundary overlay: one segment for each arc whose two
   * cells belong to different plates. Endpoints are projected the same way as
   * the faults, at {@link PLATE_LINE_LIFT}.
   */
  updatePlateLines(plateId: ArrayLike<number>): void {
    this.plateIdCache = plateId;
    const graph = this.cellMesh.boundaryGraph();
    const { arcEnds, arcCells, junctionPos } = graph;
    let w = 0;
    for (let a = 0; a < graph.arcCount; a++) {
      const cellA = arcCells[a * 2];
      const cellB = arcCells[a * 2 + 1];
      if (cellA < 0 || cellB < 0) continue;
      if (plateId[cellA] === plateId[cellB]) continue;
      const j0 = arcEnds[a * 2];
      const j1 = arcEnds[a * 2 + 1];
      const x0 = junctionPos[j0 * 3];
      const y0 = junctionPos[j0 * 3 + 1];
      const z0 = junctionPos[j0 * 3 + 2];
      const refLon = longitudeOf(x0, y0, z0);
      const p0 = projectPoint(
        x0,
        y0,
        z0,
        this.projection,
        PLATE_LINE_LIFT,
        refLon,
      );
      const p1 = projectPoint(
        junctionPos[j1 * 3],
        junctionPos[j1 * 3 + 1],
        junctionPos[j1 * 3 + 2],
        this.projection,
        PLATE_LINE_LIFT,
        refLon,
      );
      this.plateLinePositions[w++] = p0[0];
      this.plateLinePositions[w++] = p0[1];
      this.plateLinePositions[w++] = p0[2];
      this.plateLinePositions[w++] = p1[0];
      this.plateLinePositions[w++] = p1[1];
      this.plateLinePositions[w++] = p1[2];
    }
    this.plateLineGeometry.setDrawRange(0, w / 3);
    const attr = this.plateLineGeometry.getAttribute(
      "position",
    ) as THREE.BufferAttribute;
    attr.needsUpdate = true;
  }

  /** Rebuild the seed markers from the placed seed junctions. */
  updateSeeds(seeds: readonly number[]): void {
    this.seedsCache = seeds;
    const { junctionPos } = this.cellMesh.boundaryGraph();
    const positions = new Float32Array(seeds.length * 3);
    for (let s = 0; s < seeds.length; s++) {
      const j = seeds[s];
      const p = projectPoint(
        junctionPos[j * 3],
        junctionPos[j * 3 + 1],
        junctionPos[j * 3 + 2],
        this.projection,
        FAULT_LIFT,
      );
      positions[s * 3] = p[0];
      positions[s * 3 + 1] = p[1];
      positions[s * 3 + 2] = p[2];
    }
    this.seedGeometry.setAttribute(
      "position",
      new THREE.BufferAttribute(positions, 3),
    );
    this.seedGeometry.getAttribute("position").needsUpdate = true;
  }

  /**
   * Rebuild the plate-velocity arrows along the boundaries.
   *
   * Every arc separating two different plates is a candidate site; sites are
   * thinned to roughly one per {@link ARROW_SPACING} so a boundary carries a
   * readable row of arrows rather than one per cell. At each surviving site a
   * *pair* of arrows is drawn, one just to either side of the line, each
   * pointing along its own plate's surface velocity there and scaled by that
   * plate's speed. Divergent boundaries therefore read as arrows flying apart,
   * convergent ones as arrows driving together, and transform ones as arrows
   * sliding past — the same reading as a printed tectonic map.
   *
   * Arrows are filled triangles (a shaft quad plus a head) tinted by boundary
   * kind and pushed just above the surface.
   */
  updateVelocities(
    plates: PlateVelocities,
    plateId: ArrayLike<number>,
    arcKind: ArrayLike<number> | null,
  ): void {
    this.velocityPlates = plates;
    this.velocityPlateId = plateId;
    this.velocityArcKind = arcKind;
    // Hidden arrows are rebuilt when they are shown again, not every tick.
    if (!this.velocityArrows.visible) {
      this.velocitiesStale = true;
      return;
    }
    this.velocitiesStale = false;

    const layers: ArrowLayers = { outline: [], fill: [], fillColours: [] };

    if (arcKind) {
      const graph = this.cellMesh.boundaryGraph();
      const { arcCells, arcCount } = graph;
      const { arcMid, arcNormal } = this.cellMesh;
      const thinner = new SphereThinner(ARROW_SPACING);

      for (let a = 0; a < arcCount; a++) {
        const kind = arcKind[a];
        if (kind === BoundaryKind.NONE) continue;
        const cellA = arcCells[a * 2];
        const cellB = arcCells[a * 2 + 1];
        if (cellA < 0 || cellB < 0) continue;

        const mid: Vec3 = [arcMid[a * 3], arcMid[a * 3 + 1], arcMid[a * 3 + 2]];
        if (!thinner.accept(mid)) continue;

        // Across-boundary unit normal at the midpoint, cell A -> cell B:
        // the axis each arrow of the pair is offset along.
        const normal: Vec3 = [
          arcNormal[a * 3],
          arcNormal[a * 3 + 1],
          arcNormal[a * 3 + 2],
        ];
        const colour = arrowColour(kind);
        // Keep the whole pair on one side of the map seam.
        const refLon = longitudeOf(mid[0], mid[1], mid[2]);

        // The site, and the offset axis, taken into projected space once for
        // the pair: both arrows share them.
        const anchor = this.project(mid, refLon);
        const across = this.projectDirection(mid, normal, anchor, refLon);
        if (!across) continue; // degenerate at the Mercator pole clamp

        const site: ArrowSite = { mid, anchor, across, refLon, colour };
        // `across` runs cell A -> cell B, so A's own side of the line is -1.
        this.pushArrow(layers, site, -1, plateId[cellA], plates);
        this.pushArrow(layers, site, 1, plateId[cellB], plates);
      }
    }

    const positions = new Float32Array(
      layers.outline.length + layers.fill.length,
    );
    positions.set(layers.outline);
    positions.set(layers.fill, layers.outline.length);
    // The outline half is left at zero, which is exactly the black it wants.
    const colours = new Float32Array(positions.length);
    colours.set(layers.fillColours, layers.outline.length);

    this.velocityGeometry.setAttribute(
      "position",
      new THREE.BufferAttribute(positions, 3),
    );
    this.velocityGeometry.setAttribute(
      "color",
      new THREE.BufferAttribute(colours, 3),
    );
  }

  /** The active projection of a unit-sphere point, lifted clear of the faces. */
  private project(p: Vec3, refLon: number): Vec3 {
    return projectPoint(
      p[0],
      p[1],
      p[2],
      this.projection,
      VELOCITY_LIFT,
      refLon,
    );
  }

  /**
   * The unit direction, in projected space, that a tangent direction `dir` at
   * unit-sphere point `p` maps to. Taken as the projection of a short step along
   * `dir`, so it picks up whatever local rotation and stretch the projection
   * applies — a plate heading due north-east on the globe still reads as
   * north-east on the map. `null` where the step projects to nothing, which the
   * Mercator latitude clamp can do at the poles.
   */
  private projectDirection(
    p: Vec3,
    dir: Vec3,
    from: Vec3,
    refLon: number,
  ): Vec3 | null {
    const step = 1e-3;
    const ahead = this.project(
      normalise([
        p[0] + dir[0] * step,
        p[1] + dir[1] * step,
        p[2] + dir[2] * step,
      ]),
      refLon,
    );
    const delta = sub(ahead, from);
    return length(delta) < 1e-12 ? null : normalise(delta);
  }

  /**
   * Append one arrow for `plateId` at a boundary `site`, pointing along that
   * plate's surface velocity there. `sign` says which side of the line the plate
   * lies on (-1 for the arc's first cell, +1 for its second). A plate that is
   * effectively still contributes nothing.
   *
   * The arrow is slid along the inward normal until its nearest corner — its
   * outline's, not its fill's — clears the boundary by {@link ARROW_OFFSET}, so
   * it always sits wholly inside its own plate. A plate driving *into* the
   * boundary therefore shows its tip at the line with the shaft trailing back;
   * one pulling away shows its tail at the line. Neither ever reaches across
   * into the neighbouring plate.
   */
  private pushArrow(
    layers: ArrowLayers,
    site: ArrowSite,
    sign: number,
    plateId: number,
    plates: PlateVelocities,
  ): void {
    if (plateId < 0) return;
    const { mid, anchor, across, refLon, colour } = site;

    const v = plates.linearVelocity(plateId, mid);
    const speed = Math.hypot(v[0], v[1], v[2]);
    if (speed < STILL_SPEED) return;

    // Heading in projected space, and a perpendicular to it in the drawing
    // plane (the sphere's tangent plane, or the flat map) for the arrow's width.
    const dir = this.projectDirection(
      mid,
      [v[0] / speed, v[1] / speed, v[2] / speed],
      anchor,
      refLon,
    );
    if (!dir) return;
    const plane: Vec3 =
      this.projection === "sphere" ? normalise(anchor) : [0, 0, 1];
    const side = normalise(cross(plane, dir));

    const size = clamp(
      speed * VELOCITY_SCALE,
      MIN_ARROW_LENGTH,
      MAX_ARROW_LENGTH,
    );

    // Shapes: the fill, and the outline as the fill grown by a constant
    // world-unit margin (hence divided by `size`, which the shape is scaled by).
    const neck = 1 - HEAD_FRACTION;
    const margin = OUTLINE_WIDTH / size;
    const fill: ArrowShape = {
      back: 0,
      tip: 1,
      neck,
      shaft: SHAFT_HALF_WIDTH,
      head: HEAD_HALF_WIDTH,
    };
    const outline: ArrowShape = {
      back: -margin,
      tip: 1 + margin,
      neck,
      shaft: SHAFT_HALF_WIDTH + margin,
      head: HEAD_HALF_WIDTH + margin,
    };

    // How far each outline corner reaches toward the plate's own interior, and
    // so how far the whole arrow must be pushed in to clear the boundary.
    const inward: Vec3 = [across[0] * sign, across[1] * sign, across[2] * sign];
    const alongInward = dot(dir, inward);
    const sideInward = dot(side, inward);
    let nearest = Number.POSITIVE_INFINITY;
    for (const [along, sideways] of arrowCorners(outline)) {
      nearest = Math.min(nearest, along * alongInward + sideways * sideInward);
    }
    const push = ARROW_OFFSET - nearest * size;

    const origin: Vec3 = [
      anchor[0] + inward[0] * push,
      anchor[1] + inward[1] * push,
      anchor[2] + inward[2] * push,
    ];

    emitArrow(layers.outline, origin, dir, side, size, outline);
    emitArrow(layers.fill, origin, dir, side, size, fill);
    for (let i = 0; i < ARROW_VERTICES; i++) layers.fillColours.push(...colour);
  }

  /**
   * Switch the projection the scene is drawn in. Swaps the precomputed face
   * and edge buffers and re-derives the dynamic overlays (faults, seeds,
   * velocity arrows) from their cached inputs so the map shows exactly what
   * the sphere showed. Colours are untouched.
   */
  setProjection(projection: Projection): void {
    if (projection === this.projection) return;
    this.projection = projection;

    const facePositions =
      projection === "sphere" ? this.spherePositions : this.mercatorPositions;
    const faceAttr = this.geometry.getAttribute(
      "position",
    ) as THREE.BufferAttribute;
    (faceAttr.array as Float32Array).set(facePositions);
    faceAttr.needsUpdate = true;
    this.geometry.computeBoundingSphere();

    // The plates overlay shares the face geometry; lift it off the faces with
    // a radial scale on the sphere and a flat +z nudge on the map.
    if (projection === "sphere") {
      this.platesMesh.scale.setScalar(1 + PLATE_LIFT);
      this.platesMesh.position.set(0, 0, 0);
    } else {
      this.platesMesh.scale.setScalar(1);
      this.platesMesh.position.set(0, 0, PLATE_LIFT);
    }

    const edgePositions =
      projection === "sphere"
        ? this.sphereEdgePositions
        : this.mercatorEdgePositions;
    const edgeAttr = this.edgeGeometry.getAttribute(
      "position",
    ) as THREE.BufferAttribute;
    (edgeAttr.array as Float32Array).set(edgePositions);
    edgeAttr.needsUpdate = true;

    if (this.crackedCache) this.updateFaults(this.crackedCache);
    if (this.plateIdCache) this.updatePlateLines(this.plateIdCache);
    if (this.seedsCache) this.updateSeeds(this.seedsCache);
    this.refreshVelocities();
  }

  /** Rebuild the arrows from their cached inputs. */
  private refreshVelocities(): void {
    if (this.velocityPlates && this.velocityPlateId) {
      this.updateVelocities(
        this.velocityPlates,
        this.velocityPlateId,
        this.velocityArcKind,
      );
    }
  }

  /** Show or hide the plate-velocity arrows. */
  setVelocitiesVisible(visible: boolean): void {
    this.velocityArrows.visible = visible;
    if (visible && this.velocitiesStale) this.refreshVelocities();
  }

  /** Show or hide the semi-transparent plates overlay. */
  setPlatesVisible(visible: boolean): void {
    this.platesMesh.visible = visible;
  }

  /** Show or hide the plate-boundary lines. */
  setPlateLinesVisible(visible: boolean): void {
    this.plateLines.visible = visible;
  }

  /** Show or hide the black cell-boundary lines (used in data mode). */
  setEdgesVisible(visible: boolean): void {
    this.edges.visible = visible;
  }

  /** Show or hide the fault overlay and seed markers together. */
  setFaultsVisible(visible: boolean): void {
    this.faultLines.visible = visible;
    this.seedPoints.visible = visible;
  }

  dispose(): void {
    this.geometry.dispose();
    this.material.dispose();
    this.colourTexture.dispose();
    this.platesTexture.dispose();
    this.edgeGeometry.dispose();
    this.edgeMaterial.dispose();
    this.faultGeometry.dispose();
    this.faultMaterial.dispose();
    this.plateLineGeometry.dispose();
    this.plateLineMaterial.dispose();
    this.seedGeometry.dispose();
    this.seedMaterial.dispose();
    this.velocityGeometry.dispose();
    this.velocityMaterial.dispose();
    this.platesMaterial.dispose();
  }
}

/**
 * Append the triangles of one arrow — a shaft quad plus a head — laid out from
 * `origin` along `dir`, `size` world units long and `side` units wide, in the
 * proportions of `shape`.
 */
const emitArrow = (
  out: number[],
  origin: Vec3,
  dir: Vec3,
  side: Vec3,
  size: number,
  shape: ArrowShape,
): void => {
  /** A point `along` the arrow and `sideways` off it, as length fractions. */
  const at = (along: number, sideways: number): Vec3 => [
    origin[0] + (dir[0] * along + side[0] * sideways) * size,
    origin[1] + (dir[1] * along + side[1] * sideways) * size,
    origin[2] + (dir[2] * along + side[2] * sideways) * size,
  ];
  const backL = at(shape.back, shape.shaft);
  const backR = at(shape.back, -shape.shaft);
  const neckL = at(shape.neck, shape.shaft);
  const neckR = at(shape.neck, -shape.shaft);
  // Shaft, as two triangles, then the head from the neck out to the tip.
  out.push(...backL, ...backR, ...neckR);
  out.push(...backL, ...neckR, ...neckL);
  out.push(
    ...at(shape.tip, 0),
    ...at(shape.neck, shape.head),
    ...at(shape.neck, -shape.head),
  );
};

/**
 * Greedy spatial thinning of candidate points on the unit sphere: a point is
 * accepted only when no already-accepted point lies within `spacing` of it.
 *
 * Plate boundaries run over thousands of arcs, so comparing every candidate
 * against every survivor would be quadratic. Points are bucketed into a uniform
 * grid of cell size `spacing`, which puts every point that could be too close in
 * the 27 buckets around the candidate — near-linear in the number of candidates.
 */
class SphereThinner {
  private readonly buckets = new Map<number, number[]>();
  private readonly accepted: number[] = [];

  constructor(private readonly spacing: number) {}

  /** Record and accept `p` if it clears the spacing, else reject it. */
  accept(p: Vec3): boolean {
    const gx = Math.floor(p[0] / this.spacing);
    const gy = Math.floor(p[1] / this.spacing);
    const gz = Math.floor(p[2] / this.spacing);
    const limit = this.spacing * this.spacing;

    for (let dx = -1; dx <= 1; dx++) {
      for (let dy = -1; dy <= 1; dy++) {
        for (let dz = -1; dz <= 1; dz++) {
          const bucket = this.buckets.get(bucketKey(gx + dx, gy + dy, gz + dz));
          if (!bucket) continue;
          for (const i of bucket) {
            const ex = this.accepted[i * 3] - p[0];
            const ey = this.accepted[i * 3 + 1] - p[1];
            const ez = this.accepted[i * 3 + 2] - p[2];
            if (ex * ex + ey * ey + ez * ez < limit) return false;
          }
        }
      }
    }

    const index = this.accepted.length / 3;
    this.accepted.push(p[0], p[1], p[2]);
    const key = bucketKey(gx, gy, gz);
    const bucket = this.buckets.get(key);
    if (bucket) bucket.push(index);
    else this.buckets.set(key, [index]);
    return true;
  }
}

/**
 * Pack signed grid coordinates into one integer key. Points live on the unit
 * sphere and the smallest spacing in use keeps every index well inside +-512.
 */
const bucketKey = (x: number, y: number, z: number): number =>
  ((x + 512) * 1024 + (y + 512)) * 1024 + (z + 512);
