//! The character's cone-ended shape (character.rs `capsule`) as a convex hull with a
//! constant-time support function.
//!
//! The hull is rings of points at the same angles (`i * TAU / per_ring`). parry's
//! `ConvexPolyhedron` finds a support point by comparing every point (194 for the
//! character), and the character's sweeps ask for one many times per triangle they pass:
//! on the TrimUI handheld that comparison was about a third of the main thread in a town
//! [observed: PC samples, 2026-11-07]. The farthest point of a ring in a direction is
//! the one at the direction's own angle, so only the points next to that angle on each
//! ring need comparing.
//!
//! Every answer is the one the hull itself gives, bit for bit: the same point, ties
//! going to the lowest hull index as in parry's scan (physics traces are compared
//! exactly). Where rounding could let a point away from that angle tie (a direction
//! close to vertical, a ring of nearly no radius), the full scan answers. Everything
//! else (contact features, rays, points, mass) is the hull's own.

use rapier3d::math::{Pose, Real, Rotation, Vector};
use rapier3d::parry::bounding_volume::{Aabb, BoundingSphere};
use rapier3d::parry::mass_properties::MassProperties;
use rapier3d::parry::query::{PointProjection, PointQuery, Ray, RayCast, RayIntersection};
use rapier3d::parry::shape::{
    ConvexPolyhedron, FeatureId, PolygonalFeature, PolygonalFeatureMap, Shape, ShapeType, SubShapeId, SupportMap,
    TypedShape,
};

/// Index meaning "this ring point is not a hull point".
const NONE: u32 = u32::MAX;

/// How far below the best a point two angle steps away must be, relative to the size of
/// the dot products, for the shortcut to be safe [engineering choice: an f32 dot product
/// of three terms rounds by under 4e-7 of that size, so this leaves a margin of 25].
const MARGIN: f32 = 1e-5;

/// The most [`rough_angle`] is off by, radians, with room to spare (its bound is 0.0039).
const ANGLE_ERROR: f32 = 0.01;

