// Analysis view — file inspection, parameter controls, auto re-analyze.
// Matches Juavinett et al. 2017 visualization: area patches (red/blue),
// phase maps (jet, full image), VFS (jet, full image), black borders.

const { invoke } = window.__TAURI__.core;
import { buildParamGroup, wireParamListeners, applyParamChanges, fetchGroupDescriptors } from '../param-form.js';
import { errorToString } from '../lib/errors.js';

let currentFile = null;
let fileInfo = null;

/**
 * Run analysis on the given .oisi file, auto-migrating pre-2026 schema
 * files on the fly. The backend's `run_analysis` refuses pre-2026 files
 * with a clear error message; we catch that case, call `migrate_oisi`
 * (which bumps `/analysis_params` to the current Registry-tree schema),
 * and retry the analysis. Non-migration errors propagate.
 *
 * If `statusEl` is provided, status updates ("Migrating…") are written
 * to it so the user sees the migration step.
 */
async function runAnalysisWithAutoMigrate(invoke, path, statusEl = null) {
    try {
        await invoke("run_analysis", { path });
    } catch (e) {
        const msg = errorToString(e);
        if (msg.includes("pre-2026")) {
            if (statusEl) {
                statusEl.textContent = "Migrating /analysis_params to current schema…";
                statusEl.style.color = "";
            }
            console.log(`[analysis] auto-migrating pre-2026 file ${path}`);
            await invoke("migrate_oisi", { path });
            if (statusEl) statusEl.textContent = "Migrated — re-running analysis…";
            await invoke("run_analysis", { path });
        } else {
            throw e;
        }
    }
}

export async function render(container) {
    currentFile = window.openISI._analysisFile;

    if (!currentFile) {
        container.innerHTML = `
            <div class="empty-state">
                <h2>Analysis</h2>
                <p>Select a .oisi file from the Library to begin.</p>
            </div>
        `;
        return;
    }

    try {
        fileInfo = await invoke("inspect_oisi", { path: currentFile });
    } catch (e) {
        container.innerHTML = `<div class="card"><p style="color: var(--error);">Failed to inspect: ${errorToString(e)}</p></div>`;
        return;
    }

    return await renderAnalysisView(container, fileInfo, currentFile);
}

