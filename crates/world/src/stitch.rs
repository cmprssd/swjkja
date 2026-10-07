//! Closing cracks between neighbouring patches, as OpenJK's `rd-vanilla/tr_bsp.cpp`
//! does after loading (`R_StitchAllPatches`, `R_FixSharedVertexLodError`).
//!
//! Patches of one curved brush share a LOD origin and radius. Where one has a vertex
//! in the middle of another's edge segment, a matching row or column is inserted into
//! the other; afterwards, vertices shared along edges get the same LOD error so rows
//! drop out together at a distance.

use crate::patch::{Grid, MAX_GRID_SIZE, Vec3, insert_column, insert_row};

/// The original compares coordinates as `fabs(float - float) > .1` in double.
fn far(a: Vec3, b: Vec3, limit: f64) -> bool {
    (0..3).any(|i| f64::from(a[i] - b[i]).abs() > limit)
}

fn near_all(a: Vec3, b: Vec3, limit: f64) -> bool {
    (0..3).all(|i| f64::from(a[i] - b[i]).abs() < limit)
}

/// `R_MergedWidthPoints`: whether two inner vertices of the row starting at `offset`
/// coincide.
fn merged_width(g: &Grid, offset: usize) -> bool {
    (1..g.width.saturating_sub(1)).any(|i| {
        (i + 1..g.width - 1).any(|j| !far(g.verts[i + offset].xyz, g.verts[j + offset].xyz, 0.1))
    })
}

/// `R_MergedHeightPoints`: the same for the column at `offset`.
fn merged_height(g: &Grid, offset: usize) -> bool {
    (1..g.height.saturating_sub(1)).any(|i| {
        (i + 1..g.height - 1).any(|j| {
            !far(
                g.verts[g.width * i + offset].xyz,
                g.verts[g.width * j + offset].xyz,
                0.1,
            )
        })
    })
}

/// Patches whose LOD origin and radius are equal (as floats, like the original's
/// `!=` tests), so each patch is only compared with its own group. Members are in
/// surface order, which keeps the original's visiting order.
struct Groups {
    of: Vec<Option<usize>>,
    members: Vec<Vec<usize>>,
}

impl Groups {
    fn new(grids: &[Option<Grid>]) -> Self {
        let mut index = std::collections::HashMap::new();
        let mut of = vec![None; grids.len()];
        let mut members: Vec<Vec<usize>> = Vec::new();
        for (i, g) in grids.iter().enumerate() {
            let Some(g) = g else { continue };
            let values = [
                g.lod_radius,
                g.lod_origin[0],
                g.lod_origin[1],
                g.lod_origin[2],
            ];
            if values.iter().any(|v| v.is_nan()) {
                continue; // NaN equals nothing, itself included
            }
            // + 0.0 turns -0.0 into 0.0, which compare equal.
            let key = values.map(|v| (v + 0.0).to_bits());
            let group = *index.entry(key).or_insert_with(|| {
                members.push(Vec::new());
                members.len() - 1
            });
            members[group].push(i);
            of[i] = Some(group);
        }
        Self { of, members }
    }

    fn members(&self, i: usize) -> &[usize] {
        self.of[i].map_or(&[], |g| &self.members[g])
    }
}

/// One edge of a grid: its vertex positions in order, and its LOD errors.
struct Edge<'a> {
    grid: &'a Grid,
    /// True for the first or last row (vertices along the width).
    along_width: bool,
    offset: usize,
}

impl Edge<'_> {
    fn len(&self) -> usize {
        if self.along_width {
            self.grid.width
        } else {
            self.grid.height
        }
    }
    fn xyz(&self, k: usize) -> Vec3 {
        let g = self.grid;
        if self.along_width {
            g.verts[k + self.offset].xyz
        } else {
            g.verts[g.width * k + self.offset].xyz
        }
    }
    fn errors(&self) -> &[f32] {
        if self.along_width {
            &self.grid.width_lod_error
        } else {
            &self.grid.height_lod_error
        }
    }
    fn merged(&self) -> bool {
        if self.along_width {
            merged_width(self.grid, self.offset)
        } else {
            merged_height(self.grid, self.offset)
        }
    }
}

/// The four edges of a grid in the original's order for one direction: the first and
/// last row (`along_width`), or the first and last column.
fn edges(g: &Grid, along_width: bool) -> [Edge<'_>; 2] {
    [0, 1].map(|n| Edge {
        grid: g,
        along_width,
        offset: match (along_width, n) {
            (true, 0) | (false, 0) => 0,
            (true, _) => (g.height - 1) * g.width,
            (false, _) => g.width - 1,
        },
    })
}

