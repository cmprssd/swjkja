//! Curved surfaces: a patch's control points subdivided into a grid of vertices, as
//! OpenJK's `rd-vanilla/tr_curve.cpp` does it, operation for operation, so the
//! geometry (and where rows can later be dropped for distance) matches.

use swjkja_bsp::DrawVert;

/// Largest grid, in vertices per side (`MAX_GRID_SIZE`).
pub const MAX_GRID_SIZE: usize = 65;
/// The value an error-table entry gets for a row or column that is flat enough to
/// drop entirely.
const FLAT: f32 = 999.0;

pub type Vec3 = [f32; 3];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}
/// `VectorNormalize2`: the unit vector and the original length; zero stays zero.
fn normalize(a: Vec3) -> (Vec3, f32) {
    let len = length(a);
    if len == 0.0 {
        return ([0.0; 3], 0.0);
    }
    let inv = 1.0 / len;
    ([a[0] * inv, a[1] * inv, a[2] * inv], len)
}

/// A patch after subdivision: `width * height` vertices, row-major, plus the error
/// for each column (`width_lod_error`) and row (`height_lod_error`) that decides when
/// it can be skipped at a distance.
#[derive(Debug, Clone)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub verts: Vec<DrawVert>,
    pub width_lod_error: Vec<f32>,
    pub height_lod_error: Vec<f32>,
    pub mesh_bounds: [Vec3; 2],
    pub local_origin: Vec3,
    pub mesh_radius: f32,
    /// Patches with the same LOD origin and radius belong together (one curved
    /// brush) and are stitched and LOD-synced as a group.
    pub lod_origin: Vec3,
    pub lod_radius: f32,
    pub(crate) lod_fixed: u8,
    pub(crate) lod_stitched: bool,
}

impl Grid {
    pub fn vert(&self, column: usize, row: usize) -> &DrawVert {
        &self.verts[row * self.width + column]
    }

    /// What drawing it at a distance needs: its size, errors and LOD volume.
    pub fn lod(&self) -> GridLod {
        GridLod {
            width: self.width,
            height: self.height,
            width_lod_error: self.width_lod_error.clone(),
            height_lod_error: self.height_lod_error.clone(),
            lod_origin: self.lod_origin,
            lod_radius: self.lod_radius,
        }
    }
}

/// `r_lodCurveError`'s default, the same in both games.
pub const LOD_CURVE_ERROR: f32 = 250.0;

/// A grid as `RB_SurfaceGrid` draws it: the columns and rows whose error is within
/// what the view's distance allows, the rest skipped.
#[derive(Debug, Clone)]
pub struct GridLod {
    pub width: usize,
    pub height: usize,
    pub width_lod_error: Vec<f32>,
    pub height_lod_error: Vec<f32>,
    pub lod_origin: Vec3,
    pub lod_radius: f32,
}

impl GridLod {
    /// `LodErrorForVolume`: the error allowed with `lod_origin` at `origin` (the
    /// grid's entity placed) and the eye at `view_origin` looking along `forward`;
    /// the distance is along `forward`, less the LOD radius, at least 1.
    pub fn allowed_error(&self, origin: Vec3, view_origin: Vec3, forward: Vec3) -> f32 {
        let d = dot(sub(origin, view_origin), forward).abs() - self.lod_radius;
        LOD_CURVE_ERROR / d.max(1.0)
    }

    /// `RB_SurfaceGrid`'s triangles at `allowed_error` (see [`Self::allowed_error`]),
    /// over the full grid's vertices numbered from `base`. The edges always stay.
    pub fn indexes(&self, allowed_error: f32, base: u32, out: &mut Vec<u32>) {
        let kept = |errors: &[f32]| -> Vec<u32> {
            let n = errors.len();
            (0..n)
                .filter(|&i| i == 0 || i == n - 1 || errors[i] <= allowed_error)
                .map(|i| i as u32)
                .collect()
        };
        let columns = kept(&self.width_lod_error);
        let w = self.width as u32;
        let mut above: Option<u32> = None;
        for row in kept(&self.height_lod_error) {
            let row = base + row * w;
            if let Some(prev) = above {
                for pair in columns.windows(2) {
                    let (v2, v1) = (prev + pair[0], prev + pair[1]);
                    let (v3, v4) = (row + pair[0], row + pair[1]);
                    out.extend([v2, v3, v1, v1, v3, v4]);
                }
            }
            above = Some(row);
        }
    }
}

/// The working control-point array, `ctrl[row][column]`, always 65 x 65 like the
/// original's stack array (transposes move data within it).
struct Ctrl(Vec<DrawVert>);

