# UI Architecture

This is the single source of truth for the **frontend concern**: how the UI is
built, how it talks to the Rust backend, and how it renders parameters and maps.
For the project's guiding principles see [PRINCIPLES.md](PRINCIPLES.md); for the
system as a whole (backend crates, pipeline, IPC surface) see
[ARCHITECTURE.md](ARCHITECTURE.md). This doc does not restate either.

## What the frontend is

Vanilla, **build-less** JavaScript. There is no framework, no TypeScript, no
bundler, no `package.json`, no `node_modules`, no compile step. The browser
loads `ui/index.html`, which loads `ui/src/main.js` as a native ES module
(`<script type="module">`), and the rest of the modules are pulled in with
native `import` / dynamic `import()`. Tauri serves the `ui/` directory directly
(`tauri.conf.json` → `frontendDist: "../ui"`; no `devUrl`, no
`beforeDevCommand`/`beforeBuildCommand`). Editing a file under `ui/src/` and
reloading is the entire dev loop.

Styling is a single hand-written stylesheet, `ui/styles/app.css`. Dark theme —
the scientist works in a dark room and bright UI contaminates imaging. Icons are
inline Lucide SVGs (MIT) embedded in `index.html`.

### File map

```
ui/
  index.html                       static shell: icon bar, content area,
                                    layered preview panel, layer bar, status bar
  styles/app.css                   single stylesheet (dark theme)
  src/
    main.js                        entry point: navigation, layered preview/
                                   visualization, status bar, global IPC
                                   listeners, window.openISI API for views
    param-form.js                  descriptor-driven form builder
                                   (backend param descriptors → HTML inputs)
    views/
      library.js                   .oisi file explorer (home view)
      session.js                   acquisition workflow rail
                                   (Setup → Focus → Protocol → Acquire)
      analysis.js                  single-file analysis + map visualization
    lib/
      errors.js                    error-payload helpers for invoke catch blocks
      error-codes.generated.js     GENERATED catalog of error codes/categories
```

## Backend IPC

Tauri runs with `withGlobalTauri: true`, so the runtime is reached through the
injected global rather than an npm import:

```js
const { invoke } = window.__TAURI__.core;    // call a #[tauri::command]
const { listen } = window.__TAURI__.event;   // subscribe to a backend event
window.__TAURI__.dialog                       // native file/dir dialogs
```

There are two directions:

- **Commands (`invoke`)** — request/response calls into Rust commands. Examples:
  `get_monitors`, `select_display`, `validate_display`, `enumerate_cameras`,
  `connect_camera`, `set_exposure`, `capture_anatomical`, `get_experiment`,
  `update_experiment`, `get_duration_summary`, `start_preview` / `stop_preview`,
  `validate_timing`, `start_acquisition` / `stop_acquisition`,
  `save_acquisition`, `list_oisi_files`, `inspect_oisi`, `run_analysis`,
  `read_result`, `read_anatomical`, `export_map_png`, `import_snlc`,
  `migrate_oisi`. Parameters are passed as a JS object; the backend serializes
  results to JSON.
- **Events (`listen`)** — backend-pushed streams the UI subscribes to. Examples:
  `camera:frame`, `camera:status`, `camera:enumerated`, `stimulus:preview`,
  `stimulus:frame`, `stimulus:stopped`, `stimulus:complete`, `params:changed`,
  `analysis:started` / `analysis:complete` / `analysis:failed` /
  `analysis:cancelled`, and a generic `error` event for the status bar. `listen`
  returns an unlisten function; views collect these and call them on teardown.

`main.js` exposes a small `window.openISI` facade (`showView`, `enableView`,
`invoke`, `listen`, the `viz` visualization state, and the layer-rendering
helpers) so the view modules share one navigation/visualization surface instead
of each re-deriving it.

### Error handling

The backend serializes failures as a structured `AppErrorWire` object —
`{ category, code, message, … }` — but legacy paths may still reject with a bare
string. Every `catch` block that surfaces an error to the user routes the value
through `errorToString(e)` from `ui/src/lib/errors.js`, which handles both shapes
(and never produces `"[object Object]"`).

`errors.js` also exposes `errorCode(e)` and `errorCategory(e)` for branching on
error class. The stable code/category vocabularies are **not** invented in the
frontend: they are imported from `ui/src/lib/error-codes.generated.js`, which is
generated from the Rust error enums (`AppError` / `AnalysisError` /
`AcquisitionError`) and regenerated via
`OISI_REGEN_ERROR_CODES=1 cargo test -p openisi error_codes_js_in_sync`. Branch
on `ERROR_CODES.E_…` / `ERROR_CATEGORIES.…`, never on a string literal, so the
frontend cannot drift from the backend.

## Parameter forms (descriptor-driven)

The old `define_params!`/registry macro system is gone. Parameters are now typed
serde configs in the backend, surfaced to the UI as **descriptors**. The
frontend never hardcodes a parameter's type, range, label, unit, or enum values
— it asks the backend and builds the form from the answer.

The contract lives in `ui/src/param-form.js`:

- **`fetchGroupDescriptors(invoke, group)`** → calls `get_param_descriptors`
  with a `group` filter and returns the descriptors. A descriptor is
  `{ id, label, unit, param_type, value, constraint, active, group }`.
