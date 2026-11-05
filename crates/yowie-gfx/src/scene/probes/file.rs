//! Saving and loading baked light probe grids, so games bake once (in a
//! tool or on first run) and load at start-up.
//!
//! The format is little-endian: the magic `YLPG`, a version (1), the
//! origin and spacing (three `f32` each), the counts (three `u32`), the
//! intensity (`f32`), the capture resolution (`u32`), then 27 `f32` per
//! cell (nine coefficients of red, green and blue), cells x fastest, then
//! y, then z.

use yowie_core::{UVec3, Vec3};

use super::grid::LightProbeGrid;
use super::sh::CELL_FLOATS;
use crate::error::{GfxError, Result};

const MAGIC: &[u8; 4] = b"YLPG";
const VERSION: u32 = 1;
/// Bytes before the cells.
const HEADER: usize = 4 + 4 + 12 + 12 + 12 + 4 + 4;

impl LightProbeGrid {
    /// The grid's placement and baked coefficients as bytes, for a file
    /// (through `yowie_asset` or `std::fs`) or a save. Fails when the grid
    /// is not baked.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let d = self
            .data()
            .ok_or_else(|| GfxError::Probes("the grid is not baked".into()))?;
        let mut out = Vec::with_capacity(HEADER + d.cells.len() * CELL_FLOATS * 4);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        for v in self
            .origin
            .to_array()
            .into_iter()
            .chain(self.spacing.to_array())
        {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for c in self.counts.to_array() {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&self.intensity.to_le_bytes());
        out.extend_from_slice(&self.resolution.to_le_bytes());
        for cell in &d.cells {
            for v in cell {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        Ok(out)
    }

    /// A baked grid read back from [`to_bytes`](Self::to_bytes)' output.
    /// Fails on another format or version, or a length that does not match
    /// the counts.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bad = |why: &str| GfxError::Probes(format!("cannot read probe data: {why}"));
        if bytes.len() < HEADER || &bytes[..4] != MAGIC {
            return Err(bad("not a Yowie light probe file"));
        }
        let word = |i: usize| -> [u8; 4] {
            let at = 4 + i * 4;
            [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
        };
        let f = |i: usize| f32::from_le_bytes(word(i));
        let u = |i: usize| u32::from_le_bytes(word(i));
        if u(0) != VERSION {
            return Err(bad(&format!("version {} is not {VERSION}", u(0))));
        }
        let mut grid = LightProbeGrid::new(
            Vec3::new(f(1), f(2), f(3)),
            Vec3::new(f(4), f(5), f(6)),
            UVec3::new(u(7), u(8), u(9)),
        );
        grid.intensity = f(10);
        grid.resolution = u(11);
        let cells = grid.cell_count();
        if grid.counts.min_element() == 0 || bytes.len() != HEADER + cells * CELL_FLOATS * 4 {
            return Err(bad(&format!(
                "{} bytes do not hold {} cells",
                bytes.len(),
                cells
            )));
        }
        let data = bytes[HEADER..]
            .as_chunks::<4>()
            .0
            .as_chunks::<CELL_FLOATS>()
            .0
            .iter()
            .map(|cell| std::array::from_fn(|i| f32::from_le_bytes(cell[i])))
            .collect();
        grid.set_cells(data);
        Ok(grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip_and_bad_input_fails() {
        let mut g = LightProbeGrid::new(
            Vec3::new(-6.0, 0.9, -6.0),
            Vec3::new(6.0, 2.0, 6.0),
            UVec3::new(3, 2, 1),
        );
        assert!(g.to_bytes().is_err(), "unbaked grids have nothing to save");
        g.intensity = 1.5;
        let cells: Vec<[f32; CELL_FLOATS]> = (0..6)
            .map(|i| std::array::from_fn(|k| (i * 100 + k) as f32 * 0.01))
            .collect();
        g.set_cells(cells.clone());
        let bytes = g.to_bytes().expect("baked");
        let back = LightProbeGrid::from_bytes(&bytes).expect("read");
        assert_eq!(back.origin, g.origin);
        assert_eq!(back.spacing, g.spacing);
        assert_eq!(back.counts, g.counts);
        assert_eq!(back.intensity, 1.5);
        assert_eq!(back.data().map(|d| d.cells.clone()), Some(cells));
        let cases: [&[u8]; 3] = [&bytes[..bytes.len() - 4], b"PNG\0rest", &[]];
        for c in cases {
            assert!(LightProbeGrid::from_bytes(c).is_err());
        }
        let mut wrong = bytes.clone();
        wrong[4] = 9;
        assert!(LightProbeGrid::from_bytes(&wrong).is_err(), "version");
    }
}