/// atan2(y, x), within 0.0039 radians: `libm`'s `atan2f` was a fifth of the support
/// function's time. Only the nearest ring step is taken from it, and the steps either
/// side are compared exactly too, so a small error picks the same point.
fn rough_angle(y: f32, x: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let (ax, ay) = (x.abs(), y.abs());
    let z = if ax > ay { ay / ax } else { ax / ay };
    let mut a = z * (FRAC_PI_4 + 0.273 * (1.0 - z));
    if ay > ax {
        a = FRAC_PI_2 - a;
    }
    if x < 0.0 {
        a = PI - a;
    }
    if y < 0.0 {
        -a
    } else {
        a
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RingHull {
    hull: ConvexPolyhedron,
    /// For each ring, for each angle step, the hull index of the point equal to it.
    rings: Vec<Ring>,
    /// Hull points equal to no ring point (the axis ends), always compared.
    extra: Vec<u32>,
    per_ring: usize,
    step: Real,
    /// cos(step / 2) - cos(3 step / 2 - [`ANGLE_ERROR`]): how much nearer the best point
    /// of a ring is to the direction than any point outside the steps compared, per
    /// unit of radius.
    drop: Real,
    local_aabb: Aabb,
}

#[derive(Clone, Debug)]
struct Ring {
    ids: Vec<u32>,
    radius: Real,
    height: Real,
}

impl RingHull {
    /// The convex hull of `points`, laid out as `axis` points and then, for each angle
    /// step `i`, one point of each of the `rings` rings (`axis + i * rings + ring`).
    pub(crate) fn new(points: &[Vector], axis: usize, rings: usize, per_ring: usize) -> Option<Self> {
        if per_ring < 3 || rings == 0 || points.len() != axis + rings * per_ring {
            return None;
        }
        let hull = ConvexPolyhedron::from_convex_hull(points)?;
        let hp = hull.points();
        let find = |p: Vector| hp.iter().position(|h| *h == p).map_or(NONE, |i| i as u32);
        let mut used = vec![false; hp.len()];
        let rings: Vec<Ring> = (0..rings)
            .map(|r| {
                let pts: Vec<Vector> = (0..per_ring).map(|i| points[axis + i * rings + r]).collect();
                let ids: Vec<u32> = pts.iter().map(|&p| find(p)).collect();
                for &id in &ids {
                    if id != NONE {
                        used[id as usize] = true;
                    }
                }
                let radius = pts.iter().map(|p| p.x.hypot(p.y)).fold(0.0, Real::max);
                let height = pts.iter().map(|p| p.z.abs()).fold(0.0, Real::max);
                Ring { ids, radius, height }
            })
            .collect();
        let extra = (0..hp.len()).filter(|&i| !used[i]).map(|i| i as u32).collect();
        let step = std::f32::consts::TAU / per_ring as Real;
        // Never below 0: with no gap to rely on, every ring of radius takes the scan.
        let drop = ((step / 2.0).cos() - (1.5 * step - ANGLE_ERROR).cos()).max(0.0);
        let local_aabb = hull.local_aabb();
        Some(Self { hull, rings, extra, per_ring, step, drop, local_aabb })
    }

    /// Whether no point away from `dir`'s angle can tie with the best one after rounding.
    fn shortcut(&self, dir: Vector) -> bool {
        // Directions are about unit length: no overflow for `hypot` to guard against.
        let flat = (dir.x * dir.x + dir.y * dir.y).sqrt();
        // A NaN direction fails this too, and goes to the scan as before.
        // A ring of no radius is one point repeated: any of its steps is the same answer.
        self.rings.iter().all(|r| {
            if r.radius == 0.0 {
                return flat.is_finite() && dir.z.is_finite();
            }
            let gap = self.drop * r.radius * flat;
            gap > MARGIN * (r.radius * flat + r.height * dir.z.abs())
        })
    }

    fn scan(&self, dir: Vector) -> Vector {
        rapier3d::parry::utils::point_cloud_support_point(dir, self.hull.points())
    }
}

impl SupportMap for RingHull {
    fn local_support_point(&self, dir: Vector) -> Vector {
        if !self.shortcut(dir) {
            return self.scan(dir);
        }
        let pts = self.hull.points();
        let n = self.per_ring as i64;
        let centre = (rough_angle(dir.y, dir.x) / self.step).round() as i64;
        let mut best = (Real::NEG_INFINITY, NONE);
        let mut consider = |id: u32| {
            if id == NONE {
                return;
            }
            let d = pts[id as usize].dot(dir);
            if d > best.0 || (d == best.0 && id < best.1) {
                best = (d, id);
            }
        };
        for off in -1..=1 {
            let i = (centre + off).rem_euclid(n) as usize;
            for r in &self.rings {
                consider(r.ids[i]);
            }
        }
        for &id in &self.extra {
            consider(id);
        }
        if best.1 == NONE {
            return self.scan(dir);
        }
        pts[best.1 as usize]
    }
}

impl PolygonalFeatureMap for RingHull {
    fn local_support_feature(&self, dir: Vector, out_feature: &mut PolygonalFeature) {
        self.hull.local_support_feature(dir, out_feature)
    }

    fn local_support_feature_toward(&self, dir: Vector, hint: Vector, out_feature: &mut PolygonalFeature) {
        self.hull.local_support_feature_toward(dir, hint, out_feature)
    }

    fn is_convex_polyhedron(&self) -> bool {
        self.hull.is_convex_polyhedron()
    }
}

impl RayCast for RingHull {
    fn cast_local_ray_and_get_normal(&self, ray: &Ray, max_time_of_impact: Real, solid: bool) -> Option<RayIntersection> {
        self.hull.cast_local_ray_and_get_normal(ray, max_time_of_impact, solid)
    }
}

impl PointQuery for RingHull {
    fn project_local_point(&self, pt: Vector, solid: bool) -> PointProjection {
        self.hull.project_local_point(pt, solid)
    }

    fn project_local_point_and_get_feature(&self, pt: Vector) -> (PointProjection, FeatureId) {
        self.hull.project_local_point_and_get_feature(pt)
    }
}

impl Shape for RingHull {
    fn compute_local_aabb(&self) -> Aabb {
        self.local_aabb
    }

    fn compute_local_bounding_sphere(&self) -> BoundingSphere {
        self.hull.local_bounding_sphere()
    }

    /// Without rotation (the character's sweeps and body) the hull's box moved: rounding
    /// an addition is monotonic, so the smallest moved point is the moved smallest point.
    fn compute_aabb(&self, position: &Pose) -> Aabb {
        if position.rotation == Rotation::IDENTITY {
            let t = position.translation;
            Aabb::new(self.local_aabb.mins + t, self.local_aabb.maxs + t)
        } else {
            self.hull.aabb(position)
        }
    }

    fn clone_dyn(&self) -> Box<dyn Shape> {
        Box::new(self.clone())
    }

    fn scale_dyn(&self, scale: Vector, num_subdivisions: u32) -> Option<Box<dyn Shape>> {
        self.hull.scale_dyn(scale, num_subdivisions)
    }

    fn mass_properties(&self, density: Real) -> MassProperties {
        self.hull.mass_properties(density)
    }

    fn is_convex(&self) -> bool {
        true
    }

    fn shape_type(&self) -> ShapeType {
        ShapeType::Custom
    }

    fn as_typed_shape(&self) -> TypedShape<'_> {
        TypedShape::Custom(self)
    }

    fn ccd_thickness(&self) -> Real {
        self.hull.ccd_thickness()
    }

    fn ccd_angular_thickness(&self) -> Real {
        self.hull.ccd_angular_thickness()
    }

    fn as_support_map(&self) -> Option<&dyn SupportMap> {
        Some(self as &dyn SupportMap)
    }

    fn as_polygonal_feature_map(&self) -> Option<(&dyn PolygonalFeatureMap, Real)> {
        Some((self as &dyn PolygonalFeatureMap, 0.0))
    }

    fn feature_normal_at_point(&self, subshape: SubShapeId, feature: FeatureId, point: Vector) -> Option<Vector> {
        self.hull.feature_normal_at_point(subshape, feature, point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The character's layout (character.rs `capsule`): two axis ends, then four rings.
    fn shape(radius: f32, flat: f32, cone: f32, half: f32, n: usize) -> Vec<Vector> {
        let c = (std::f32::consts::PI / n as f32).cos();
        let (r, tip) = (radius / c, flat / c);
        let mut points = vec![Vector::new(0.0, 0.0, -half), Vector::new(0.0, 0.0, half)];
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            for (rr, z) in [(tip, -half), (r, cone - half), (r, half - cone), (tip, half)] {
                points.push(Vector::new(rr * a.cos(), rr * a.sin(), z));
            }
        }
        points
    }

    /// A deterministic spread of directions: random, near vertical, exactly vertical,
    /// on ring angles and halfway between them.
    fn directions(n: usize) -> Vec<Vector> {
        let mut out = vec![Vector::Z, -Vector::Z, Vector::X, -Vector::Y, Vector::ZERO];
        let mut s = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
        };
        for _ in 0..20_000 {
            out.push(Vector::new(rnd(), rnd(), rnd()));
            let tilt = 10f32.powf(-6.0 * rnd().abs());
            out.push(Vector::new(rnd() * tilt, rnd() * tilt, rnd().signum()));
        }
        let step = std::f32::consts::TAU / n as f32;
        for i in 0..4 * n {
            let a = i as f32 * step / 2.0;
            for z in [-1.0, -0.3, 0.0, 0.3, 1.0] {
                out.push(Vector::new(a.cos(), a.sin(), z));
            }
        }
        out
    }

    #[test]
    fn support_points_match_the_hulls_scan_bit_for_bit() {
        let cases = [
            (16.0, 2.0, 21.0, 64.0, 48), // a cone bottom with a flat tip
            (16.0, 0.0, 23.0, 64.0, 48), // a pointed cone: the tip rings collapse onto the axis
            (16.0, 16.0, 0.0, 64.0, 48), // flat to the rim: two rings coincide
            (0.3, 0.01, 0.4, 1.28, 48),  // rapier units (k = 0.02)
            (16.0, 2.0, 21.0, 64.0, 7),
        ];
        for (radius, flat, cone, half, n) in cases {
            let points = shape(radius, flat, cone, half, n);
            let fast = RingHull::new(&points, 2, 4, n).expect("hull");
            let all = directions(n);
            let short = all.iter().filter(|d| fast.shortcut(**d)).count();
            // The shortcut answers most directions (the near-vertical ones go to the scan).
            assert!(short * 2 > all.len(), "{radius} {flat} {n}: shortcut for {short} of {}", all.len());
            for d in all {
                let want = rapier3d::parry::utils::point_cloud_support_point(d, fast.hull.points());
                let got = fast.local_support_point(d);
                assert_eq!(
                    (got.x.to_bits(), got.y.to_bits(), got.z.to_bits()),
                    (want.x.to_bits(), want.y.to_bits(), want.z.to_bits()),
                    "{radius} {flat} {cone} {half} {n}: direction {d:?}"
                );
            }
        }
    }

    #[test]
    fn rough_angle_is_within_its_bound() {
        let mut worst = 0f32;
        for i in 0..200_000 {
            let a = i as f32 / 200_000.0 * std::f32::consts::TAU - std::f32::consts::PI;
            for r in [1.0e-6, 1.0, 3.0e4] {
                let (y, x) = (r * a.sin(), r * a.cos());
                let d = (rough_angle(y, x) - y.atan2(x)).abs();
                worst = worst.max(d.min(std::f32::consts::TAU - d));
            }
        }
        assert!(worst < 0.0039, "worst {worst}");
    }

    #[test]
    fn boxes_match_the_hulls_own() {
        let fast = RingHull::new(&shape(16.0, 2.0, 21.0, 64.0, 48), 2, 4, 48).expect("hull");
        for t in [Vector::ZERO, Vector::new(1234.5, -77.25, 3.0e4), Vector::new(-0.1, 1.0e-7, -9.0e5)] {
            let pose = Pose::from_translation(t);
            let (got, want) = (fast.compute_aabb(&pose), fast.hull.aabb(&pose));
            assert!(got.mins == want.mins && got.maxs == want.maxs, "{t:?}: {got:?} against {want:?}");
        }
        let turned = Pose::new(Vector::new(5.0, 6.0, 7.0), Vector::new(0.0, 0.0, 0.4));
        let (got, want) = (fast.compute_aabb(&turned), fast.hull.aabb(&turned));
        assert!(got.mins == want.mins && got.maxs == want.maxs);
    }
}