impl Ctrl {
    fn new() -> Self {
        Self(vec![zero_vert(); MAX_GRID_SIZE * MAX_GRID_SIZE])
    }
    fn at(&self, row: usize, column: usize) -> &DrawVert {
        &self.0[row * MAX_GRID_SIZE + column]
    }
    fn at_mut(&mut self, row: usize, column: usize) -> &mut DrawVert {
        &mut self.0[row * MAX_GRID_SIZE + column]
    }
    fn swap(&mut self, a: (usize, usize), b: (usize, usize)) {
        self.0
            .swap(a.0 * MAX_GRID_SIZE + a.1, b.0 * MAX_GRID_SIZE + b.1);
    }
}

fn zero_vert() -> DrawVert {
    DrawVert {
        xyz: [0.0; 3],
        st: [0.0; 2],
        lightmap: [[0.0; 2]; 4],
        normal: [0.0; 3],
        color: [[0; 4]; 4],
    }
}

/// `LerpDrawVert`: the midpoint of two vertices (colours rounded down).
pub fn lerp(a: &DrawVert, b: &DrawVert) -> DrawVert {
    let mid = |x: f32, y: f32| 0.5 * (x + y);
    let mut out = zero_vert();
    for i in 0..3 {
        out.xyz[i] = mid(a.xyz[i], b.xyz[i]);
        out.normal[i] = mid(a.normal[i], b.normal[i]);
    }
    for i in 0..2 {
        out.st[i] = mid(a.st[i], b.st[i]);
    }
    for k in 0..4 {
        for i in 0..2 {
            out.lightmap[k][i] = mid(a.lightmap[k][i], b.lightmap[k][i]);
        }
        for i in 0..4 {
            out.color[k][i] = ((u16::from(a.color[k][i]) + u16::from(b.color[k][i])) >> 1) as u8;
        }
    }
    out
}

/// `Transpose`: swaps rows and columns in place within the 65 x 65 array.
fn transpose(width: usize, height: usize, ctrl: &mut Ctrl) {
    if width > height {
        for i in 0..height {
            for j in i + 1..width {
                if j < height {
                    ctrl.swap((j, i), (i, j));
                } else {
                    *ctrl.at_mut(j, i) = *ctrl.at(i, j);
                }
            }
        }
    } else {
        for i in 0..width {
            for j in i + 1..height {
                if j < width {
                    ctrl.swap((i, j), (j, i));
                } else {
                    *ctrl.at_mut(i, j) = *ctrl.at(j, i);
                }
            }
        }
    }
}

/// `MakeMeshNormals`: each vertex's normal from the cross products of its eight
/// neighbour directions, wrapping around closed patches and skipping degenerate edges.
fn make_mesh_normals(width: usize, height: usize, ctrl: &mut Ctrl) {
    const NEIGHBORS: [(isize, isize); 8] = [
        (0, 1),
        (1, 1),
        (1, 0),
        (1, -1),
        (0, -1),
        (-1, -1),
        (-1, 0),
        (-1, 1),
    ];
    let (w, h) = (width as isize, height as isize);
    let len_sq = |v: Vec3| dot(v, v);
    let wrap_width =
        (0..height).all(|i| len_sq(sub(ctrl.at(i, 0).xyz, ctrl.at(i, width - 1).xyz)) <= 1.0);
    let wrap_height =
        (0..width).all(|i| len_sq(sub(ctrl.at(0, i).xyz, ctrl.at(height - 1, i).xyz)) <= 1.0);

    for i in 0..width {
        for j in 0..height {
            let base = ctrl.at(j, i).xyz;
            let mut around = [[0.0f32; 3]; 8];
            let mut good = [false; 8];
            for (k, &(dx, dy)) in NEIGHBORS.iter().enumerate() {
                for dist in 1..=3 {
                    let mut x = i as isize + dx * dist;
                    let mut y = j as isize + dy * dist;
                    if wrap_width {
                        if x < 0 {
                            x += w - 1;
                        } else if x >= w {
                            x = 1 + x - w;
                        }
                    }
                    if wrap_height {
                        if y < 0 {
                            y += h - 1;
                        } else if y >= h {
                            y = 1 + y - h;
                        }
                    }
                    if x < 0 || x >= w || y < 0 || y >= h {
                        break; // edge of the patch
                    }
                    let (dir, len) = normalize(sub(ctrl.at(y as usize, x as usize).xyz, base));
                    if len == 0.0 {
                        continue; // degenerate edge: look further out
                    }
                    good[k] = true;
                    around[k] = dir;
                    break;
                }
            }
            let mut sum = [0.0f32; 3];
            for k in 0..8 {
                if !good[k] || !good[(k + 1) & 7] {
                    continue;
                }
                let (n, len) = normalize(cross(around[(k + 1) & 7], around[k]));
                if len == 0.0 {
                    continue;
                }
                sum = [sum[0] + n[0], sum[1] + n[1], sum[2] + n[2]];
            }
            ctrl.at_mut(j, i).normal = normalize(sum).0;
        }
    }
}

