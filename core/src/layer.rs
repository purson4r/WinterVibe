use crate::geometry::V2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnowDriftMode {
    Small,
    Large,
    #[default]
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnowDriftSettings {
    pub mode: SnowDriftMode,
    pub height: u32,
    pub size: u32,
    pub unevenness: u32,
    pub accumulation: u32,
    pub max_height: u32,
}

impl Default for SnowDriftSettings {
    fn default() -> SnowDriftSettings {
        SnowDriftSettings { mode: SnowDriftMode::Mixed, height: 100, size: 100, unevenness: 50, accumulation: 100, max_height: 80 }
    }
}

impl SnowDriftSettings {
    pub fn validated(mut self) -> SnowDriftSettings {
        self.height = self.height.clamp(50, 150);
        self.size = self.size.clamp(50, 200);
        self.unevenness = self.unevenness.min(100);
        self.accumulation = self.accumulation.clamp(50, 150);
        self.max_height = self.max_height.clamp(10, 80);
        self
    }
}

/// A height field of snow columns lying on a bed: the screen bottom, or the top edge of a window
/// whose rounded corners curve the bed down at both ends.
pub struct SnowLayer {
    pub column_width: f32,
    pub opens_at_edges: bool,
    pub origin: V2,
    pub heights: Vec<f32>,
    /// How much of each column's snow a wall has packed down already, so it isn't packed again.
    pub packed: Vec<f32>,
    /// The bed under each column relative to `origin.y`; zero along a flat edge, negative where a corner rounds off.
    pub bed: Vec<f32>,
    pub visible: Vec<bool>,
    pub top: f32,
    pub bed_depth: f32,
    pub occluders: Vec<[f32; 4]>,
    pub moved_at: f64,
    /// When the snow came back onto its window from elsewhere, to grow back into shape rather than appear at once.
    pub returned_at: f64,
    pub spill_left: f32,
    pub spill_right: f32,
    /// How high the snow lying just past each end, left then right, comes up that end: it holds up the snow on the
    /// ledge, which spills off only above it.
    pub outside: [f32; 2],
    /// Smoothed path of the supporting edge; the snow reacts to its accelerations, not to raw jittery samples.
    pub track: V2,
    pub track_velocity: V2,
    /// The snow body's displacement from its support: x slides along it, y lifts off it.
    pub offset: V2,
    pub offset_velocity: V2,
    pub previous_base_y: f32,
    pub agitation: f32,
    /// How far the snow has moved along the window since it came, as the window's left edge was dragged in or out
    /// under it: what is drawn in it moves along too.
    pub drift: f32,
    /// The window's right edge, where the snow ends short of a whole column: which edge moved tells a resize from a
    /// move.
    pub right: f32,
    /// How many leaves lie on this heap, as of the last step.
    pub lying_leaves: f32,
    /// How fast the heap was going as it came off its window at this step, for the leaves lying on it to go along.
    pub thrown: Option<V2>,
    pub swing_direction: V2,
    pub swing_peak: f32,
    pub previous_swing_peak: f32,

    capacity: Vec<f32>,
    depth: f32,
    phases: (f32, f32),
    corner: (f32, f32),
    drift_settings: SnowDriftSettings,
    maximum: f32,
    drift_enabled: bool,
    supported: Vec<bool>,
    supported_range: Option<(usize, usize)>,
    parity: bool,
}

impl SnowLayer {
    pub fn new(origin: V2, columns: usize, column_width: f32, depth: f32, opens_at_edges: bool, phases: (f32, f32)) -> SnowLayer {
        let mut layer = SnowLayer {
            column_width,
            opens_at_edges,
            origin,
            heights: vec![0.0; columns],
            packed: vec![0.0; columns],
            bed: vec![0.0; columns],
            visible: vec![true; columns],
            top: 0.0,
            bed_depth: 0.0,
            occluders: Vec::new(),
            moved_at: -1.0,
            returned_at: -10.0,
            spill_left: 0.0,
            spill_right: 0.0,
            outside: [0.0; 2],
            track: origin,
            track_velocity: V2::ZERO,
            offset: V2::ZERO,
            offset_velocity: V2::ZERO,
            previous_base_y: origin.y,
            agitation: 0.0,
            drift: 0.0,
            right: 0.0,
            lying_leaves: 0.0,
            thrown: None,
            swing_direction: V2::ZERO,
            swing_peak: 0.0,
            previous_swing_peak: 0.0,
            capacity: Vec::new(),
            depth,
            drift_settings: SnowDriftSettings::default(),
            maximum: f32::INFINITY,
            drift_enabled: false,
            phases,
            corner: (0.0, 2.0),
            supported: vec![false; columns],
            supported_range: None,
            parity: false,
        };
        layer.update_capacity();
        layer
    }

    pub fn set_drift_settings(&mut self, settings: SnowDriftSettings) {
        self.drift_settings = settings.validated();
        self.drift_enabled = true;
        self.update_maximum();
        for i in 0..self.heights.len() {
            self.heights[i] = self.heights[i].min(self.maximum);
            self.packed[i] = self.packed[i].min(self.heights[i]);
        }
        self.refresh_top();
    }

    pub fn set_drift_enabled(&mut self, enabled: bool) {
        self.drift_enabled = enabled;
        self.update_maximum();
        if enabled {
            for i in 0..self.heights.len() {
                self.heights[i] = self.heights[i].min(self.maximum);
                self.packed[i] = self.packed[i].min(self.heights[i]);
            }
            self.refresh_top();
        }
    }

    pub fn count(&self) -> usize {
        self.heights.len()
    }

    pub fn width(&self) -> f32 {
        self.heights.len() as f32 * self.column_width
    }

    pub fn base(&self) -> V2 {
        self.origin + self.offset
    }

    /// Where the snow's own positions along it start: what lies in it keeps its place there as it slides or the
    /// window's left edge moves under it.
    pub fn heap_x(&self) -> f32 {
        self.base().x + self.drift
    }

    pub fn index(&self, x: f32) -> Option<usize> {
        let i = ((x - self.base().x) / self.column_width).floor();
        (i >= 0.0 && (i as usize) < self.heights.len()).then_some(i as usize)
    }

    pub fn clamped_index(&self, x: f32) -> usize {
        let i = ((x - self.base().x) / self.column_width).floor() as i64;
        i.clamp(0, self.heights.len() as i64 - 1) as usize
    }

    pub fn center_x(&self, i: usize) -> f32 {
        self.base().x + (i as f32 + 0.5) * self.column_width
    }

    /// Height of the snow's underside (the bed) at column `i`, in overlay coordinates.
    pub fn floor(&self, i: usize) -> f32 {
        self.base().y + self.bed[i]
    }

    pub fn surface(&self, i: usize) -> f32 {
        self.base().y + self.bed[i] + self.heights[i]
    }

    /// Rounds the bed's ends like the window's corners: a superellipse quadrant of the given radius.
    pub fn shape_corners(&mut self, radius: f32, exponent: f32) {
        if radius == self.corner.0 && exponent == self.corner.1 {
            return;
        }
        self.corner = (radius, exponent);
        self.update_bed();
    }

    /// Moves the snow one column along its support and returns the column that slid off the end.
    pub fn slide_column(&mut self, direction: i32) -> (f32, f32, f32) {
        self.release_support();
        if direction > 0 {
            let last = self.heights.len() - 1;
            let x = self.center_x(last);
            let y = self.floor(last);
            let dropped = self.heights.pop().unwrap_or(0.0);
            self.heights.insert(0, 0.0);
            self.packed.pop();
            self.packed.insert(0, 0.0);
            return (dropped, x, y);
        }
        let x = self.center_x(0);
        let y = self.floor(0);
        let dropped = self.heights.remove(0);
        self.heights.push(0.0);
        self.packed.remove(0);
        self.packed.push(0.0);
        (dropped, x, y)
    }

    pub fn fill(&self, i: usize) -> f32 {
        (1.0 - self.heights[i] / self.capacity[i]).max(0.0)
    }

    pub fn set(&mut self, i: usize, height: f32) {
        self.heights[i] = height.clamp(0.0, self.maximum);
        self.packed[i] = self.packed[i].min(self.heights[i]);
    }

    pub fn replace(&mut self, fresh: Vec<f32>) {
        let count = fresh.len();
        self.heights = fresh;
        self.packed = vec![0.0; count];
        self.visible = vec![true; count];
        self.supported = vec![false; count];
        self.supported_range = None;
        self.update_capacity();
        self.update_bed();
    }

    pub fn resize(&mut self, columns: usize) {
        if columns == self.heights.len() {
            return;
        }
        let mut fresh = self.heights.clone();
        fresh.resize(columns, 0.0);
        self.replace(fresh);
    }

    pub fn update_visibility(&mut self, screen_width: f32, blockers: &[[f32; 4]]) {
        for i in 0..self.heights.len() {
            let x = self.center_x(i);
            let y = self.origin.y + self.bed[i] + 1.0;
            self.visible[i] = x >= 0.0 && x <= screen_width && !blockers.iter().any(|b| x >= b[0] && x < b[2] && y >= b[1] && y < b[3]);
        }
    }

    pub fn refresh_top(&mut self) {
        self.top = self.heights.iter().copied().fold(0.0, f32::max);
    }

    /// Adds snow centred on `x` and returns the area that fell outside the layer.
    pub fn deposit(&mut self, area: f32, x: f32, wide: bool) -> f32 {
        if area <= 0.0 {
            return 0.0;
        }
        if !self.drift_enabled {
            let center = ((x - self.base().x) / self.column_width).floor() as i64;
            let amount = area / self.column_width;
            let kernel: &[(i64, f32)] =
                if wide { &[(-2, 0.0625), (-1, 0.25), (0, 0.375), (1, 0.25), (2, 0.0625)] } else { &[(-1, 0.25), (0, 0.5), (1, 0.25)] };
            let mut missed = 0.0;
            for &(offset, weight) in kernel {
                let i = center + offset;
                if i >= 0 && (i as usize) < self.heights.len() {
                    self.heights[i as usize] += amount * weight;
                } else {
                    missed += area * weight;
                }
            }
            return missed;
        }
        let area = area * self.drift_settings.accumulation as f32 / 100.0;
        let center = ((x - self.base().x) / self.column_width).floor() as i64;
        let size_scale = self.drift_settings.size as f32 / 100.0;
        let spread_points = match self.drift_settings.mode {
            SnowDriftMode::Small => 10.0,
            SnowDriftMode::Large => 58.0,
            SnowDriftMode::Mixed => 28.0,
        } * size_scale;
        let radius = if wide { (spread_points / self.column_width).round().clamp(2.0, 48.0) as i64 } else { 1 };
        let unevenness = 0.12 + self.drift_settings.unevenness as f32 * 0.006;
        let mut total = 0.0;
        for offset in -radius..=radius {
            let distance = offset as f32 / radius.max(1) as f32;
            let smooth = (-3.0 * distance * distance).exp();
            total += smooth * self.profile_factor(center + offset, unevenness);
        }
        if total <= 0.0 {
            return 0.0;
        }
        let mut missed = 0.0;
        for offset in -radius..=radius {
            let distance = offset as f32 / radius.max(1) as f32;
            let weight = (-3.0 * distance * distance).exp() * self.profile_factor(center + offset, unevenness) / total;
            let i = center + offset;
            if i >= 0 && (i as usize) < self.heights.len() {
                let i = i as usize;
                let amount = area / self.column_width * weight;
                let accepted = amount.min((self.maximum - self.heights[i]).max(0.0));
                self.heights[i] += accepted;
            } else {
                missed += area * weight;
            }
        }
        missed
    }

    /// Piles snow against a wall just past column `edge`, as a plough heaps it: a wedge falling away from the wall
    /// at `slope` per column, the way `inward` points.
    pub fn heap(&mut self, area: f32, edge: usize, inward: i32, slope: f32) {
        let amount = area / self.column_width;
        if amount <= 0.0 {
            return;
        }
        let mut low = self.bed[edge] + self.heights[edge];
        let mut high = low + amount;
        for _ in 0..20 {
            let middle = (low + high) / 2.0;
            if self.wedge(middle, edge, inward, slope, 0.0) < amount {
                low = middle;
            } else {
                high = middle;
            }
        }
        let total = self.wedge(high, edge, inward, slope, 0.0);
        if total > 0.0 {
            self.wedge(high, edge, inward, slope, (amount / total).min(1.0));
        }
    }

    /// The snow it takes to raise the surface to a line falling away from `level` at `edge`; adds `scale` of it.
    fn wedge(&mut self, level: f32, edge: usize, inward: i32, slope: f32, scale: f32) -> f32 {
        let reach = if inward > 0 { self.heights.len() - edge } else { edge + 1 };
        let mut total = 0.0;
        for k in 0..reach {
            let line = level - slope * k as f32;
            if line <= -self.bed_depth {
                break;
            }
            let i = if inward > 0 { edge + k } else { edge - k };
            let gap = (line - (self.bed[i] + self.heights[i])).min((self.maximum - self.heights[i]).max(0.0));
            if gap <= 0.0 {
                continue;
            }
            total += gap;
            self.heights[i] += gap * scale;
            self.packed[i] += gap * scale;
        }
        total
    }

    /// Columns touching the cursor are held up by it, so their walls don't slump into the hole until it leaves.
    pub fn hold(&mut self, i: usize) {
        self.supported[i] = true;
        self.supported_range = Some(match self.supported_range {
            Some((low, high)) => (low.min(i), high.max(i)),
            None => (i, i),
        });
    }

    pub fn release_support(&mut self) {
        let Some((low, high)) = self.supported_range.take() else { return };
        for held in &mut self.supported[low..=high] {
            *held = false;
        }
    }

    /// Slumps slopes steeper than the angle of repose and rounds crests, measured on the snow surface over the bed,
    /// so snow slides off rounded corners. Open layers shed what slides past their ends into `spill`.
    pub fn relax(&mut self, repose: f32, rounding: f32) -> bool {
        let count = self.heights.len();
        if count <= 1 || self.top <= 0.0 {
            return false;
        }
        let limit = repose * self.column_width;
        let forward_first = self.parity;
        self.parity = !self.parity;
        let mut moved = false;
        let mut spilled_left = 0.0;
        let mut spilled_right = 0.0;
        let open = self.opens_at_edges;
        let [outside_left, outside_right] = self.outside;
        let h = &mut self.heights;
        let p = &mut self.packed;
        let b = &self.bed;
        let held = &self.supported;

        for pass in 0..6 {
            let forward = (pass % 2 == 0) == forward_first;
            for k in 0..count - 1 {
                let i = if forward { k } else { count - 2 - k };
                if held[i] || held[i + 1] {
                    continue;
                }
                let difference = (b[i] + h[i]) - (b[i + 1] + h[i + 1]);
                if difference > limit + 0.01 {
                        let transfer = ((difference - limit) * 0.5).min(h[i]).min((self.maximum - h[i + 1]).max(0.0));
                        shift(h, p, transfer, i, i + 1);
                    if transfer > 0.0 {
                        moved = true;
                    }
                } else if difference < -limit - 0.01 {
                    let transfer = ((-difference - limit) * 0.5).min(h[i + 1]).min((self.maximum - h[i]).max(0.0));
                    shift(h, p, transfer, i + 1, i);
                    if transfer > 0.0 {
                        moved = true;
                    }
                }
            }
            if open {
                if !held[0] && h[0] - outside_left > limit + 0.01 {
                    let transfer = (h[0] - outside_left - limit) * 0.5;
                    h[0] -= transfer;
                    p[0] = p[0].min(h[0]);
                    spilled_left += transfer;
                    moved = true;
                }
                if !held[count - 1] && h[count - 1] - outside_right > limit + 0.01 {
                    let transfer = (h[count - 1] - outside_right - limit) * 0.5;
                    h[count - 1] -= transfer;
                    p[count - 1] = p[count - 1].min(h[count - 1]);
                    spilled_right += transfer;
                    moved = true;
                }
            }
        }
        // Pulls each slope toward its neighbours' average: rounds crests and notches, leaves even slopes alone.
        for i in 1..count.saturating_sub(2).max(1) {
            if held[i - 1] || held[i] || held[i + 1] || held[i + 2] {
                continue;
            }
            let (s0, s1, s2, s3) = (b[i - 1] + h[i - 1], b[i] + h[i], b[i + 1] + h[i + 1], b[i + 2] + h[i + 2]);
            let kink = (s1 - s2) - 0.5 * ((s0 - s1) + (s2 - s3));
            if kink.abs() <= 0.02 {
                continue;
            }
            let transfer = if kink > 0.0 { (kink * rounding).min(h[i]) } else { (kink * rounding).max(-h[i + 1]) };
            if transfer > 0.0 {
                let transfer = transfer.min((self.maximum - h[i + 1]).max(0.0));
                shift(h, p, transfer, i, i + 1);
            } else {
                let transfer = (-transfer).min((self.maximum - h[i]).max(0.0));
                shift(h, p, transfer, i + 1, i);
            }
            if transfer.abs() > 0.003 {
                moved = true;
            }
        }
        self.spill_left += spilled_left * self.column_width;
        self.spill_right += spilled_right * self.column_width;
        moved
    }

    /// Snow down to `floor` melts slowly; anything heaped above it settles back down over `settle` seconds.
    pub fn thaw(&mut self, dt: f32, floor: f32, rate: f32, settle: f32) -> bool {
        if self.top <= 0.0 {
            return false;
        }
        let mut fastest: f32 = 0.0;
        for (h, packed) in self.heights.iter_mut().zip(&mut self.packed).filter(|(h, _)| **h > 0.0) {
            let loss = h.min((rate + (*h - floor).max(0.0) / settle) * dt);
            *h -= loss;
            *packed = packed.min(*h);
            fastest = fastest.max(loss);
        }
        // Sub-pixel melting is picked up by the idle glint refresh instead of forcing full-rate frames.
        fastest > 0.01
    }

    /// Writes the smoothed snow surface for each column, then the bed under it.
    pub fn write_smoothed(&self, buffer: &mut [f32]) {
        let count = self.heights.len();
        for i in 0..count {
            let left = self.heights[i.saturating_sub(1)];
            let right = self.heights[(i + 1).min(count - 1)];
            buffer[i] = self.bed[i] + 0.25 * left + 0.5 * self.heights[i] + 0.25 * right;
            buffer[count + i] = self.bed[i];
        }
    }

    fn update_bed(&mut self) {
        let (radius, exponent) = self.corner;
        let span = self.width();
        let column_width = self.column_width;
        self.bed = (0..self.heights.len())
            .map(|i| {
                if radius <= 0.0 {
                    return 0.0;
                }
                let x = (i as f32 + 0.5) * column_width;
                let inset = x.min(span - x);
                if inset >= radius {
                    return 0.0;
                }
                let u = 1.0 - inset / radius;
                -radius * (1.0 - (1.0 - u.powf(exponent)).max(0.0).powf(1.0 / exponent))
            })
            .collect();
        self.bed_depth = -self.bed.iter().copied().fold(0.0, f32::min);
    }

    fn update_capacity(&mut self) {
        let (phase0, phase1) = self.phases;
        self.capacity = (0..self.heights.len())
            .map(|i| {
                let x = i as f32 * self.column_width;
                let drift = 0.12 * (x * 0.0105 + phase0).sin() + 0.07 * (x * 0.037 + phase1).sin();
                self.depth * (0.95 + drift)
            })
            .collect();
    }

    fn update_maximum(&mut self) {
        if !self.drift_enabled {
            self.maximum = f32::INFINITY;
        } else if self.opens_at_edges {
            self.maximum = (self.drift_settings.max_height as f32).min(self.depth * 1.2);
        } else {
            self.maximum = self.drift_settings.max_height as f32;
        }
    }

    fn profile_factor(&self, column: i64, amplitude: f32) -> f32 {
        let x = column as f32 * self.column_width;
        let (large, medium, small) = match self.drift_settings.mode {
            SnowDriftMode::Small => (54.0, 24.0, 11.0),
            SnowDriftMode::Large => (360.0, 150.0, 58.0),
            SnowDriftMode::Mixed => (240.0, 92.0, 34.0),
        };
        let scale = self.drift_settings.size as f32 / 100.0;
        let seed = (self.phases.0.to_bits() as u64).rotate_left(17) ^ self.phases.1.to_bits() as u64;
        let noise = 0.55 * smooth_noise(x, large * scale, seed)
            + 0.30 * smooth_noise(x, medium * scale, seed ^ 0x9e3779b97f4a7c15)
            + 0.15 * smooth_noise(x, small * scale, seed ^ 0xd1b54a32d192ed03);
        (1.0 + amplitude * noise).max(0.1)
    }
}

/// Moves snow between columns, packed snow in proportion.
fn shift(h: &mut [f32], p: &mut [f32], amount: f32, from: usize, to: usize) {
    if amount <= 0.0 {
        return;
    }
    let share = if h[from] > 0.0 { p[from].min(amount * p[from] / h[from]) } else { 0.0 };
    h[from] -= amount;
    h[to] += amount;
    p[from] -= share;
    p[to] += share;
}

fn smooth_noise(x: f32, wavelength: f32, seed: u64) -> f32 {
    let coordinate = x / wavelength.max(1.0);
    let cell = coordinate.floor() as i64;
    let fraction = coordinate - cell as f32;
    let blend = fraction * fraction * (3.0 - 2.0 * fraction);
    let a = signed_hash(seed ^ (cell as u64).wrapping_mul(0x9e3779b97f4a7c15));
    let b = signed_hash(seed ^ ((cell + 1) as u64).wrapping_mul(0x9e3779b97f4a7c15));
    a + (b - a) * blend
}

fn signed_hash(value: u64) -> f32 {
    let mut value = value;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^= value >> 31;
    ((value >> 40) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::{SnowDriftMode, SnowDriftSettings, SnowLayer};
    use crate::geometry::V2;

    fn layer(settings: SnowDriftSettings) -> SnowLayer {
        let mut layer = SnowLayer::new(V2::ZERO, 300, 2.0, 40.0, false, (0.7, 1.4));
        layer.set_drift_settings(settings);
        layer
    }

    #[test]
    fn deposits_distribute_mass_without_creating_snow() {
        let mut layer = layer(SnowDriftSettings::default());
        layer.deposit(100.0, 300.0, true);
        let deposited = layer.heights.iter().sum::<f32>() * layer.column_width;
        assert!((deposited - 100.0).abs() < 0.01);
    }

    #[test]
    fn mixed_profile_has_smooth_nonuniform_deposition_even_at_minimum_roughness() {
        let mut settings = SnowDriftSettings::default();
        settings.unevenness = 0;
        let mut layer = layer(settings);
        for i in 0..120 {
            layer.deposit(2.0, 80.0 + i as f32 * 2.0, true);
        }
        let heights = &layer.heights[20..100];
        let range = heights.iter().copied().fold(f32::MIN, f32::max) - heights.iter().copied().fold(f32::MAX, f32::min);
        assert!(range > 0.01);
        let sharpest = heights.windows(2).map(|pair| (pair[0] - pair[1]).abs()).fold(0.0, f32::max);
        assert!(sharpest < 1.5);
    }

    #[test]
    fn large_drift_distribution_is_wider_than_small() {
        let mut small = layer(SnowDriftSettings { mode: SnowDriftMode::Small, ..SnowDriftSettings::default() });
        let mut large = layer(SnowDriftSettings { mode: SnowDriftMode::Large, ..SnowDriftSettings::default() });
        small.deposit(100.0, 300.0, true);
        large.deposit(100.0, 300.0, true);
        let spread = |layer: &SnowLayer| layer.heights.iter().filter(|&&height| height > 0.001).count();
        assert!(spread(&large) > spread(&small));
    }

    #[test]
    fn configured_maximum_height_is_never_exceeded() {
        let mut settings = SnowDriftSettings::default();
        settings.max_height = 20;
        let mut layer = layer(settings);
        for _ in 0..100 {
            layer.deposit(100.0, 300.0, true);
        }
        assert!(layer.heights.iter().all(|&height| height <= 20.0));
    }

    #[test]
    fn changing_drift_shape_keeps_the_existing_mass_and_surface() {
        let mut layer = layer(SnowDriftSettings::default());
        for i in 0..80 {
            layer.deposit(4.0, 100.0 + i as f32 * 2.0, true);
        }
        let before = layer.heights.clone();
        let mass_before = before.iter().sum::<f32>();
        layer.set_drift_settings(SnowDriftSettings { mode: SnowDriftMode::Small, ..SnowDriftSettings::default() });
        assert_eq!(layer.heights, before);
        assert!((layer.heights.iter().sum::<f32>() - mass_before).abs() < 0.001);
    }

    #[test]
    fn rounding_moves_snow_without_changing_its_mass() {
        let mut layer = layer(SnowDriftSettings::default());
        layer.deposit(400.0, 300.0, true);
        layer.refresh_top();
        let before = layer.heights.iter().sum::<f32>();
        for _ in 0..12 {
            layer.relax(4.0, 0.2);
        }
        assert!((layer.heights.iter().sum::<f32>() - before).abs() < 0.001);
    }

    #[test]
    fn thaw_reduces_high_snow_gradually() {
        let mut layer = layer(SnowDriftSettings::default());
        layer.deposit(500.0, 300.0, true);
        layer.refresh_top();
        let before = layer.heights.iter().sum::<f32>();
        layer.thaw(0.1, 0.0, 0.05, 10.0);
        let after = layer.heights.iter().sum::<f32>();
        assert!(after < before);
        assert!(before - after < before * 0.1);
    }
}