/// Tries to insert into `g2` the vertex between `e1`'s vertices `a` and `b` (which are
/// two apart), where they match two neighbouring vertices of one of `g2`'s edges.
fn try_insert(g2: &Grid, a: Vec3, b: Vec3, point: Vec3, lod_error: f32) -> Option<Grid> {
    // Columns first: g2's first and last row.
    if g2.width < MAX_GRID_SIZE {
        for (m, e2) in edges(g2, true).iter().enumerate() {
            for l in 0..g2.width - 1 {
                if far(a, e2.xyz(l), 0.1) || far(b, e2.xyz(l + 1), 0.1) {
                    continue;
                }
                if near_all(e2.xyz(l), e2.xyz(l + 1), 0.01) {
                    continue;
                }
                let row = if m == 1 { g2.height - 1 } else { 0 };
                return insert_column(g2, l + 1, row, point, lod_error);
            }
        }
    }
    // Then rows: g2's first and last column.
    if g2.height < MAX_GRID_SIZE {
        for (m, e2) in edges(g2, false).iter().enumerate() {
            for l in 0..g2.height - 1 {
                if far(a, e2.xyz(l), 0.1) || far(b, e2.xyz(l + 1), 0.1) {
                    continue;
                }
                if near_all(e2.xyz(l), e2.xyz(l + 1), 0.01) {
                    continue;
                }
                let column = if m == 1 { g2.width - 1 } else { 0 };
                return insert_row(g2, l + 1, column, point, lod_error);
            }
        }
    }
    None
}

/// `R_StitchPatches`: one insertion into `g2` where an edge of `g1` has a vertex `g2`
/// lacks, or `None`.
fn stitch(g1: &Grid, g2: &Grid) -> Option<Grid> {
    // Forward along each edge of g1: rows, then columns.
    for along_width in [true, false] {
        for e1 in edges(g1, along_width) {
            if e1.merged() {
                continue;
            }
            let mut k = 0;
            while k + 2 < e1.len() {
                let point = e1.xyz(k + 1);
                if let Some(g) = try_insert(g2, e1.xyz(k), e1.xyz(k + 2), point, e1.errors()[k + 1])
                {
                    return Some(g);
                }
                k += 2;
            }
        }
    }
    // Backward along each edge of g1 (catches edges that run the other way).
    for along_width in [true, false] {
        for e1 in edges(g1, along_width) {
            if e1.merged() {
                continue;
            }
            let mut k = e1.len() - 1;
            while k > 1 {
                // The original reads the error at k + 1 here (one past the end on the
                // first step, which is unreadable garbage); k - 1 stands in for that case.
                let err = e1
                    .errors()
                    .get(k + 1)
                    .copied()
                    .unwrap_or(e1.errors()[k - 1]);
                if let Some(g) = try_insert(g2, e1.xyz(k), e1.xyz(k - 2), e1.xyz(k - 1), err) {
                    return Some(g);
                }
                k -= 2;
            }
        }
    }
    None
}

/// `R_StitchAllPatches`. `grids[i]` is surface i's grid, if it is a patch. Returns the
/// number of insertions made.
pub fn stitch_all(grids: &mut [Option<Grid>]) -> usize {
    let groups = Groups::new(grids);
    let mut stitches = 0;
    loop {
        let mut stitched = false;
        for i in 0..grids.len() {
            match &mut grids[i] {
                Some(g) if !g.lod_stitched => g.lod_stitched = true,
                _ => continue,
            }
            stitched = true;
            // R_TryStitchingPatch: against every patch of the same group, itself
            // included, in surface order.
            for &j in groups.members(i) {
                let next = |grids: &[Option<Grid>]| match (&grids[i], &grids[j]) {
                    (Some(g1), Some(g2)) => stitch(g1, g2),
                    _ => None,
                };
                while let Some(mut new) = next(grids) {
                    new.lod_stitched = false;
                    grids[j] = Some(new);
                    stitches += 1;
                }
            }
        }
        if !stitched {
            return stitches;
        }
    }
}

/// `R_FixSharedVertexLodError`: patches of one group that share inner edge vertices
/// get the same LOD errors for them.
pub fn fix_shared_vertex_lod_error(grids: &mut [Option<Grid>]) {
    let groups = Groups::new(grids);
    for i in 0..grids.len() {
        match &mut grids[i] {
            Some(g) if g.lod_fixed == 0 => g.lod_fixed = 2,
            _ => continue,
        }
        fix_r(grids, &groups, i + 1, i);
    }
}

fn fix_r(grids: &mut [Option<Grid>], groups: &Groups, start: usize, g1_index: usize) {
    let g1 = grids[g1_index].clone().expect("a grid");
    for &j in groups.members(g1_index).iter().filter(|&&j| j >= start) {
        let Some(g2) = &mut grids[j] else { continue };
        if g2.lod_fixed == 2 {
            continue;
        }
        let mut touch = false;
        for along_width in [true, false] {
            for e1 in edges(&g1, along_width) {
                if e1.merged() {
                    continue;
                }
                for k in 1..e1.len().saturating_sub(1) {
                    let p = e1.xyz(k);
                    let err = e1.errors()[k];
                    // Matching vertices on g2's rows set its column errors, and on its
                    // columns its row errors.
                    for g2_along_width in [true, false] {
                        for n2 in 0..2 {
                            let e2 = &edges(g2, g2_along_width)[n2];
                            if e2.merged() {
                                continue;
                            }
                            let matches: Vec<usize> = (1..e2.len().saturating_sub(1))
                                .filter(|&l| !far(p, e2.xyz(l), 0.1))
                                .collect();
                            for l in matches {
                                if g2_along_width {
                                    g2.width_lod_error[l] = err;
                                } else {
                                    g2.height_lod_error[l] = err;
                                }
                                touch = true;
                            }
                        }
                    }
                }
            }
        }
        if touch {
            g2.lod_fixed = 2;
            fix_r(grids, groups, start, j);
        }
    }
}
