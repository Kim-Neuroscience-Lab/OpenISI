// Analysis view — file inspection, parameter controls, auto re-analyze.
// Matches Juavinett et al. 2017 visualization: area patches (red/blue),
// phase maps (jet, full image), VFS (jet, full image), black borders.

const { invoke } = window.__TAURI__.core;

let currentFile = null;
let fileInfo = null;

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
        container.innerHTML = `<div class="card"><p style="color: var(--error);">Failed to inspect: ${e}</p></div>`;
        return;
    }

    return await renderAnalysisView(container, fileInfo, currentFile);
}

async function renderAnalysisView(container, fileInfo, currentFile) {

    // Load analysis params and ring config.
    const params = await invoke("get_analysis_params");
    const ring = await invoke("get_ring_overlay");
    const seg = params.segmentation || {};

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
                await invoke("run_analysis", { path: currentFile });
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
        </div>

        <div class="card">
            <h3>Retinotopy</h3>
            <div class="form-row">
                <label>Smoothing \u03c3</label>
                <input type="number" id="param-sigma" step="0.5" min="0" max="20" value="${params.smoothing_sigma}" style="width:70px"> px
            </div>
            <div class="form-row">
                <label>Rotation</label>
                <select id="param-rotation">
                    <option value="0" ${params.rotation_k === 0 ? "selected" : ""}>0\u00b0</option>
                    <option value="1" ${params.rotation_k === 1 ? "selected" : ""}>90\u00b0 CCW</option>
                    <option value="2" ${params.rotation_k === 2 ? "selected" : ""}>180\u00b0</option>
                    <option value="3" ${params.rotation_k === 3 ? "selected" : ""}>270\u00b0 CCW</option>
                </select>
            </div>
            <div class="form-row">
                <label>Azi range</label>
                <input type="number" id="param-azi-range" step="5" min="1" max="360" value="${params.azi_angular_range}" style="width:70px"> \u00b0
            </div>
            <div class="form-row">
                <label>Alt range</label>
                <input type="number" id="param-alt-range" step="5" min="1" max="360" value="${params.alt_angular_range}" style="width:70px"> \u00b0
            </div>
            <div class="form-row">
                <label>Azi offset</label>
                <input type="number" id="param-azi-off" step="1" min="-180" max="180" value="${params.offset_azi}" style="width:70px"> \u00b0
            </div>
            <div class="form-row">
                <label>Alt offset</label>
                <input type="number" id="param-alt-off" step="1" min="-90" max="90" value="${params.offset_alt}" style="width:70px"> \u00b0
            </div>
        </div>

        <div class="card">
            <h3>Segmentation</h3>
            <div class="form-row">
                <label>VFS smooth \u03c3</label>
                <input type="number" id="param-vfs-sigma" step="0.5" min="0" max="30" value="${seg.sign_map_filter_sigma}" style="width:70px"> px
            </div>
            <div class="form-row">
                <label>VFS threshold</label>
                <input type="number" id="param-vfs-thresh" step="0.05" min="0" max="1" value="${seg.sign_map_threshold}" style="width:70px">
                <span class="mono-value" style="font-size:10px; color:var(--text-muted)">0 = auto (1.5\u00d7\u03c3)</span>
            </div>
            <div class="form-row">
                <label>Ecc. radius</label>
                <input type="number" id="param-ecc-radius" step="5" min="5" max="90" value="${seg.eccentricity_radius}" style="width:70px"> \u00b0
            </div>
        </div>

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
    let analyzeTimeout;
    function scheduleReanalyze() {
        clearTimeout(analyzeTimeout);
        analyzeTimeout = setTimeout(reanalyze, 800);
    }

    async function reanalyze() {
        const statusEl = document.getElementById("analysis-status");
        if (!statusEl) return;

        // Save params.
        try {
            await invoke("set_analysis_params", {
                smoothingSigma: parseFloat(document.getElementById("param-sigma").value) || 2.0,
                rotationK: parseInt(document.getElementById("param-rotation").value) || 0,
                aziAngularRange: parseFloat(document.getElementById("param-azi-range").value) || 100,
                altAngularRange: parseFloat(document.getElementById("param-alt-range").value) || 100,
                offsetAzi: parseFloat(document.getElementById("param-azi-off").value) || 0,
                offsetAlt: parseFloat(document.getElementById("param-alt-off").value) || 0,
                signMapFilterSigma: parseFloat(document.getElementById("param-vfs-sigma")?.value) || null,
                signMapThreshold: parseFloat(document.getElementById("param-vfs-thresh")?.value) ?? null,
                eccentricityRadius: parseFloat(document.getElementById("param-ecc-radius")?.value) || null,
            });
        } catch (e) {
            statusEl.textContent = `Param save error: ${e}`;
            return;
        }

        statusEl.textContent = "Re-analyzing...";
        statusEl.style.color = "";

        try {
            await invoke("run_analysis", { path: currentFile });
            statusEl.textContent = "Analysis complete";
            fileInfo = await invoke("inspect_oisi", { path: currentFile });

            // Reload all results (re-inspect + rebuild popup + reload borders).
            await window.openISI.setAnalysisFile(currentFile);

            const currentMap = viz.mapName;
            if (currentMap && currentMap !== "none") {
                await window.openISI.setMapName(currentMap);
            }
        } catch (e) {
            statusEl.textContent = `Error: ${e}`;
            statusEl.style.color = "var(--error)";
        }
    }

    // Wire all param inputs to trigger re-analysis.
    const paramIds = ["param-sigma", "param-rotation", "param-azi-range", "param-alt-range",
                      "param-azi-off", "param-alt-off", "param-vfs-sigma", "param-vfs-thresh", "param-ecc-radius"];
    paramIds.forEach(id => {
        const el = document.getElementById(id);
        if (el) el.addEventListener("change", scheduleReanalyze);
    });

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
            document.getElementById("export-status").textContent = `Error: ${e}`;
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