/// `InvertCtrl`: mirrors every row.
fn invert_ctrl(width: usize, height: usize, ctrl: &mut Ctrl) {
    for i in 0..height {
        for j in 0..width / 2 {
            ctrl.swap((i, j), (i, width - 1 - j));
        }
    }
}

/// `InvertErrorTable`, after a transpose and mirror.
fn invert_error_table(table: &mut [[f32; MAX_GRID_SIZE]; 2], width: usize, height: usize) {
    let copy = *table;
    table[1][..width].copy_from_slice(&copy[0][..width]);
    for i in 0..height {
        table[0][i] = copy[1][height - 1 - i];
    }
}

/// `PutPointsOnCurve`: moves the odd rows and columns from control points onto the
/// curve.
fn put_points_on_curve(width: usize, height: usize, ctrl: &mut Ctrl) {
    for i in 0..width {
        for j in (1..height).step_by(2) {
            let prev = lerp(ctrl.at(j, i), ctrl.at(j + 1, i));
            let next = lerp(ctrl.at(j, i), ctrl.at(j - 1, i));
            *ctrl.at_mut(j, i) = lerp(&prev, &next);
        }
    }
    for j in 0..height {
        for i in (1..width).step_by(2) {
            let prev = lerp(ctrl.at(j, i), ctrl.at(j, i + 1));
            let next = lerp(ctrl.at(j, i), ctrl.at(j, i - 1));
            *ctrl.at_mut(j, i) = lerp(&prev, &next);
        }
    }
}

/// `R_CreateSurfaceGridMesh`.
fn create_grid(
    width: usize,
    height: usize,
    ctrl: &Ctrl,
    table: &[[f32; MAX_GRID_SIZE]; 2],
) -> Grid {
    let mut verts = Vec::with_capacity(width * height);
    let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
    for j in 0..height {
        for i in 0..width {
            let v = *ctrl.at(j, i);
            for k in 0..3 {
                bounds[0][k] = bounds[0][k].min(v.xyz[k]);
                bounds[1][k] = bounds[1][k].max(v.xyz[k]);
            }
            verts.push(v);
        }
    }
    let origin = [0, 1, 2].map(|k| (bounds[0][k] + bounds[1][k]) * 0.5);
    let radius = length(sub(bounds[0], origin));
    Grid {
        width,
        height,
        verts,
        width_lod_error: table[0][..width].to_vec(),
        height_lod_error: table[1][..height].to_vec(),
        mesh_bounds: bounds,
        local_origin: origin,
        mesh_radius: radius,
        lod_origin: origin,
        lod_radius: radius,
        lod_fixed: 0,
        lod_stitched: false,
    }
}

/// `R_SubdividePatchToGrid`: `points` are `width * height` control points, row-major;
/// `subdivisions` is `r_subdivisions` (4 by default: the largest allowed distance from
/// a straight segment to the curve, in map units).
pub fn subdivide(
    mut width: usize,
    mut height: usize,
    points: &[DrawVert],
    subdivisions: f32,
) -> Grid {
    let mut ctrl = Ctrl::new();
    for j in 0..height {
        for i in 0..width {
            *ctrl.at_mut(j, i) = points[j * width + i];
        }
    }
    let mut table = [[0.0f32; MAX_GRID_SIZE]; 2];
    for dir in 0..2 {
        let mut j = 0;
        while j + 2 < width {
            // The largest distance from a control row to the straight line between its
            // ends, measured at the curve's midpoint.
            let mut max_len = 0.0f32;
            for i in 0..height {
                let (a, b, c) = (
                    ctrl.at(i, j).xyz,
                    ctrl.at(i, j + 1).xyz,
                    ctrl.at(i, j + 2).xyz,
                );
                let mid = [0, 1, 2].map(|l| (a[l] + b[l] * 2.0 + c[l]) * 0.25);
                let mid = sub(mid, a);
                let (d, _) = normalize(sub(c, a));
                let along = dot(mid, d);
                let off = sub(mid, [d[0] * along, d[1] * along, d[2] * along]);
                max_len = max_len.max(dot(off, off));
            }
            let max_len = max_len.sqrt();
            if max_len < 0.1 {
                table[dir][j + 1] = FLAT;
                j += 2;
                continue;
            }
            if width + 2 > MAX_GRID_SIZE || max_len <= subdivisions {
                table[dir][j + 1] = 1.0 / max_len;
                j += 2;
                continue;
            }
            table[dir][j + 2] = 1.0 / max_len;
            width += 2;
            for i in 0..height {
                let prev = lerp(ctrl.at(i, j), ctrl.at(i, j + 1));
                let next = lerp(ctrl.at(i, j + 1), ctrl.at(i, j + 2));
                let mid = lerp(&prev, &next);
                for k in (j + 4..width).rev() {
                    *ctrl.at_mut(i, k) = *ctrl.at(i, k - 2);
                }
                *ctrl.at_mut(i, j + 1) = prev;
                *ctrl.at_mut(i, j + 2) = mid;
                *ctrl.at_mut(i, j + 3) = next;
            }
            // Check the new halves again (j stays: the original's `j -= 2` then `j += 2`).
        }
        transpose(width, height, &mut ctrl);
        std::mem::swap(&mut width, &mut height);
    }

    put_points_on_curve(width, height, &mut ctrl);

    // Drop the rows and columns that were flat to begin with. Like the original, the
    // index moves on after a removal, so of two adjacent flat ones only the first goes.
    let mut i = 1;
    while i + 1 < width {
        if table[0][i] == FLAT {
            for j in i + 1..width {
                for k in 0..height {
                    *ctrl.at_mut(k, j - 1) = *ctrl.at(k, j);
                }
                table[0][j - 1] = table[0][j];
            }
            width -= 1;
        }
        i += 1;
    }
    let mut i = 1;
    while i + 1 < height {
        if table[1][i] == FLAT {
            for j in i + 1..height {
                for k in 0..width {
                    *ctrl.at_mut(j - 1, k) = *ctrl.at(j, k);
                }
                table[1][j - 1] = table[1][j];
            }
            height -= 1;
        }
        i += 1;
    }

    // Keep the long side as the width (rotating the grid if needed).
    if height > width {
        transpose(width, height, &mut ctrl);
        invert_error_table(&mut table, width, height);
        std::mem::swap(&mut width, &mut height);
        invert_ctrl(width, height, &mut ctrl);
    }
    make_mesh_normals(width, height, &mut ctrl);
    create_grid(width, height, &ctrl, &table)
}

