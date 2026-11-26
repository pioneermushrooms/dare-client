//! Getting around the map: which tiles a ship fits on, straight-line checks, and
//! A* paths between tiles.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use oss_sim::TileMap;

/// The map is this many tiles across and down.
pub const TILES: i32 = 1024;
const CELLS: usize = (TILES * TILES) as usize;
/// Straight and diagonal step costs.
const STEP: u32 = 10;
const DIAGONAL: u32 = 14;

/// Which tiles a ship of one size can sit on, and paths between them.
pub struct Nav {
    /// Tiles where a ship centred on the tile would touch a wall.
    blocked: Vec<bool>,
    /// Tiles a bullet can't pass (any solid tile).
    solid: Vec<bool>,
    // A* scratch space, reused between searches.
    cost: Vec<u32>,
    from: Vec<u32>,
    touched: Vec<u32>,
    /// Path searches run so far.
    pub searches: usize,
}

fn index(x: i32, y: i32) -> usize {
    (y * TILES + x) as usize
}

fn inside(x: i32, y: i32) -> bool {
    (0..TILES).contains(&x) && (0..TILES).contains(&y)
}

/// The tile under a position in pixels.
pub fn tile_of(px: (f32, f32)) -> (i32, i32) {
    (
        ((px.0 / 16.0).floor() as i32).clamp(0, TILES - 1),
        ((px.1 / 16.0).floor() as i32).clamp(0, TILES - 1),
    )
}

/// The centre of a tile in pixels.
pub fn centre(t: (i32, i32)) -> (f32, f32) {
    (t.0 as f32 * 16.0 + 8.0, t.1 as f32 * 16.0 + 8.0)
}

impl Nav {
    /// Clearance for a ship of `radius` pixels, with a few pixels to spare so a
    /// path doesn't scrape along walls.
    pub fn new(map: &TileMap, radius: i32) -> Self {
        let clearance = radius.max(1) + 3;
        let mut blocked = vec![false; CELLS];
        let mut solid = vec![false; CELLS];
        for y in 0..TILES {
            for x in 0..TILES {
                solid[index(x, y)] = map.is_solid(x, y);
                blocked[index(x, y)] = map.box_collides(x * 16 + 8, y * 16 + 8, clearance);
            }
        }
        Nav {
            blocked,
            solid,
            cost: vec![u32::MAX; CELLS],
            from: vec![u32::MAX; CELLS],
            touched: Vec::new(),
            searches: 0,
        }
    }

    /// Whether the ship fits on tile (x, y).
    pub fn open(&self, x: i32, y: i32) -> bool {
        inside(x, y) && !self.blocked[index(x, y)]
    }

    fn see_through(&self, x: i32, y: i32) -> bool {
        inside(x, y) && !self.solid[index(x, y)]
    }

    /// Whether the ship can fly in a straight line from `a` to `b` (pixels).
    pub fn clear_path(&self, a: (f32, f32), b: (f32, f32)) -> bool {
        self.walk(a, b, |x, y| self.open(x, y))
    }

    /// Whether a bullet could get from `a` to `b` (pixels) without hitting a wall.
    pub fn in_sight(&self, a: (f32, f32), b: (f32, f32)) -> bool {
        self.walk(a, b, |x, y| self.see_through(x, y))
    }

    fn walk(&self, a: (f32, f32), b: (f32, f32), ok: impl Fn(i32, i32) -> bool) -> bool {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let steps = ((dx.abs().max(dy.abs())) / 4.0).ceil().max(1.0) as i32;
        (0..=steps).all(|i| {
            let t = i as f32 / steps as f32;
            let (x, y) = tile_of((a.0 + dx * t, a.1 + dy * t));
            ok(x, y)
        })
    }

    /// The open tile nearest `t`, searching square rings up to `rings` tiles out.
    pub fn nearest_open(&self, t: (i32, i32), rings: i32) -> Option<(i32, i32)> {
        if self.open(t.0, t.1) {
            return Some(t);
        }
        for r in 1..=rings {
            for d in -r..=r {
                for c in [
                    (t.0 + d, t.1 - r),
                    (t.0 + d, t.1 + r),
                    (t.0 - r, t.1 + d),
                    (t.0 + r, t.1 + d),
                ] {
                    if self.open(c.0, c.1) {
                        return Some(c);
                    }
                }
            }
        }
        None
    }

