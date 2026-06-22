"""Golden for `spherical_uv_to_angle` / `spherical_angle_to_uv`
(openisi-stimulus/src/geometry.rs:246) against a VERBATIM transcription of the
Allen/Zhuang Marshel-2012 spherical correction kernel in
`MonitorSetup.Monitor.remap()` (reference/.../MonitorSetup.py:177-206).

ORACLE (the exact lines Allen runs, no offsets / center_coordinates here):
    new_map_x[:, j] = (180/pi) * arctan(lin_coord_x / dis)
    dis2            = sqrt(dis^2 + lin_coord_x^2)
    new_map_y[i, j] = (180/pi) * arctan(lin_coord_y / dis2)
i.e. forward (cm -> deg):
    deg_x = atan(x_cm / dis)
    deg_y = atan(y_cm / sqrt(dis^2 + x_cm^2))
This is the inverse of our Rust `spherical_uv_to_angle` direction:
    az = atan(y_cm / xo)
    el = asin(z_cm / sqrt(xo^2 + y_cm^2 + z_cm^2))   <- claims to match
Note asin(z/sqrt(d2^2+z^2)) == atan(z/d2) for d2=sqrt(xo^2+y^2), so the two
forms are the same transform; this golden PROVES that numerically against the
real Allen formula, not just via round-trip.

We map cm <-> UV with bisector=0, center=0 so:
    y_cm = (u-0.5)*W ,  z_cm = (0.5-v)*H
and the Rust DisplayGeometry(Spherical, dis=DIS, 0,0,0,0, W,H, Wpx,Hpx) will
reproduce exactly these cm. We store the cm grid plus the oracle degrees in f64.
Allen casts the kernel to float32; we deliberately store f64 (Allen's formula in
double) because our Rust is f64 -- the math, not the float32 quantization, is the
faithfulness claim. (float32 max-abs-diff vs f64 reported below for context.)

Fixtures (all little-endian, C-order):
  fixtures/sph_marshel_cm.npy   <f8 [N,2]  (y_cm, z_cm) input cm coords
  fixtures/sph_marshel_deg.npy  <f8 [N,2]  (az_deg, el_deg) oracle (cm->deg)
  fixtures/sph_marshel_meta.npy <f8 [4]    (DIS, W, H, Nrows)
Run:  python gen_spherical_marshel_golden.py
"""
import os
import numpy as np

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)

# Rig-realistic geometry (cm). Wide enough to push azimuth toward +/-50 deg and
# elevation past +/-30 deg so the eccentricity coupling (dis2) is exercised hard.
DIS = 15.0        # viewing distance cm
W = 53.0          # display width cm
H = 30.0          # display height cm


def allen_remap_kernel(x_cm, y_cm, dis):
    """VERBATIM Allen MonitorSetup.remap math (per element), no center offset.

    Only change from source: operate on scalars (the source loops i,j over the
    grid) and keep f64 instead of the source's float32 cast. No formula altered.
    """
    deg_x = (180.0 / np.pi) * np.arctan(x_cm / dis)            # azimuth
    dis2 = np.sqrt(np.square(dis) + np.square(x_cm))
    deg_y = (180.0 / np.pi) * np.arctan(y_cm / dis2)          # altitude
    return deg_x, deg_y


def allen_remap_kernel_f32(x_cm, y_cm, dis):
    """Exact float32 path Allen actually executes (for context only)."""
    nm = np.zeros((), dtype=np.float32)
    deg_x = np.float32((180.0 / np.pi) * np.arctan(np.float32(x_cm) / dis))
    dis2 = np.sqrt(np.square(dis) + np.square(np.float32(x_cm)))
    deg_y = np.float32((180.0 / np.pi) * np.arctan(np.float32(y_cm) / dis2))
    return float(deg_x), float(deg_y)


def main():
    # Stress grid in cm: corners, edges, center, and points at extreme azimuth
    # (large |x_cm|) crossed with extreme elevation (large |y_cm|) so that the
    # dis2 = sqrt(dis^2+x_cm^2) coupling materially changes deg_y.
    xs = np.array([-W / 2, -W / 4, -1.3, 0.0, 1.3, W / 4, W / 2], dtype=np.float64)
    ys = np.array([-H / 2, -H / 4, -0.7, 0.0, 0.7, H / 4, H / 2], dtype=np.float64)

    cm = []
    deg = []
    f32_diffs = []
    for x in xs:
        for y in ys:
            dx, dy = allen_remap_kernel(x, y, DIS)
            cm.append((x, y))
            deg.append((dx, dy))
            fx, fy = allen_remap_kernel_f32(x, y, DIS)
            f32_diffs.append(abs(fx - dx))
            f32_diffs.append(abs(fy - dy))

    cm = np.asarray(cm, dtype="<f8")
    deg = np.asarray(deg, dtype="<f8")
    meta = np.asarray([DIS, W, H, float(cm.shape[0])], dtype="<f8")

    np.save(os.path.join(FIX, "sph_marshel_cm.npy"), np.ascontiguousarray(cm))
    np.save(os.path.join(FIX, "sph_marshel_deg.npy"), np.ascontiguousarray(deg))
    np.save(os.path.join(FIX, "sph_marshel_meta.npy"), np.ascontiguousarray(meta))

    print(f"  DIS={DIS} W={W} H={H}  N={cm.shape[0]} cm->deg samples")
    print(f"  az range  [{deg[:,0].min():.4f}, {deg[:,0].max():.4f}] deg")
    print(f"  el range  [{deg[:,1].min():.4f}, {deg[:,1].max():.4f}] deg")
    print(f"  y_cm range[{cm[:,0].min():.3f}, {cm[:,0].max():.3f}]  "
          f"z_cm range[{cm[:,1].min():.3f}, {cm[:,1].max():.3f}]")
    print(f"  abs(az)+abs(el) sum = {np.abs(deg).sum():.6f}")
    print(f"  oracle float32-vs-f64 max diff = {max(f32_diffs):.3e} deg "
          f"(stored as f64)")

    # Sanity: cross-check the asin form (our Rust el) equals Allen's atan form.
    x, y = -W / 2, H / 2
    dx, dy = allen_remap_kernel(x, y, DIS)
    r = np.sqrt(DIS**2 + x**2 + y**2)
    el_asin = (180.0 / np.pi) * np.arcsin(y / r)
    print(f"  identity check: atan-form el={dy:.10f}  asin-form el={el_asin:.10f}  "
          f"diff={abs(dy-el_asin):.2e}")


if __name__ == "__main__":
    main()