- **`buildParamInput(desc)` / `buildParamGroup(descriptors, title)`** → render a
  descriptor to an HTML input (or a titled card of inputs). Type drives the
  widget: `bool` → checkbox, `enum` → `<select>` built from
  `constraint.values` (each an `{ value, label }` pair, where `value` is the
  wire string and `label` the human display), `string` → text, numeric
  (`u16`/`u32`/`i32`/`usize`/`f64`) → `<input type="number">` with min/max/step
  from `constraint`. Inputs carry `data-param-id`; rows carry `data-param-row`.
  Inactive descriptors render nothing.
- **`wireParamListeners(container, invoke, onAfterSet)`** → on change, reads the
  element value, validates against the constraint, and calls
  `set_params { updates: { [id]: value } }`. On failure the input is marked
  `.invalid`. An optional `onAfterSet` callback lets a view react (e.g. the
  Protocol section recomputes the duration summary; Analysis schedules a
  re-analyze).
- **`applyParamChanges(container, changes)`** → the reactive path. The backend
  emits `params:changed` when a parameter's value, constraint, or active-state
  changes server-side (e.g. selecting an envelope reveals/hides dependent
  fields, or tightens a range). `main.js` subscribes globally and patches the
  live DOM in place — updating values, rebuilding enum options, adjusting
  numeric min/max, and showing/hiding rows — without a full re-render.

Analysis takes this one step further: `analysis.js` asks the backend for the
pipeline stages via `get_analysis_stages`, then renders one descriptor group per
stage (`buildParamGroup`). Adding a stage in Rust surfaces it in the UI with no
frontend edit. Every analysis knob — including method pickers — is a descriptor;
there is no separate "method tunable" code path.

## Views

A view is an ES module under `ui/src/views/` exporting
`async render(container)`. `render` populates the container and may return a
`cleanup()` function, which `main.js` calls before switching away (used to drop
event listeners and intervals). Views are lazy-loaded with dynamic `import()`
and cached. Navigation is gated by a `viewState` map: Library is always enabled;
Session is enabled by "New Session"; Analysis is enabled when a file is opened
for analysis. Switching views preserves each view's module-level state.

- **Library** (`library.js`, home view) — lists `.oisi` files from the data
  directory (`list_oisi_files`), sortable by name/date/size, with multi-select
  and delete. Buttons: New Session, Import SNLC `.mat` (`import_snlc`), Download
  Sample Data, Set Data Directory. "Analyze" on a row sets the active file
  (`set_active_oisi`), enables and shows the Analysis view.
- **Session** (`session.js`) — accordion workflow rail with four sections:
  **Setup** (monitor select/validate, physical size, viewing distance, mount
  rotation, camera scan/connect), **Focus** (exposure slider, head-ring overlay
  with drag/scroll interaction, anatomical capture — preview expands to full
  height), **Protocol** (descriptor-driven stimulus/rig/geometry/timing groups,
  conditions list, presentation/timing, saved-experiment load/save, live
  duration summary, stimulus preview), and **Acquire** (readiness checklist,
  timing characterization, live acquisition dashboard fed by `stimulus:frame`,
  save/discard on `stimulus:complete`). Physical geometry lives in Setup, not
  Protocol: the rig's position is a property of the setup, not the stimulus.
- **Analysis** (`analysis.js`) — single-file view. Inspects the `.oisi`
  (`inspect_oisi`), renders per-stage parameter groups, and auto-runs analysis
  if the file has data but no results (auto-migrating pre-2026 files via
  `migrate_oisi`). Param edits debounce a re-analyze; `run_analysis` enqueues
  work on a backend worker thread and the UI tracks progress via the
  `analysis:started/complete/failed/cancelled` events. Exposes the head-ring
  controls and PNG export (`export_map_png`).

## Layered preview & map visualization

The preview panel (right side of `index.html`) stacks canvases in one container:
`layer-base` (camera frame or anatomical), `layer-map` (colormapped result
overlay, with opacity + CSS `mix-blend-mode`), `layer-borders` (area borders),
and `layer-ring` (interactive head-ring overlay). A separate stimulus canvas
shows the live stimulus preview. The **layer bar** (right edge) toggles each
layer and its options via popups.

Visualization state lives in the `viz` object in `main.js` and is persisted to
`localStorage` (`openisi_viz`); display preferences such as base mode, current
map, opacity, blend mode, and overlay toggles survive relaunch. Rendering is
done directly with the Canvas 2D API:

- `camera:frame` / `stimulus:preview` events carry PNG bytes; the UI blobs them
  into an `Image` and draws to the relevant canvas.
- Map results are fetched with `read_result { path, name }`, which returns typed
  data: `scalar_map` (rendered with a jet colormap, with min/max autoscaling and
  optional masked/SNR-transparent regions), `bool_mask` (black where true), or
  `label_map` (area patches colored red/blue by VFS sign, using a cached
  `area_signs` result). The map-overlay popup is built dynamically from the
  results actually present in the file (`inspect_oisi`), with human-readable
  labels.
- The head ring is stored server-side (`get_ring_overlay` / `set_ring_overlay`)
  and is editable by drag (move center) and scroll (resize) directly on the
  canvas, kept in sync with the Focus/Analysis numeric inputs.

## Startup

`DOMContentLoaded` wires the camera/stimulus preview listeners, the layer bar
and ring interaction, the global IPC listeners, and the status bar (refreshed on
an interval via `get_workspace_status`), then shows the Library view and runs
`autoSetup()` — which auto-selects a stimulus monitor, validates the display,
enumerates cameras, and connects the first one found.