    /// A path of open tiles from `start` to `goal` (both included), looking at no
    /// more than `budget` tiles. Blocked ends move to the nearest open tile.
    pub fn path(
        &mut self,
        start: (i32, i32),
        goal: (i32, i32),
        budget: usize,
    ) -> Option<Vec<(i32, i32)>> {
        self.searches += 1;
        let start = self.nearest_open(start, 4)?;
        let goal = self.nearest_open(goal, 8)?;
        for &i in &self.touched {
            self.cost[i as usize] = u32::MAX;
            self.from[i as usize] = u32::MAX;
        }
        self.touched.clear();

        let h = |x: i32, y: i32| {
            let (dx, dy) = ((x - goal.0).unsigned_abs(), (y - goal.1).unsigned_abs());
            STEP * dx.max(dy) + (DIAGONAL - STEP) * dx.min(dy)
        };
        let s = index(start.0, start.1);
        self.cost[s] = 0;
        self.touched.push(s as u32);
        let mut open = BinaryHeap::new();
        open.push(Reverse((h(start.0, start.1), 0u32, s as u32)));
        let mut looked = 0;
        while let Some(Reverse((_, g, i))) = open.pop() {
            let i = i as usize;
            if g > self.cost[i] {
                continue;
            }
            let (x, y) = ((i as i32) % TILES, (i as i32) / TILES);
            if (x, y) == goal {
                return Some(self.trace(i));
            }
            looked += 1;
            if looked > budget {
                return None;
            }
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if !self.open(nx, ny) {
                    continue;
                }
                // No cutting corners past a wall.
                let diagonal = dx != 0 && dy != 0;
                if diagonal && (!self.open(x + dx, y) || !self.open(x, y + dy)) {
                    continue;
                }
                let n = index(nx, ny);
                let ng = g + if diagonal { DIAGONAL } else { STEP };
                if ng < self.cost[n] {
                    if self.cost[n] == u32::MAX {
                        self.touched.push(n as u32);
                    }
                    self.cost[n] = ng;
                    self.from[n] = i as u32;
                    open.push(Reverse((ng + h(nx, ny), ng, n as u32)));
                }
            }
        }
        None
    }

    fn trace(&self, mut i: usize) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        loop {
            out.push(((i as i32) % TILES, (i as i32) / TILES));
            match self.from[i] {
                u32::MAX => break,
                p => i = p as usize,
            }
        }
        out.reverse();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty map with a wall across x = 500, open only at y 600..=605.
    fn walled() -> TileMap {
        let mut cells = vec![0u8; CELLS];
        for y in 0..TILES {
            if !(600..=605).contains(&y) {
                cells[index(500, y)] = 1;
            }
        }
        TileMap::from_cells(&cells)
    }

    #[test]
    fn paths_go_round_walls() {
        let mut nav = Nav::new(&walled(), 2);
        assert!(!nav.open(500, 100));
        assert!(nav.open(500, 602));
        let p = nav.path((490, 500), (510, 500), 200_000).unwrap();
        assert_eq!(p.first(), Some(&(490, 500)));
        assert_eq!(p.last(), Some(&(510, 500)));
        // Through the gap, and never on a blocked tile.
        assert!(p.iter().any(|&(x, y)| x == 500 && (600..=605).contains(&y)));
        assert!(p.iter().all(|&(x, y)| nav.open(x, y)));
        // A tiny budget gives up rather than searching the whole map.
        assert!(nav.path((490, 500), (510, 500), 10).is_none());
    }

    #[test]
    fn straight_lines_see_walls() {
        let nav = Nav::new(&walled(), 2);
        let (a, b) = (centre((490, 500)), centre((510, 500)));
        assert!(!nav.in_sight(a, b));
        assert!(!nav.clear_path(a, b));
        assert!(nav.in_sight(centre((490, 602)), centre((510, 602))));
        assert!(nav.clear_path(centre((480, 400)), centre((480, 700))));
        assert_eq!(nav.nearest_open((500, 100), 3).map(|t| t.0), Some(499));
        assert_eq!(tile_of((8000.0, 16.5)), (500, 1));
    }
}