async function renderAnalysisView(container, fileInfo, currentFile) {

    // Load analysis params (via descriptor registry) and ring config. Per
    // analysis stage we fetch the stage's own GroupId so the UI shows ONE
    // section per pipeline stage (method picker + active tunables together),
    // not one giant flat "all analysis params" list. The stage list is the
    // backend's single source of truth (derived from PARAM_DEFS), fetched via
    // `get_analysis_stages` — NOT hardcoded here, so adding a stage in Rust
    // surfaces it automatically with no frontend edit.
    const STAGES = await invoke("get_analysis_stages");
    const stageDescs = await Promise.all(
        STAGES.map(s => fetchGroupDescriptors(invoke, s.key))
    );
    const ring = await invoke("get_ring_overlay");

    // Compute backend label — auto-detected at startup. Display-only
    // (no override toggle); the best available device is always used.
    let analysisBackend = "—";
    try { analysisBackend = await invoke("get_analysis_backend"); }
    catch (e) { console.warn("get_analysis_backend failed:", errorToString(e)); }

    // Set up the layered preview.
    window.openISI.showPreviewPanel();
    // Wait one frame for layout to compute before measuring panel height.
    await new Promise(r => requestAnimationFrame(r));
    enterAnalysisFocusMode();

    const viz = window.openISI.viz;
    // Always reload if there's no cached data, even if the file path matches.
    const isReturning = viz.analysisFile === currentFile && viz.mapImageData !== null;

    if (!isReturning) {
        // Auto-run analysis if the file has data but no results yet.
        if (!fileInfo.has_results && (fileInfo.has_acquisition || fileInfo.has_complex_maps)) {
            try {
                await runAnalysisWithAutoMigrate(invoke, currentFile);
                fileInfo = await invoke("inspect_oisi", { path: currentFile });
            } catch (e) {
                console.error("Auto-analysis failed:", e);
            }
        }

        await window.openISI.setAnalysisFile(currentFile);
        if (fileInfo.has_anatomical) window.openISI.setBaseMode("anatomical");
        if (fileInfo.has_results) {
            // Default to VFS overlay with borders and ring.
            const defaultMap = viz.availableResults.has("vfs") ? "vfs" : "area_labels";
            await window.openISI.setMapName(defaultMap);
            viz.bordersVisible = true;
            viz.ringVisible = true;
        }
        window.openISI.renderBordersLayer();
        window.openISI.updateRingLayer();
    } else {
        window.openISI.renderMapLayer();
        window.openISI.renderBordersLayer();
        window.openISI.updateRingLayer();
    }
    window.openISI.syncLayerBarUI();

    const filename = currentFile.split(/[\\/]/).pop();

    container.innerHTML = `
        <div class="card">
            <h3>${filename}</h3>
            <div class="form-row">
                <label>Acquisition</label>
                <span class="mono-value">${fileInfo.has_acquisition ? "\u2713" : "\u2014"}</span>
            </div>
            <div class="form-row">
                <label>Results</label>
                <span class="mono-value">${fileInfo.has_results ? `\u2713 (${(fileInfo.results || []).length} results)` : "\u2014"}</span>
            </div>
            <div class="form-row">
                <label>Dimensions</label>
                <span class="mono-value">${fileInfo.dimensions ? `${fileInfo.dimensions[1]} \u00d7 ${fileInfo.dimensions[0]}` : "\u2014"}</span>
            </div>
            <div class="form-row">
                <label>Compute Backend</label>
                <span class="mono-value">${analysisBackend}</span>
            </div>
        </div>

        ${STAGES.map((s, i) => buildParamGroup(stageDescs[i], s.title)).join("")}

        <div class="card">
            <h3>Head Ring</h3>
            <div class="form-row">
                <label>Show</label>
                <input type="checkbox" id="analysis-ring-enabled" ${ring.enabled ? "checked" : ""}>
            </div>
            <div class="form-row">
                <label>Radius (px)</label>
                <input type="number" id="analysis-ring-radius" min="10" max="1000" step="5" value="${ring.radius_px}" style="width:80px">
            </div>
            <div class="form-row">
                <label>Center X</label>
                <input type="number" id="analysis-ring-cx" value="${ring.center_x_px}" style="width:70px">
                <label style="min-width:auto">Y</label>
                <input type="number" id="analysis-ring-cy" value="${ring.center_y_px}" style="width:70px">
            </div>
            <div class="form-row">
                <label>Diameter (mm)</label>
                <input type="number" id="analysis-ring-diam" min="0.1" max="50" step="0.1" value="5.0" style="width:70px">
                <span id="analysis-ring-ppm" class="mono-value" style="font-size:11px">${ring.radius_px > 0 ? `= ${(ring.radius_px * 2 / 5.0).toFixed(1)} px/mm` : ""}</span>
            </div>
        </div>

        <div class="card">
            <div class="form-row">
                <span id="analysis-status" class="mono-value">${fileInfo.has_results ? "Results available" : "\u2014"}</span>
            </div>
        </div>

        <div class="card">
            <h3>Export</h3>
            <div class="form-row">
                <button id="btn-export-png">Export Current Map as PNG</button>
            </div>
            <div id="export-status" class="mono-value" style="margin-top: 4px;"></div>
        </div>
    `;

    // ── Auto re-analyze on param change ─────────────────────────────
    //
    // The backend's run_analysis just enqueues a request and returns
    // immediately. The actual heavy work happens on a dedicated worker
    // thread, and lifecycle updates arrive as Tauri events:
    //   analysis:started   — worker began processing this request
    //   analysis:complete  — results written, reload them
    //   analysis:failed    — show error, no reload
    //   analysis:cancelled — preempted by a newer request, ignore
    //
    // This keeps the UI responsive during long analyses and lets a
    // newer request preempt an in-flight one (no queue pileup).
    let analyzeTimeout;
    function scheduleReanalyze() {
        clearTimeout(analyzeTimeout);
        analyzeTimeout = setTimeout(triggerReanalyze, 800);
    }

    async function triggerReanalyze() {
        const statusEl = document.getElementById("analysis-status");
        if (statusEl) {
            statusEl.textContent = "Re-analyzing...";
            statusEl.style.color = "";
        }
        try {
            await runAnalysisWithAutoMigrate(invoke, currentFile, statusEl);
            // No await on "complete" — the analysis:complete event handler
            // below reloads results when the worker finishes.
        } catch (e) {
            if (statusEl) {
                statusEl.textContent = `Error: ${errorToString(e)}`;
                statusEl.style.color = "var(--error)";
            }
        }
    }

    // Subscribe to analysis lifecycle events from the worker thread.
    // Tauri's event API delivers these on the UI thread; the handlers
    // never block.
    const { listen } = window.__TAURI__.event;
    const unlistenStarted = await listen("analysis:started", () => {
        const statusEl = document.getElementById("analysis-status");
        if (statusEl) {
            statusEl.textContent = "Analyzing…";
            statusEl.style.color = "";
        }
    });
    const unlistenComplete = await listen("analysis:complete", async (event) => {
        const statusEl = document.getElementById("analysis-status");
        if (statusEl) {
            statusEl.textContent = "Analysis complete";
            statusEl.style.color = "";
        }
        try {
            fileInfo = await invoke("inspect_oisi", { path: currentFile });
            await window.openISI.setAnalysisFile(currentFile);
            const currentMap = viz.mapName;
            if (currentMap && currentMap !== "none") {
                await window.openISI.setMapName(currentMap);
            }
        } catch (e) {
            console.error("Failed to reload after analysis:complete:", e);
        }
    });
    const unlistenFailed = await listen("analysis:failed", (event) => {
        const statusEl = document.getElementById("analysis-status");
        if (statusEl) {
            statusEl.textContent = `Error: ${event.payload?.message ?? "analysis failed"}`;
            statusEl.style.color = "var(--error)";
        }
    });
    const unlistenCancelled = await listen("analysis:cancelled", () => {
        // Preempted by a newer Run — keep status as "Analyzing…" (the
        // next started/complete event will overwrite). No-op intentionally.
    });
    // Tear down listeners when the view re-renders (best-effort —
    // analysis.js re-renders fully on each navigation).
    window.__openisi_analysis_unlisten?.();
    window.__openisi_analysis_unlisten = () => {
        unlistenStarted();
        unlistenComplete();
        unlistenFailed();
        unlistenCancelled();
    };

    // Wire descriptor-driven param inputs: set_params on change, then trigger reanalysis.
    // Param edits (method pickers + tunables) are wired uniformly here via
    // the descriptor-driven path: `wireParamListeners` → `set_params` →
    // reanalysis. There is no separate "method tunable" path — every analysis
    // knob is a registry param rendered by `buildParamGroup`.
    wireParamListeners(container, invoke, () => scheduleReanalyze());

    // Ring controls in analysis view.
    function updateAnalysisRing() {
        const enabled = document.getElementById("analysis-ring-enabled")?.checked;
        const radius = parseInt(document.getElementById("analysis-ring-radius")?.value) || 0;
        const cx = parseInt(document.getElementById("analysis-ring-cx")?.value) || 0;
        const cy = parseInt(document.getElementById("analysis-ring-cy")?.value) || 0;
        const overlay = { enabled, radius_px: radius, center_x_px: cx, center_y_px: cy };
        invoke("set_ring_overlay", { overlay });
        viz.ringVisible = enabled;
        window.openISI.updateRingLayer();
        // Update pix/mm.
        const diam = parseFloat(document.getElementById("analysis-ring-diam")?.value) || 0;
        const ppmEl = document.getElementById("analysis-ring-ppm");
        if (ppmEl && diam > 0 && radius > 0) {
            ppmEl.textContent = `= ${((radius * 2) / diam).toFixed(1)} px/mm`;
        }
    }
    ["analysis-ring-enabled", "analysis-ring-radius", "analysis-ring-cx", "analysis-ring-cy", "analysis-ring-diam"].forEach(id => {
        const el = document.getElementById(id);
        if (el) {
            el.addEventListener("input", updateAnalysisRing);
            el.addEventListener("change", updateAnalysisRing);
        }
    });

    // Export.
    document.getElementById("btn-export-png").addEventListener("click", async () => {
        const mapName = viz.mapName;
        if (!mapName || mapName === "none" || mapName === "patches") {
            document.getElementById("export-status").textContent = mapName === "patches"
                ? "Use a map overlay (Azi/Alt/VFS) for PNG export"
                : "Select a map first";
            return;
        }
        const dialog = window.__TAURI__?.dialog;
        if (!dialog) return;
        const outPath = await dialog.save({
            title: "Export Map as PNG",
            filters: [{ name: "PNG", extensions: ["png"] }],
            defaultPath: `${mapName}.png`,
        });
        if (!outPath) return;
        try {
            await invoke("export_map_png", { path: currentFile, mapName, outputPath: outPath });
            document.getElementById("export-status").textContent = `Exported: ${outPath.split(/[\\/]/).pop()}`;
        } catch (e) {
            document.getElementById("export-status").textContent = `Error: ${errorToString(e)}`;
        }
    });

    // Cleanup: restore camera mode when leaving analysis.
    return function cleanup() {
        exitAnalysisFocusMode();
        window.openISI.setBaseMode("camera");
        window.openISI.setMapName("none");
        window.openISI.viz.bordersVisible = false;
        window.openISI.renderBordersLayer();
        window.openISI.syncLayerBarUI();
    };
}