fn rebuild(
    grid: &Grid,
    width: usize,
    height: usize,
    ctrl: &mut Ctrl,
    table: &[[f32; MAX_GRID_SIZE]; 2],
) -> Grid {
    make_mesh_normals(width, height, ctrl);
    let mut g = create_grid(width, height, ctrl, table);
    g.lod_radius = grid.lod_radius;
    g.lod_origin = grid.lod_origin;
    g
}

/// `R_GridInsertColumn`: a new column at `column`, halfway between its neighbours,
/// with the vertex at `row` moved to `point`. `None` if the grid is already full.
pub fn insert_column(
    grid: &Grid,
    column: usize,
    row: usize,
    point: Vec3,
    lod_error: f32,
) -> Option<Grid> {
    let width = grid.width + 1;
    if width > MAX_GRID_SIZE {
        return None;
    }
    let mut ctrl = Ctrl::new();
    let mut table = [[0.0f32; MAX_GRID_SIZE]; 2];
    let mut old = 0;
    for i in 0..width {
        if i == column {
            for j in 0..grid.height {
                let mut v = lerp(grid.vert(i - 1, j), grid.vert(i, j));
                if j == row {
                    v.xyz = point;
                }
                *ctrl.at_mut(j, i) = v;
            }
            table[0][i] = lod_error;
            continue;
        }
        table[0][i] = grid.width_lod_error[old];
        for j in 0..grid.height {
            *ctrl.at_mut(j, i) = *grid.vert(old, j);
        }
        old += 1;
    }
    table[1][..grid.height].copy_from_slice(&grid.height_lod_error);
    Some(rebuild(grid, width, grid.height, &mut ctrl, &table))
}

/// `R_GridInsertRow`: like [`insert_column`], for a row.
pub fn insert_row(
    grid: &Grid,
    row: usize,
    column: usize,
    point: Vec3,
    lod_error: f32,
) -> Option<Grid> {
    let height = grid.height + 1;
    if height > MAX_GRID_SIZE {
        return None;
    }
    let mut ctrl = Ctrl::new();
    let mut table = [[0.0f32; MAX_GRID_SIZE]; 2];
    let mut old = 0;
    for i in 0..height {
        if i == row {
            for j in 0..grid.width {
                let mut v = lerp(grid.vert(j, i - 1), grid.vert(j, i));
                if j == column {
                    v.xyz = point;
                }
                *ctrl.at_mut(i, j) = v;
            }
            table[1][i] = lod_error;
            continue;
        }
        table[1][i] = grid.height_lod_error[old];
        for j in 0..grid.width {
            *ctrl.at_mut(i, j) = *grid.vert(j, old);
        }
        old += 1;
    }
    table[0][..grid.width].copy_from_slice(&grid.width_lod_error);
    Some(rebuild(grid, grid.width, height, &mut ctrl, &table))
}
