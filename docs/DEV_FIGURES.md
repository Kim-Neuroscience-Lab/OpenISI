# Dev workflow: generated figures

For dev debugging without launching the UI, the headless binary's
`--figures` flag exports per-result-map PNGs — jet colormap for scalars,
black/white for boolean masks, red/blue by area sign for label maps,
anatomical as grayscale.

**Location.** Output lands in `dev_figures/<oisi_stem>/<run_tag>/` at the
repo root, separate from the user's data directory so dev artifacts don't
intermingle with real recordings. `dev_figures/` is gitignored.

**Run tag.** `<baseline_mode>-<device>-<UTC-timestamp>`, e.g.
`allframes-cuda-20260603T1145`. Tag components are pulled from
`params.baseline_mode`, `compute::device_tag()` after resolution, and the
current UTC minute. Different runs never overwrite each other; side-by-side
comparison across baseline modes or devices is `ls dev_figures/<stem>/`.

**meta.json.** Each run directory contains a JSON file recording the full
reproduction context. Uses portable identifiers from the `.oisi` root
attributes (`animal_id`, `created_at`), not absolute paths, so a
`dev_figures/` directory is shareable across machines:

```json
{
  "source": {
    "filename": "5_14_2026_test5_1778801597.oisi",
    "animal_id": "5/14/2026_test5",
    "created_at": "1778801597"
  },
  "device": "CUDA (Burn dispatch)",
  "baseline_mode": "OutsideSweepWindows",
  "baseline_frame_count": 250,
  "git_sha": "350aa2d",
  "git_dirty": true,
  "git_branch": "main",
  "timestamp_utc": "2026-06-03T11:45:00Z",
  "analysis_params": { ... }
}
```

`source.created_at` is the acquisition's unix timestamp — globally unique
to the recording, survives renames and copies, and identifies the source
without needing a content hash.

**CLI.** `--figures` with no path auto-tags into
`dev_figures/<oisi_stem>/<auto_tag>/`. Explicit `--figures <path>` honors a
custom path (no auto-tag) for one-off comparisons.