function enterAnalysisFocusMode() {
    const panel = document.getElementById("preview-panel");
    if (!panel) return;
    const cards = panel.querySelectorAll(".preview-card");

    // Hide stimulus preview.
    if (cards.length >= 2) cards[1].style.display = "none";

    panel.classList.add("focus-mode");

    // Camera is square — fill available panel height.
    const style = getComputedStyle(panel);
    const padV = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
    const labelEl = cards[0]?.querySelector(".preview-label");
    const labelH = labelEl ? labelEl.offsetHeight + 4 : 0;
    const availH = panel.clientHeight - padV - labelH;
    const size = Math.max(100, Math.floor(availH));

    const camContainer = cards[0]?.querySelector(".preview-container");
    if (camContainer) {
        camContainer.style.width = `${size}px`;
        camContainer.style.height = `${size}px`;
    }
    const padH = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    panel.style.width = `${size + padH}px`;
}

function exitAnalysisFocusMode() {
    const panel = document.getElementById("preview-panel");
    if (!panel) return;
    panel.classList.remove("focus-mode");
    const cards = panel.querySelectorAll(".preview-card");
    if (cards.length >= 2) cards[1].style.display = "";
    const camContainer = cards[0]?.querySelector(".preview-container");
    if (camContainer) {
        camContainer.style.width = "";
        camContainer.style.height = "";
    }
    panel.style.width = "";
    if (window.openISI._resizePreview) requestAnimationFrame(window.openISI._resizePreview);
}
