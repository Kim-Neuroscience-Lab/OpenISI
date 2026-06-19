# Pinned oracle versions

The faithful-reproduction goldens (`crates/isi-analysis/tests/golden/`) validate
OpenISI's methods against these **exact** vendored reference versions. "Faithful
to oracle X" means *faithful to the commit pinned here* — re-diff against the pin
to detect upstream drift (per `docs/PIPELINE_METHODS.md` §6b, version-adoption
policy). Each repo is vendored with its own `.git`; the commit below is its
`HEAD`.

| repo (`reference/…`) | upstream | commit | dated | role |
|---|---|---|---|---|
| `ISI` | [zhuangjun1981 ← Garrett/Marshel/Callaway **SNLC** MATLAB](https://github.com/) | `175f012d0be1208a851ca26939066ddb4c66756c` | 2019-11-06 | **SNLC oracle** — `splitPatchesX`/`fusePatchesX`, `getMouseAreasX`, `Gprocesskret`+`adaptiveSmoother`, `overlaymaps`, `getMagFactors`, `getAreaBorders`. |
| `corticalmapping` | [zhuangjun1981/corticalmapping](https://github.com/zhuangjun1981/corticalmapping) | `0ddd261b3993f5ce5608adfbd98a588afc56d20c` | 2020-07-13 | **Allen oracle — movie processing.** The ONLY source for `normalize_movie`, `generatePhaseMap2`, `getMappingMovies` (`isRectify`); `retinotopic_mapping` lacks these. |
| `retinotopic_mapping` | [zhuangjun1981/retinotopic_mapping](https://github.com/zhuangjun1981/retinotopic_mapping) | `eb5a57f00b4e80950c6fc00d58bc0ea0e21b4f3f` | 2024-08-20 | **Allen oracle — canonical published segmentation.** Canonical for `getVisualSpace`/`_splitPatches`/`_mergePatches` (the only function that changed 2020↔2024: `getVisualSpace` boundary `<=`→`<`; we are on this version). Core fns (`_getSignMap`, `_getDeterminantMap`, `_getRawPatchMap`, `eccentricityMap`) are byte-identical to `corticalmapping`. |
| `NeuroAnalysisTools` | [zhuangjun1981/NeuroAnalysisTools](https://github.com/zhuangjun1981/NeuroAnalysisTools) | `0c7acdb745ef93e009ec538af11252e743f9d430` | 2022-07-07 | Zhuang successor package. **Surveyed 2026-06-18: NO new retinotopy method** — `RetinotopicMapping.py` function inventory is identical to canonical `retinotopic_mapping`; extras are 2P/behavior/motion tooling (out of scope). Not an oracle gap. |
| `wfield` | [jcouto/wfield](https://github.com/jcouto/wfield) | `0befe16679f2503a5126decabcd2a66e2d55d710` | 2025-08-26 | Couto / Churchland-lineage widefield. **Surveyed 2026-06-18: NOT a new retinotopy lineage** — its `visual_sign_map` is verbatim "adapted from the Allen retinotopy code" (same VFS we have) and it has NO segmentation/patch code. Genuine novelty = *preprocessing* OpenISI lacks: motion correction (general cv2 registration) + SVD/low-rank denoising; hemodynamic correction is **fluorescence-only (N/A to reflectance ISI)**. These are optional capability adds, NOT faithfulness gaps; ISI value unproven. |

**Reconciliation note (2026-06-18, roadmap 6a):** `corticalmapping` (2020) and
`retinotopic_mapping` (2024) are algorithmically equivalent for every function we
golden against, except `getVisualSpace` (a boundary tightening) — and our golden
was already transcribed from the canonical 2024 version. No re-pin needed. See
`docs/PIPELINE_METHODS.md` §11.

> `KimLabISI` (also present under `reference/`) is **NOT an oracle** — an old,
> failed attempt. Do not validate against it.

The canonical per-method `repo@commit / file:lines` pins land in each method's
structured citation during the 5b naming/citation pass; this file is the
repo-level source of truth in the meantime.
