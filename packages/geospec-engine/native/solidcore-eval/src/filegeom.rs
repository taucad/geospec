//! A file face's surface from its STEP definition, for the distance of a point from
//! it: analytic surfaces in closed form, B-spline surfaces by de Boor's algorithm with
//! first derivatives and Gauss-Newton foot points. Taken from the R3v2 diagnostics
//! (`truth.rs`) unchanged, so the closure test's edge gaps are the M16 census's.

// Ported as measured; its indices are bounded by the knot vectors and control nets
// `Nurbs::new` checks.
#![allow(clippy::indexing_slicing, clippy::neg_cmp_op_on_partial_ord)]

use step_import::model::{Frame, Surface};

pub(crate) type V3 = [f64; 3];

pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn sc(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

const TAU: f64 = std::f64::consts::TAU;

/// A non-uniform rational B-spline surface with full knot vectors.
pub struct Nurbs {
    p: usize,
    q: usize,
    nu: usize,
    nv: usize,
    pts: Vec<V3>,
    w: Vec<f64>,
    uk: Vec<f64>,
    vk: Vec<f64>,
    pub dom: [f64; 4],
    /// Closed in u (v): the surface meets itself across the domain; the chart then
    /// unwraps that direction by its period, as for a cylinder.
    pub closed: [bool; 2],
}

fn full_knots(knots: &[f64], mults: &[u32]) -> Vec<f64> {
    let mut out = Vec::new();
    for (k, m) in knots.iter().zip(mults) {
        for _ in 0..*m {
            out.push(*k);
        }
    }
    out
}

fn find_span(n: usize, p: usize, u: f64, k: &[f64]) -> usize {
    // n = number of control points
    if u >= k[n] {
        let mut s = n - 1;
        while s > p && k[s] >= k[n] {
            s -= 1;
        }
        return s;
    }
    if u <= k[p] {
        let mut s = p;
        while s + 1 < n && k[s + 1] <= k[p] {
            s += 1;
        }
        return s;
    }
    let (mut lo, mut hi) = (p, n);
    let mut mid = (lo + hi) / 2;
    while u < k[mid] || u >= k[mid + 1] {
        if u < k[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
        mid = (lo + hi) / 2;
    }
    mid
}

/// Basis functions and their first derivatives at `u` in span `i` (Piegl-Tiller A2.3).
fn ders1(i: usize, u: f64, p: usize, k: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut ndu = vec![vec![0.0; p + 1]; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = u - k[i + 1 - j];
        right[j] = k[i + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = if ndu[j][r] != 0.0 {
                ndu[r][j - 1] / ndu[j][r]
            } else {
                0.0
            };
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }
    let n0: Vec<f64> = (0..=p).map(|j| ndu[j][p]).collect();
    let mut n1 = vec![0.0; p + 1];
    if p > 0 {
        for r in 0..=p {
            // first derivative: p * (N_{r-1,p-1}/(k[i+r]-k[i+r-p]) - N_{r,p-1}/(k[i+r+1]-k[i+r+1-p]))
            let mut d = 0.0;
            if r >= 1 {
                let den = ndu[p][r - 1];
                if den != 0.0 {
                    d += ndu[r - 1][p - 1] / den;
                }
            }
            if r < p {
                let den = ndu[p][r];
                if den != 0.0 {
                    d -= ndu[r][p - 1] / den;
                }
            }
            n1[r] = d * p as f64;
        }
    }
    (n0, n1)
}

impl Nurbs {
    fn new(b: &step_import::model::BSplineSurface) -> Option<Self> {
        let (p, q) = (b.u_degree as usize, b.v_degree as usize);
        let nu = b.points.len();
        let nv = b.points.first()?.len();
        let uk = full_knots(&b.u_knots, &b.u_mults);
        let vk = full_knots(&b.v_knots, &b.v_mults);
        if uk.len() != nu + p + 1 || vk.len() != nv + q + 1 {
            return None;
        }
        let mut pts = Vec::with_capacity(nu * nv);
        let mut w = Vec::with_capacity(nu * nv);
        for i in 0..nu {
            if b.points[i].len() != nv {
                return None;
            }
            for j in 0..nv {
                pts.push(b.points[i][j]);
                w.push(b.weights.as_ref().map_or(1.0, |ws| ws[i][j]));
            }
        }
        let dom = [uk[p], uk[nu], vk[q], vk[nv]];
        let mut n = Self {
            p,
            q,
            nu,
            nv,
            pts,
            w,
            uk,
            vk,
            dom,
            closed: [false; 2],
        };
        let scale = n.pts.iter().map(|p| norm(*p)).fold(1.0, f64::max);
        let meets = |a: &Self, along_u: bool| {
            (0..=16).all(|k| {
                let t = k as f64 / 16.0;
                let (x, y) = if along_u {
                    let v = dom[2] + (dom[3] - dom[2]) * t;
                    (a.eval_raw(dom[0], v).0, a.eval_raw(dom[1], v).0)
                } else {
                    let u = dom[0] + (dom[1] - dom[0]) * t;
                    (a.eval_raw(u, dom[2]).0, a.eval_raw(u, dom[3]).0)
                };
                norm(sub(x, y)) <= 1e-9 * scale
            })
        };
        n.closed = [meets(&n, true), meets(&n, false)];
        Some(n)
    }

    fn period(&self, dir_v: bool) -> f64 {
        if dir_v {
            self.dom[3] - self.dom[2]
        } else {
            self.dom[1] - self.dom[0]
        }
    }

    /// Wraps the closed directions into the domain.
    fn wrap(&self, u: f64, v: f64) -> (f64, f64) {
        let w = |x: f64, lo: f64, per: f64, closed: bool| {
            if closed && (x < lo || x > lo + per) {
                lo + (x - lo).rem_euclid(per)
            } else {
                x
            }
        };
        (
            w(u, self.dom[0], self.period(false), self.closed[0]),
            w(v, self.dom[2], self.period(true), self.closed[1]),
        )
    }

    fn eval(&self, u: f64, v: f64) -> (V3, V3, V3) {
        let (u, v) = self.wrap(u, v);
        self.eval_raw(u, v)
    }

    fn eval_raw(&self, u: f64, v: f64) -> (V3, V3, V3) {
        let su = find_span(self.nu, self.p, u, &self.uk);
        let sv = find_span(self.nv, self.q, v, &self.vk);
        let (nu0, nu1) = ders1(su, u, self.p, &self.uk);
        let (nv0, nv1) = ders1(sv, v, self.q, &self.vk);
        let (mut a, mut au, mut av) = ([0.0; 3], [0.0; 3], [0.0; 3]);
        let (mut w, mut wu, mut wv) = (0.0, 0.0, 0.0);
        for (a_i, (b0, b1)) in nu0.iter().zip(&nu1).enumerate() {
            let i = su - self.p + a_i;
            for (b_j, (c0, c1)) in nv0.iter().zip(&nv1).enumerate() {
                let j = sv - self.q + b_j;
                let idx = i * self.nv + j;
                let ww = self.w[idx];
                let pt = self.pts[idx];
                let f = b0 * c0 * ww;
                let fu = b1 * c0 * ww;
                let fv = b0 * c1 * ww;
                for d in 0..3 {
                    a[d] += f * pt[d];
                    au[d] += fu * pt[d];
                    av[d] += fv * pt[d];
                }
                w += f;
                wu += fu;
                wv += fv;
            }
        }
        let s = sc(a, 1.0 / w);
        let s_u = sc(sub(au, sc(s, wu)), 1.0 / w);
        let s_v = sc(sub(av, sc(s, wv)), 1.0 / w);
        (s, s_u, s_v)
    }

    /// Gauss-Newton foot point of `x` from `seed`, clamped to the domain.
    fn foot(&self, x: V3, seed: (f64, f64)) -> (f64, f64) {
        let (mut u, mut v) = seed;
        let [u0, u1, v0, v1] = self.dom;
        for _ in 0..60 {
            let (s, su, sv) = self.eval(u, v);
            let r = sub(s, x);
            let (a, b, c) = (dot(su, su), dot(su, sv), dot(sv, sv));
            let (g1, g2) = (-dot(su, r), -dot(sv, r));
            let det = a * c - b * b;
            if !(det.abs() > 1e-300) {
                break;
            }
            let du = (c * g1 - b * g2) / det;
            let dv = (a * g2 - b * g1) / det;
            let nu = if self.closed[0] {
                u + du
            } else {
                (u + du).clamp(u0, u1)
            };
            let nv = if self.closed[1] {
                v + dv
            } else {
                (v + dv).clamp(v0, v1)
            };
            let step = ((nu - u) / (u1 - u0)).abs() + ((nv - v) / (v1 - v0)).abs();
            u = nu;
            v = nv;
            if step < 1e-15 {
                break;
            }
        }
        (u, v)
    }

    fn global_foot(&self, x: V3) -> (f64, f64) {
        let [u0, u1, v0, v1] = self.dom;
        let n = 24;
        let mut cands: Vec<(f64, f64, f64)> = Vec::new();
        for i in 0..=n {
            for j in 0..=n {
                let u = u0 + (u1 - u0) * i as f64 / n as f64;
                let v = v0 + (v1 - v0) * j as f64 / n as f64;
                let d = norm(sub(self.eval(u, v).0, x));
                cands.push((d, u, v));
            }
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut best = (f64::INFINITY, u0, v0);
        for c in cands.iter().take(4) {
            let (u, v) = self.foot(x, (c.1, c.2));
            let d = norm(sub(self.eval(u, v).0, x));
            if d < best.0 {
                best = (d, u, v);
            }
        }
        (best.1, best.2)
    }
}

/// A face's surface from its STEP definition.
pub enum FSurf {
    Plane(Frame),
    Cyl(Frame, f64),
    Cone(Frame, f64, f64),
    Sphere(Frame, f64),
    Torus(Frame, f64, f64),
    Nurbs(Box<Nurbs>),
}

fn local(f: &Frame, p: V3) -> V3 {
    let d = sub(p, f.origin);
    [dot(d, f.x), dot(d, f.y), dot(d, f.z)]
}

fn unwrap(prev: f64, x: f64) -> f64 {
    unwrap_p(prev, x, TAU)
}

fn unwrap_p(prev: f64, x: f64, per: f64) -> f64 {
    x + per * ((prev - x) / per).round()
}

impl FSurf {
    pub fn new(s: &Surface) -> Option<Self> {
        Some(match s {
            Surface::Plane { frame } => Self::Plane(*frame),
            Surface::Cylinder { frame, radius } => Self::Cyl(*frame, *radius),
            Surface::Cone {
                frame,
                radius,
                semi_angle_deg,
            } => Self::Cone(*frame, *radius, semi_angle_deg.to_radians().tan()),
            Surface::Sphere { frame, radius } => Self::Sphere(*frame, *radius),
            Surface::Torus {
                frame,
                major,
                minor,
            } => Self::Torus(*frame, *major, *minor),
            Surface::BSpline(b) => Self::Nurbs(Box::new(Nurbs::new(b)?)),
            _ => return None,
        })
    }

    /// The period of u (v) when the chart unwraps it.
    fn period_u(&self) -> Option<f64> {
        match self {
            Self::Plane(_) => None,
            Self::Nurbs(n) => n.closed[0].then(|| n.period(false)),
            _ => Some(TAU),
        }
    }

    fn period_v(&self) -> Option<f64> {
        match self {
            Self::Torus(..) => Some(TAU),
            Self::Nurbs(n) => n.closed[1].then(|| n.period(true)),
            _ => None,
        }
    }

    fn periodic_u(&self) -> bool {
        self.period_u().is_some()
    }

    fn periodic_v(&self) -> bool {
        self.period_v().is_some()
    }

    /// Parameters of `p` (closed form, or a foot point from `prev`), unwrapped next
    /// to `prev` in the periodic directions.
    pub fn chart(&self, p: V3, prev: Option<(f64, f64)>) -> (f64, f64) {
        let (mut u, mut v) = match self {
            Self::Plane(f) => {
                let l = local(f, p);
                (l[0], l[1])
            }
            Self::Cyl(f, _) | Self::Cone(f, ..) => {
                let l = local(f, p);
                let rho = l[0].hypot(l[1]);
                let u = match prev {
                    Some(q) if rho < 1e-12 * (1.0 + l[2].abs()) => q.0,
                    _ => l[1].atan2(l[0]),
                };
                (u, l[2])
            }
            Self::Sphere(f, r) => {
                let l = local(f, p);
                let rho = l[0].hypot(l[1]);
                let u = match prev {
                    Some(q) if rho < 1e-12 * r => q.0,
                    _ => l[1].atan2(l[0]),
                };
                (u, l[2].atan2(rho))
            }
            Self::Torus(f, big, _) => {
                let l = local(f, p);
                let rho = l[0].hypot(l[1]);
                (l[1].atan2(l[0]), l[2].atan2(rho - big))
            }
            Self::Nurbs(n) => {
                return match prev {
                    Some(q) => n.foot(p, q),
                    None => n.global_foot(p),
                }
            }
        };
        if let Some(q) = prev {
            if self.periodic_u() {
                u = unwrap(q.0, u);
            }
            if self.periodic_v() {
                v = unwrap(q.1, v);
            }
        }
        (u, v)
    }

    /// Distance of `p` from the surface (B-spline: from the foot point found from
    /// `seed`, else a global search), and the parameters used.
    pub fn dist(&self, p: V3, seed: Option<(f64, f64)>) -> (f64, (f64, f64)) {
        let uv = self.chart(p, seed);
        let d = match self {
            Self::Plane(f) => local(f, p)[2].abs(),
            Self::Cyl(f, r) => {
                let l = local(f, p);
                (l[0].hypot(l[1]) - r).abs()
            }
            Self::Cone(f, r, ta) => {
                let l = local(f, p);
                (l[0].hypot(l[1]) - ta * l[2] - r).abs() / (1.0 + ta * ta).sqrt()
            }
            Self::Sphere(f, r) => (norm(sub(p, f.origin)) - r).abs(),
            Self::Torus(f, big, r) => {
                let l = local(f, p);
                ((l[0].hypot(l[1]) - big).hypot(l[2]) - r).abs()
            }
            Self::Nurbs(n) => norm(sub(n.eval(uv.0, uv.1).0, p)),
        };
        (d, uv)
    }
}
