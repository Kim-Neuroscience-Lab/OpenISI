# Dev workflow: generated figures

For dev debugging without launching the UI, the headless binary's
`--figures` flag exports per-result-map PNGs — jet colormap for scalars,
black/white for boolean masks, red/blue by area sign for label maps,
anatomical as grayscale.

**Location.** Output lands in `dev_figures/<oisi_stem>/<run_tag>/` at the
repo root, separate from the user's data directory so dev artifacts don't
intermingle with real recordings. `dev_figures/` is gitignored.

**Run tag.** `<device>-<UTC-timestamp>`, e.g. `cuda-20260603T1145`. Tag
components are pulled from `compute::device_tag()` after resolution (`cuda` /
`cpu`) and the current UTC minute (`default_figures_dir` in
`src-tauri/src/bin/headless/figures.rs`). Runs that differ by device or
minute land in distinct directories; side-by-side comparison across devices is
`ls dev_figures/<stem>/`. (Two runs that differ only in baseline mode within
the same UTC minute share a tag — use an explicit `--figures <path>` to keep
them separate.)

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
