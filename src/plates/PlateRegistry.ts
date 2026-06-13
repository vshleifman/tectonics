/**
 * Persistent plate identity across topology changes.
 *
 * The flood fill in {@link Plates.assignPlates} renumbers connected components on
 * every run, so its "plate 3" this frame has no relationship to "plate 3" last
 * frame. The registry restores a stable identity by treating each reassignment
 * as connected-component tracking: it maps each raw flood-fill label onto a
 * stable id by maximum overlap with the previous assignment, allocating fresh
 * ids only where no predecessor is claimed.
 *
 * It is deliberately pure and free of any per-plate properties (motion, crust):
 * its sole job is the relabel, so it stays trivially unit-testable and the flood
 * fill stays untouched. Property reconciliation keys off the {@link RelabelReport}.
 */
export interface RelabelReport {
    /** Stable ids that survived the relabel (a raw label resolved to each). */
    keptIds: number[];
    /** Freshly allocated stable ids (raw labels with no winning predecessor). */
    newIds: number[];
    /** Previously-live stable ids no raw label claimed this frame. */
    retiredIds: number[];
    /**
     * Parent stable id for each new id: the predecessor it overlapped most but
     * lost (a split), or -1 when it has no predecessor (first run / genuine new).
     */
    parentOf: Map<number, number>;
}

export class PlateRegistry {
    /** Monotonic id source; never reused within a world so colours stay put. */
    private nextId = 0;

    /** False until the first relabel, so the first run treats prev as empty. */
    private hasPrev = false;

    /** Stable ids currently carrying cells, to compute retirements. */
    private liveIds = new Set<number>();

    /** Forget all identity (call when the world is rebuilt from scratch). */
    reset(): void {
        this.nextId = 0;
        this.hasPrev = false;
        this.liveIds.clear();
    }

    /**
     * Map raw flood-fill labels (`raw`, values in [0, rawCount)) onto stable ids,
     * writing the result back into `out` in place. `out` is expected to hold the
     * previous stable assignment on entry; all reads of it complete before any
     * write, so passing the same array for previous-in and stable-out is safe.
     *
     * Linear in cell count plus a small per-label tally, so it is cheap enough to
     * run on every simulation step later.
     */
    relabel(raw: Int32Array, rawCount: number, out: Int32Array): RelabelReport {
        const cellCount = raw.length;

        // 1. Tally previous-id overlap per raw label.
        const overlaps: Array<Map<number, number>> = new Array(rawCount);
        for (let r = 0; r < rawCount; r++) overlaps[r] = new Map();
        if (this.hasPrev) {
            for (let i = 0; i < cellCount; i++) {
                const r = raw[i];
                if (r < 0) continue;
                const prevId = out[i];
                if (prevId < 0) continue;
                const tally = overlaps[r];
                tally.set(prevId, (tally.get(prevId) ?? 0) + 1);
            }
        }

        // 2. Each raw label claims the previous id it overlaps most. Ties break
        //    to the smaller (older) id for long-term identity stability.
        const claimPrev = new Int32Array(rawCount).fill(-1);
        const claimCount = new Int32Array(rawCount);
        for (let r = 0; r < rawCount; r++) {
            let bestId = -1;
            let bestCount = 0;
            for (const [prevId, count] of overlaps[r]) {
                if (
                    count > bestCount ||
                    (count === bestCount && (bestId < 0 || prevId < bestId))
                ) {
                    bestCount = count;
                    bestId = prevId;
                }
            }
            claimPrev[r] = bestId;
            claimCount[r] = bestCount;
        }

        // 3. Resolve conflicts (handles splits): if several raw labels claim the
        //    same previous id, the one with the largest overlap keeps it.
        const winnerForPrev = new Map<number, number>();
        for (let r = 0; r < rawCount; r++) {
            const prevId = claimPrev[r];
            if (prevId < 0) continue;
            const cur = winnerForPrev.get(prevId);
            if (
                cur === undefined ||
                claimCount[r] > claimCount[cur] ||
                (claimCount[r] === claimCount[cur] && r < cur)
            ) {
                winnerForPrev.set(prevId, r);
            }
        }

        // 4. Allocate: winners inherit their id; everyone else gets a fresh id,
        //    tagged with the predecessor it broke off from (if any).
        const stableOf = new Int32Array(rawCount).fill(-1);
        const keptIds: number[] = [];
        const newIds: number[] = [];
        const parentOf = new Map<number, number>();
        for (const [prevId, r] of winnerForPrev) {
            stableOf[r] = prevId;
            keptIds.push(prevId);
        }
        for (let r = 0; r < rawCount; r++) {
            if (stableOf[r] >= 0) continue;
            const id = this.nextId++;
            stableOf[r] = id;
            newIds.push(id);
            parentOf.set(id, claimPrev[r]);
        }

        // 5. Write stable ids back over the previous assignment.
        for (let i = 0; i < cellCount; i++) {
            const r = raw[i];
            out[i] = r < 0 ? -1 : stableOf[r];
        }

        // 6. Anything live last frame but not kept has been merged away.
        const keptSet = new Set(keptIds);
        const retiredIds: number[] = [];
        for (const id of this.liveIds) {
            if (!keptSet.has(id)) retiredIds.push(id);
        }

        this.liveIds = new Set([...keptIds, ...newIds]);
        this.hasPrev = true;
        return { keptIds, newIds, retiredIds, parentOf };
    }
}
