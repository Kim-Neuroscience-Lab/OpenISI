"""Golden for Garrett-2014 eccentricity vs Allen `eccentricityMap`
(RetinotopicMapping.py:729-760), verbatim:
    eccMap = arctan( sqrt( tan(alt-altC)^2
                           + tan(azi-aziC)^2 / cos(alt-altC)^2 ) ) * 180/pi

This validates the per-pixel great-circle distance formula
(`math::eccentricity_pixel_deg`), the faithfulness-critical core of
`EccentricityMethod::Garrett2014WholeCortexV1`. (The V1-center-of-mass step
that picks altC/aziC is our orchestration on top and is exercised separately
by the pipeline.) Fixed center altC=5, aziC=10 deg; |alt-altC|,|azi-aziC| < 90
so tan/cos are well defined.

Output: fixtures/ecc_{alt,azi,golden}.bin (float64 row-major 64x64)
Run:  python gen_ecc_golden.py
"""
import os
import numpy as np

N = 64
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
ALTC, AZIC = 5.0, 10.0

alt = np.linspace(-30.0, 30.0, N)[:, None] * np.ones((1, N))   # varies along rows
azi = np.ones((N, 1)) * np.linspace(-40.0, 40.0, N)[None, :]   # varies along cols

a2 = (alt - ALTC) * np.pi / 180.0
z2 = (azi - AZIC) * np.pi / 180.0
ecc = np.arctan(np.sqrt(np.tan(a2) ** 2 + np.tan(z2) ** 2 / np.cos(a2) ** 2)) * 180.0 / np.pi

for nm, arr in [("ecc_alt.bin", alt), ("ecc_azi.bin", azi), ("ecc_golden.bin", ecc)]:
    np.ascontiguousarray(arr, dtype="<f8").tofile(os.path.join(FIX, nm))

print(f"  ecc range [{ecc.min():.4f}, {ecc.max():.4f}] deg  center=({ALTC},{AZIC})")
