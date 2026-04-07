// OpenISI — Main entry point
// Icon bar navigation, view management, layered preview panel, status bar.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ═══════════════════════════════════════════════════════════════════════
// View system
// ═══════════════════════════════════════════════════════════════════════

const views = {};
let currentView = null;
let activeCleanup = null;

const viewState = {
    library: true,
    session: false,
    analysis: false,
};

async function loadView(name) {
    if (!views[name]) {
        const module = await import(`./views/${name}.js`);
        views[name] = module;
    }
    return views[name];
}

async function showView(name) {
    if (!viewState[name]) return;
    if (name === currentView) return;

    if (activeCleanup) {
        activeCleanup();
        activeCleanup = null;
    }

    const container = document.getElementById("content");
    const module = await loadView(name);

    document.querySelectorAll("#icon-bar .icon-btn").forEach(btn => {
        btn.classList.toggle("active", btn.dataset.view === name);
    });

    currentView = name;
    const cleanup = await module.render(container);
    if (typeof cleanup === "function") {
        activeCleanup = cleanup;
    }
}

function enableView(name) {
    viewState[name] = true;
    const btn = document.getElementById(`nav-${name}`);
    if (btn) btn.classList.remove("disabled");
}

function disableView(name) {
    viewState[name] = false;
    const btn = document.getElementById(`nav-${name}`);
    if (btn) btn.classList.add("disabled");
}

// ═══════════════════════════════════════════════════════════════════════
// Layered visualization state
// ═══════════════════════════════════════════════════════════════════════

const viz = {
    baseMode: "camera",    // "camera" | "anatomical" | "none"
    mapName: "none",       // "none" | map name | "patches"
    mapOpacity: 0.7,
    blendMode: "source-over",  // CSS mix-blend-mode for map layer
    ringVisible: false,
    bordersVisible: false,

    // Cached data
    anatomicalImageData: null,  // ImageData or null
    mapImageData: null,         // ImageData for current map overlay
    mapRawData: null,           // raw data (f64[], i32[], u8[] depending on type)
    currentResultType: null,    // "scalar_map" | "bool_mask" | "label_map"
    areaSignsCache: null,       // i8[] for label_map rendering
    mapDimensions: null,        // { width, height }
    analysisFile: null,         // path to current .oisi for map loading
    availableResults: new Set(),// set of available result map names
    segData: null,              // {width, height, labels: [i32], area_signs: [i8], borders: [u8]} or null
};

// Restore persisted viz preferences from localStorage.
try {
    const saved = JSON.parse(localStorage.getItem("openisi_viz") || "{}");
    if (saved.baseMode) viz.baseMode = saved.baseMode;
    if (saved.mapName) viz.mapName = saved.mapName;
    if (saved.mapOpacity !== undefined) viz.mapOpacity = saved.mapOpacity;
    if (saved.blendMode) viz.blendMode = saved.blendMode;
    if (saved.ringVisible !== undefined) viz.ringVisible = saved.ringVisible;
    if (saved.bordersVisible !== undefined) viz.bordersVisible = saved.bordersVisible;
    if (saved.analysisFile) viz.analysisFile = saved.analysisFile;
} catch (_e) { alert("Error: " + _e); }

/// Persist viz preferences to localStorage.
function saveVizState() {
    try {
        localStorage.setItem("openisi_viz", JSON.stringify({
            baseMode: viz.baseMode,
            mapName: viz.mapName,
            mapOpacity: viz.mapOpacity,
            blendMode: viz.blendMode,
            ringVisible: viz.ringVisible,
            bordersVisible: viz.bordersVisible,
            analysisFile: viz.analysisFile,
        }));
    } catch (_e) { alert("Error: " + _e); }
}

let stimulusAspectRatio = 16 / 9;

function showPreviewPanel() {
    const panel = document.getElementById("preview-panel");
    if (panel) {
        panel.classList.remove("hidden");
        requestAnimationFrame(resizePreviewPanel);
    }
}

function resizePreviewPanel() {
    const panel = document.getElementById("preview-panel");
    if (!panel || panel.classList.contains("hidden")) return;

    const cards = panel.querySelectorAll(".preview-card");
    if (cards.length < 2) return;

    const style = getComputedStyle(panel);
    const padV = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
    const gap = parseFloat(style.gap) || 8;
    let labelHeight = 0;
    for (const card of cards) {
        const label = card.querySelector(".preview-label");
        if (label) labelHeight += label.offsetHeight + 4;
    }
    const availH = panel.clientHeight - padV - gap - labelHeight;
    if (availH <= 0) return;

    const w = availH / (1 + 1 / stimulusAspectRatio);
    const padH = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    panel.style.width = `${Math.round(w + padH)}px`;

    const camContainer = document.getElementById("cam-container");
    const stimContainer = document.getElementById("stim-container");
    if (camContainer) {
        camContainer.style.aspectRatio = "1";
        camContainer.style.width = `${Math.round(w)}px`;
        camContainer.style.height = `${Math.round(w)}px`;
    }
    if (stimContainer) {
        stimContainer.style.aspectRatio = `${stimulusAspectRatio}`;
        stimContainer.style.width = `${Math.round(w)}px`;
        stimContainer.style.height = `${Math.round(w / stimulusAspectRatio)}px`;
    }
}

window.addEventListener("resize", () => requestAnimationFrame(resizePreviewPanel));

// ═══════════════════════════════════════════════════════════════════════
// Layer rendering
// ═══════════════════════════════════════════════════════════════════════

let cameraConnected = false;

function setupCameraPreview() {
    const canvas = document.getElementById("layer-base");
    const ctx = canvas.getContext("2d");
    const noMsg = document.getElementById("no-camera-msg");
    const panel = document.getElementById("preview-panel");

    listen("camera:frame", (event) => {
        const d = event.payload;
        if (!cameraConnected) return;
        if (viz.baseMode !== "camera") return;

        noMsg.style.display = "none";
        const blob = new Blob([new Uint8Array(d.png_bytes)], { type: "image/png" });
        const url = URL.createObjectURL(blob);
        const img = new Image();
        img.onload = () => {
            canvas.width = img.width;
            canvas.height = img.height;
            ctx.drawImage(img, 0, 0);
            URL.revokeObjectURL(url);
        };
        img.src = url;
    });

    listen("camera:status", (event) => {
        cameraConnected = event.payload.connected;
        if (cameraConnected) {
            panel.classList.remove("hidden");
            noMsg.style.display = "none";
        } else {
            if (viz.baseMode === "camera") {
                noMsg.style.display = "flex";
                noMsg.textContent = "No camera";
            }
        }
    });
}

function setupStimulusPreview() {
    const canvas = document.getElementById("stimulus-canvas");
    const ctx = canvas.getContext("2d");
    const noMsg = document.getElementById("no-stimulus-msg");

    listen("stimulus:preview", (event) => {
        const d = event.payload;
        noMsg.style.display = "none";
        const blob = new Blob([new Uint8Array(d.png_bytes)], { type: "image/png" });
        const url = URL.createObjectURL(blob);
        const img = new Image();
        img.onload = () => {
            canvas.width = img.width;
            canvas.height = img.height;
            ctx.drawImage(img, 0, 0);
            URL.revokeObjectURL(url);
        };
        img.src = url;
    });

    listen("stimulus:stopped", () => {
        noMsg.style.display = "flex";
        noMsg.textContent = "No stimulus";
    });

    listen("stimulus:complete", () => {
        noMsg.style.display = "flex";
        noMsg.textContent = "Complete";
    });
}

/// Render anatomical image data to layer-base.
function renderAnatomicalToBase() {
    if (!viz.anatomicalImageData) return;
    const canvas = document.getElementById("layer-base");
    canvas.width = viz.anatomicalImageData.width;
    canvas.height = viz.anatomicalImageData.height;
    canvas.getContext("2d").putImageData(viz.anatomicalImageData, 0, 0);
    document.getElementById("no-camera-msg").style.display = "none";
}

/// Clear base layer.
function clearBase() {
    const canvas = document.getElementById("layer-base");
    const ctx = canvas.getContext("2d");
    ctx.clearRect(0, 0, canvas.width, canvas.height);
}

/// Render current map to layer-map.
function renderMapLayer() {
    const canvas = document.getElementById("layer-map");
    if (!viz.mapImageData) {
        canvas.getContext("2d").clearRect(0, 0, canvas.width, canvas.height);
        canvas.style.opacity = 0;
        return;
    }
    canvas.width = viz.mapImageData.width;
    canvas.height = viz.mapImageData.height;
    canvas.getContext("2d").putImageData(viz.mapImageData, 0, 0);
    canvas.style.opacity = viz.mapOpacity;
    canvas.style.mixBlendMode = viz.blendMode;
}

/// Update ring layer visibility.
function updateRingLayer() {
    const canvas = document.getElementById("layer-ring");
    if (!canvas) return;
    canvas.style.display = viz.ringVisible ? "block" : "none";
    if (viz.ringVisible) drawGlobalRing();
}

/// Set up global ring interaction — drag to move, scroll to resize.
function setupRingInteraction() {
    const canvas = document.getElementById("layer-ring");
    if (!canvas) return;
    canvas.style.pointerEvents = "auto";

    let dragging = false;

    canvas.addEventListener("mousedown", (e) => {
        if (!viz.ringVisible) return;
        dragging = true;
        updateRingFromMouse(e);
    });

    canvas.addEventListener("mousemove", (e) => {
        if (!dragging) return;
        updateRingFromMouse(e);
    });

    window.addEventListener("mouseup", () => { dragging = false; });

    // Scroll to resize ring. Shift+scroll or right-drag could be used for other layers.
    canvas.addEventListener("wheel", async (e) => {
        if (!viz.ringVisible) return;
        e.preventDefault();
        try {
            const ring = await invoke("get_ring_overlay");
            const delta = e.deltaY > 0 ? -5 : 5;
            ring.radius_px = Math.max(10, Math.min(1000, ring.radius_px + delta));
            await invoke("set_ring_overlay", { overlay: ring });
            drawGlobalRing();
            // Update Focus UI if visible.
            const el = document.getElementById("ring-radius");
            if (el) el.value = ring.radius_px;
            // Update pix/mm if visible.
            const ppmEl = document.getElementById("ring-pix-per-mm");
            const diamEl = document.getElementById("ring-diameter-mm");
            if (ppmEl && diamEl) {
                const diam = parseFloat(diamEl.value) || 0;
                if (diam > 0) ppmEl.textContent = `= ${((ring.radius_px * 2) / diam).toFixed(1)} px/mm`;
            }
        } catch (_) {}
    }, { passive: false });

    async function updateRingFromMouse(e) {
        const camCanvas = document.getElementById("layer-base");
        if (!camCanvas || camCanvas.width === 0 || camCanvas.height === 0) return;
        const rect = canvas.getBoundingClientRect();
        const displayX = e.clientX - rect.left;
        const displayY = e.clientY - rect.top;
        const scaleX = camCanvas.width / canvas.width;
        const scaleY = camCanvas.height / canvas.height;
        const cx = Math.round(displayX * scaleX);
        const cy = Math.round(displayY * scaleY);
        try {
            const ring = await invoke("get_ring_overlay");
            ring.center_x_px = cx;
            ring.center_y_px = cy;
            await invoke("set_ring_overlay", { overlay: ring });
            drawGlobalRing();
            // Update Focus UI if visible.
            const cxEl = document.getElementById("ring-cx");
            const cyEl = document.getElementById("ring-cy");
            if (cxEl) cxEl.value = cx;
            if (cyEl) cyEl.value = cy;
        } catch (_) {}
    }
}

/// Draw the ring overlay from saved config. Called globally — not tied to Focus section.
async function drawGlobalRing() {
    const canvas = document.getElementById("layer-ring");
    const camCanvas = document.getElementById("layer-base");
    if (!canvas || !camCanvas) return;

    let ring;
    try {
        ring = await invoke("get_ring_overlay");
    } catch (_) { return; }

    const container = canvas.parentElement;
    canvas.width = container.clientWidth;
    canvas.height = container.clientHeight;
    const ctx = canvas.getContext("2d");
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    if (!ring.enabled) return;
    if (camCanvas.width === 0 || camCanvas.height === 0) return;

    const scaleX = canvas.width / camCanvas.width;
    const scaleY = canvas.height / camCanvas.height;

    // Ring circle.
    ctx.beginPath();
    ctx.arc(ring.center_x_px * scaleX, ring.center_y_px * scaleY, ring.radius_px * scaleX, 0, Math.PI * 2);
    ctx.strokeStyle = "rgba(0, 255, 0, 0.7)";
    ctx.lineWidth = 2;
    ctx.stroke();

    // Crosshair.
    const cxd = ring.center_x_px * scaleX;
    const cyd = ring.center_y_px * scaleY;
    ctx.beginPath();
    ctx.moveTo(cxd - 8, cyd);
    ctx.lineTo(cxd + 8, cyd);
    ctx.moveTo(cxd, cyd - 8);
    ctx.lineTo(cxd, cyd + 8);
    ctx.strokeStyle = "rgba(0, 255, 0, 0.5)";
    ctx.lineWidth = 1;
    ctx.stroke();
}

/// Render area borders from border mask data (black 1px lines).
function renderBordersLayer() {
    const canvas = document.getElementById("layer-borders");
    if (!canvas) return;
    const ctx = canvas.getContext("2d");

    if (!viz.bordersVisible || !viz.segData || !viz.segData.borders || !viz.segData.width) {
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        return;
    }

    const width = viz.segData.width;
    const height = viz.segData.height;
    canvas.width = width;
    canvas.height = height;
    const imgData = ctx.createImageData(width, height);
    const borders = viz.segData.borders;

    for (let i = 0; i < borders.length; i++) {
        if (borders[i] > 0) {
            imgData.data[i * 4] = 0;
            imgData.data[i * 4 + 1] = 0;
            imgData.data[i * 4 + 2] = 0;
            imgData.data[i * 4 + 3] = 255;
        }
    }
    ctx.putImageData(imgData, 0, 0);
}

/// Apply base mode change.
function setBaseMode(mode) {
    viz.baseMode = mode;
    saveVizState();
    updatePopupGroup("popup-base", mode);
    document.getElementById("lbtn-base")?.classList.toggle("active", mode !== "camera");
    const noMsg = document.getElementById("no-camera-msg");

    if (mode === "camera") {
        if (!cameraConnected) {
            noMsg.style.display = "flex";
            noMsg.textContent = "No camera";
        } else {
            noMsg.style.display = "none";
        }
    } else if (mode === "anatomical") {
        noMsg.style.display = "none";
        renderAnatomicalToBase();
        if (!viz.anatomicalImageData) {
            noMsg.style.display = "flex";
            noMsg.textContent = "No anatomical";
        }
    } else {
        clearBase();
        noMsg.style.display = "none";
    }
}

/// Build ImageData from loaded result data, rendered by type.
function buildMapImageData() {
    const { width, height } = viz.mapDimensions || {};
    if (!width || !height) return null;
    const data = viz.mapRawData;
    const type = viz.currentResultType;

    if (!data || !type) return null;
    const imgData = new ImageData(width, height);

    if (type === "label_map") {
        // Area labels → red (positive VFS) / blue (negative VFS) patches.
        // Need area_signs — load separately if not cached.
        const signs = viz.areaSignsCache || [];
        for (let i = 0; i < data.length; i++) {
            const label = data[i];
            if (label > 0 && label <= signs.length) {
                if (signs[label - 1] > 0) {
                    imgData.data[i * 4] = 255; imgData.data[i * 4 + 1] = 50; imgData.data[i * 4 + 2] = 50;
                } else {
                    imgData.data[i * 4] = 50; imgData.data[i * 4 + 1] = 50; imgData.data[i * 4 + 2] = 255;
                }
                imgData.data[i * 4 + 3] = 255;
            }
            // else: transparent (alpha stays 0)
        }
        return imgData;
    }

    if (type === "bool_mask") {
        // Boolean mask → black pixels where true, transparent elsewhere.
        for (let i = 0; i < data.length; i++) {
            if (data[i] > 0) {
                imgData.data[i * 4] = 0;
                imgData.data[i * 4 + 1] = 0;
                imgData.data[i * 4 + 2] = 0;
                imgData.data[i * 4 + 3] = 255;
            }
        }
        return imgData;
    }

    // scalar_map → jet colormap.
    // Maps with "masked" zero regions (vfs_thresholded, eccentricity, magnification)
    // have 0.0 for unassigned pixels → render as transparent.
    const maskedMaps = new Set(["vfs_thresholded", "eccentricity", "magnification"]);
    const isMasked = maskedMaps.has(viz.mapName);

    let min = Infinity, max = -Infinity;
    for (const v of data) {
        if (isFinite(v) && (!isMasked || v !== 0.0)) {
            if (v < min) min = v;
            if (v > max) max = v;
        }
    }
    if (!isFinite(min)) { min = 0; max = 1; }
    const range = max - min || 1;

    for (let i = 0; i < data.length; i++) {
        if (isMasked && data[i] === 0.0) {
            imgData.data[i * 4 + 3] = 0; // transparent
            continue;
        }
        const t = (data[i] - min) / range;
        const [r, g, b] = jetColormap(Math.max(0, Math.min(1, t)));
        imgData.data[i * 4] = r;
        imgData.data[i * 4 + 1] = g;
        imgData.data[i * 4 + 2] = b;
        imgData.data[i * 4 + 3] = 255;
    }
    return imgData;
}

/// Load any result by name from the current analysis file. Unified command.
async function setMapName(mapName) {
    viz.mapName = mapName;
    saveVizState();
    updatePopupGroup("popup-map", mapName);
    document.getElementById("lbtn-map")?.classList.toggle("active", mapName !== "none");

    if (mapName === "none" || !viz.analysisFile) {
        viz.mapImageData = null;
        viz.mapRawData = null;
        viz.currentResultType = null;
        renderMapLayer();
        return;
    }

    try {
        const result = await invoke("read_result", { path: viz.analysisFile, name: mapName });
        viz.currentResultType = result.type;
        viz.mapRawData = result.data;
        if (result.width && result.height) {
            viz.mapDimensions = { width: result.width, height: result.height };
        }

        // Cache area_signs if we loaded label_map (needed for coloring).
        if (result.type === "label_map" && !viz.areaSignsCache) {
            try {
                const signs = await invoke("read_result", { path: viz.analysisFile, name: "area_signs" });
                viz.areaSignsCache = signs.data;
            } catch (_) {
                viz.areaSignsCache = [];
            }
        }

        viz.mapImageData = buildMapImageData();
        renderMapLayer();
    } catch (e) {
        console.error("Failed to load result:", mapName, e);
        viz.mapImageData = null;
        viz.mapRawData = null;
        viz.currentResultType = null;
        renderMapLayer();
    }
}

/// Load anatomical from an .oisi file into the viz cache.
async function loadAnatomical(filePath) {
    try {
        const anat = await invoke("read_anatomical", { path: filePath });
        const imgData = new ImageData(anat.width, anat.height);
        for (let i = 0; i < anat.data.length; i++) {
            const v = anat.data[i];
            imgData.data[i * 4] = v;
            imgData.data[i * 4 + 1] = v;
            imgData.data[i * 4 + 2] = v;
            imgData.data[i * 4 + 3] = 255;
        }
        viz.anatomicalImageData = imgData;
        if (viz.baseMode === "anatomical") renderAnatomicalToBase();
    } catch (_) {
        viz.anatomicalImageData = null;
    }
}

/// Set the analysis file — loads anatomical + enables map buttons.
async function setAnalysisFile(filePath) {
    viz.analysisFile = filePath;
    if (!filePath) {
        viz.anatomicalImageData = null;
        viz.mapImageData = null;
        viz.segData = null;
        viz.availableResults = new Set();
        renderMapLayer();
        renderBordersLayer();
        return;
    }

    // Inspect file to discover typed results.
    let results = [];
    try {
        const info = await invoke("inspect_oisi", { path: filePath });
        results = info.results || [];
    } catch (e) {
        throw new Error("inspect_oisi failed: " + e);
    }
    viz.availableResults = new Set(results.map(r => r.name));
    viz.resultTypes = {};
    for (const r of results) {
        viz.resultTypes[r.name] = r.result_type;
    }

    // Build map popup dynamically from results.
    buildMapPopup(results);

    // Load area_signs for label_map rendering.
    viz.areaSignsCache = null;
    if (viz.availableResults.has("area_signs")) {
        try {
            const signs = await invoke("read_result", { path: filePath, name: "area_signs" });
            viz.areaSignsCache = signs.data;
        } catch (_e) { alert("Error: " + _e); }
    }

    // Load borders for the borders layer.
    viz.segData = null;
    if (viz.availableResults.has("area_borders")) {
        try {
            const borders = await invoke("read_result", { path: filePath, name: "area_borders" });
            viz.segData = { width: borders.width, height: borders.height, borders: borders.data };
        } catch (_e) { alert("Error: " + _e); }
    }

    await loadAnatomical(filePath);
    renderBordersLayer();
}

/// Build map popup buttons dynamically from available results.
function buildMapPopup(results) {
    const popup = document.getElementById("popup-map");
    if (!popup || !results) return;

    // Keep the title and controls (opacity, blend).
    const titleEl = popup.querySelector(".popup-title");
    const controlEls = [];
    let collecting = false;
    for (const child of [...popup.children]) {
        if (child.classList?.contains("popup-title") && child.textContent.includes("Opacity")) {
            collecting = true;
        }
        if (collecting) controlEls.push(child);
    }

    // Clear and rebuild.
    popup.innerHTML = "";
    const title = document.createElement("div");
    title.className = "popup-title";
    title.textContent = "Map Overlay";
    popup.appendChild(title);

    // "None" button.
    const noneBtn = document.createElement("button");
    noneBtn.className = "popup-opt active";
    noneBtn.dataset.val = "none";
    noneBtn.textContent = "None";
    noneBtn.addEventListener("click", () => {
        setMapName("none");
        popup.querySelectorAll(".popup-opt").forEach(b => b.classList.toggle("active", b.dataset.val === "none"));
        document.getElementById("lbtn-map")?.classList.remove("active");
    });
    popup.appendChild(noneBtn);

    // Human-readable names.
    const displayNames = {
        azi_phase_degrees: "Azi Phase",
        alt_phase_degrees: "Alt Phase",
        azi_phase: "Azi Phase (rad)",
        alt_phase: "Alt Phase (rad)",
        azi_amplitude: "Azi Amplitude",
        alt_amplitude: "Alt Amplitude",
        vfs: "VFS",
        vfs_thresholded: "VFS (thresholded)",
        area_labels: "Area Patches",
        area_borders: "Area Borders",
        eccentricity: "Eccentricity",
        magnification: "Magnification",
        contours_azi: "Contours Azi",
        contours_alt: "Contours Alt",
        snr_azi: "SNR Azi",
        snr_alt: "SNR Alt",
    };

    // Add a button for each displayable result.
    for (const r of results) {
        if (r.result_type === "sign_array") continue; // metadata, not displayable
        const btn = document.createElement("button");
        btn.className = "popup-opt";
        btn.dataset.val = r.name;
        btn.dataset.type = r.result_type;
        btn.textContent = displayNames[r.name] || r.name;
        btn.addEventListener("click", () => {
            setMapName(r.name);
            popup.querySelectorAll(".popup-opt").forEach(b => b.classList.toggle("active", b.dataset.val === r.name));
            document.getElementById("lbtn-map")?.classList.toggle("active", true);
        });
        popup.appendChild(btn);
    }

    // Re-append the control elements (opacity, blend).
    for (const el of controlEls) popup.appendChild(el);
}


// ═══════════════════════════════════════════════════════════════════════
// Control wiring
// ═══════════════════════════════════════════════════════════════════════

function updatePopupGroup(popupId, activeVal) {
    document.querySelectorAll(`#${popupId} .popup-opt`).forEach(btn => {
        btn.classList.toggle("active", btn.dataset.val === activeVal);
    });
}

/// Sync all layer bar popup highlights and icon states from current viz state.
function syncLayerBarUI() {
    updatePopupGroup("popup-base", viz.baseMode);
    document.getElementById("lbtn-base")?.classList.toggle("active", viz.baseMode !== "camera");
    updatePopupGroup("popup-map", viz.mapName);
    document.getElementById("lbtn-map")?.classList.toggle("active", viz.mapName !== "none");
    updatePopupGroup("popup-borders", viz.bordersVisible ? "show" : "hide");
    document.getElementById("lbtn-borders")?.classList.toggle("active", viz.bordersVisible);
    updatePopupGroup("popup-ring", viz.ringVisible ? "show" : "hide");
    document.getElementById("lbtn-ring")?.classList.toggle("active", viz.ringVisible);
    const slider = document.getElementById("ctrl-map-opacity");
    const label = document.getElementById("ctrl-map-opacity-val");
    if (slider) { slider.value = Math.round(viz.mapOpacity * 100); }
    if (label) { label.textContent = `${Math.round(viz.mapOpacity * 100)}%`; }
    const blendSelect = document.getElementById("ctrl-blend-mode");
    if (blendSelect) { blendSelect.value = viz.blendMode; }
}

function setupLayerBar() {
    // Toggle popup on icon click.
    document.querySelectorAll(".layer-icon").forEach(icon => {
        icon.addEventListener("click", (e) => {
            e.stopPropagation();
            const wrap = icon.closest(".layer-icon-wrap");
            const popup = wrap.querySelector(".layer-popup");
            const wasOpen = popup.classList.contains("open");
            // Close all popups first.
            document.querySelectorAll(".layer-popup").forEach(p => p.classList.remove("open"));
            if (!wasOpen) popup.classList.add("open");
        });
    });

    // Close popups on outside click.
    document.addEventListener("click", () => {
        document.querySelectorAll(".layer-popup").forEach(p => p.classList.remove("open"));
    });
    // Prevent popup clicks from closing themselves.
    document.querySelectorAll(".layer-popup").forEach(p => {
        p.addEventListener("click", (e) => e.stopPropagation());
    });

    // Base layer options.
    document.querySelectorAll("#popup-base .popup-opt").forEach(btn => {
        btn.addEventListener("click", () => {
            setBaseMode(btn.dataset.val);
            updatePopupGroup("popup-base", btn.dataset.val);
            // Highlight icon when not "camera" (non-default).
            document.getElementById("lbtn-base").classList.toggle("active", btn.dataset.val !== "camera");
        });
    });

    // Map layer options wired dynamically by buildMapPopup() — not here.

    // Map opacity slider.
    const slider = document.getElementById("ctrl-map-opacity");
    const valLabel = document.getElementById("ctrl-map-opacity-val");
    if (slider) {
        slider.addEventListener("input", () => {
            viz.mapOpacity = parseFloat(slider.value) / 100;
            valLabel.textContent = `${slider.value}%`;
            const mapCanvas = document.getElementById("layer-map");
            if (mapCanvas) mapCanvas.style.opacity = viz.mapOpacity;
            saveVizState();
        });
    }

    // Blend mode select.
    const blendSelect = document.getElementById("ctrl-blend-mode");
    if (blendSelect) {
        blendSelect.addEventListener("change", () => {
            viz.blendMode = blendSelect.value;
            const mapCanvas = document.getElementById("layer-map");
            if (mapCanvas) mapCanvas.style.mixBlendMode = viz.blendMode;
            saveVizState();
        });
    }

    // Borders toggle.
    document.querySelectorAll("#popup-borders .popup-opt").forEach(btn => {
        btn.addEventListener("click", () => {
            viz.bordersVisible = btn.dataset.val === "show";
            updatePopupGroup("popup-borders", btn.dataset.val);
            renderBordersLayer();
            document.getElementById("lbtn-borders").classList.toggle("active", viz.bordersVisible);
            saveVizState();
        });
    });

    // Ring toggle.
    document.querySelectorAll("#popup-ring .popup-opt").forEach(btn => {
        btn.addEventListener("click", () => {
            viz.ringVisible = btn.dataset.val === "show";
            updatePopupGroup("popup-ring", btn.dataset.val);
            updateRingLayer();
            document.getElementById("lbtn-ring").classList.toggle("active", viz.ringVisible);
            saveVizState();
        });
    });
}

// ═══════════════════════════════════════════════════════════════════════
// Jet colormap
// ═══════════════════════════════════════════════════════════════════════

function jetColormap(t) {
    t = Math.max(0, Math.min(1, t));
    let r, g, b;
    if (t < 0.125) {
        r = 0; g = 0; b = 128 + t / 0.125 * 127;
    } else if (t < 0.375) {
        r = 0; g = (t - 0.125) / 0.25 * 255; b = 255;
    } else if (t < 0.625) {
        r = (t - 0.375) / 0.25 * 255; g = 255; b = 255 - (t - 0.375) / 0.25 * 255;
    } else if (t < 0.875) {
        r = 255; g = 255 - (t - 0.625) / 0.25 * 255; b = 0;
    } else {
        r = 255 - (t - 0.875) / 0.125 * 127; g = 0; b = 0;
    }
    return [Math.round(r), Math.round(g), Math.round(b)];
}

// ═══════════════════════════════════════════════════════════════════════
// Status bar
// ═══════════════════════════════════════════════════════════════════════

async function updateStatusBar() {
    try {
        const status = await invoke("get_workspace_status");
        document.getElementById("status-display").textContent = `Display: ${status.display}`;
        document.getElementById("status-camera").textContent = `Camera: ${status.camera}`;
        document.getElementById("status-activity").textContent = status.activity;
        document.getElementById("status-activity").className = status.activity === "Acquiring" ? "status-ok" : "";
    } catch (e) {}
}

// ═══════════════════════════════════════════════════════════════════════
// Global event listeners
// ═══════════════════════════════════════════════════════════════════════

async function setupGlobalListeners() {
    await listen("camera:status", (event) => {
        const d = event.payload;
        const el = document.getElementById("status-camera");
        if (d.connected) {
            el.textContent = `Camera: ${d.model} ${d.width_px}\u00d7${d.height_px}`;
            el.className = "status-ok";
        } else {
            el.textContent = "Camera: \u2014";
            el.className = "";
        }
    });

    await listen("stimulus:frame", () => {
        document.getElementById("status-activity").textContent = "Acquiring";
        document.getElementById("status-activity").className = "status-ok";
    });

    await listen("stimulus:complete", () => {
        document.getElementById("status-activity").textContent = "Complete";
        document.getElementById("status-activity").className = "status-ok";
    });

    await listen("stimulus:stopped", () => {
        document.getElementById("status-activity").textContent = "Idle";
        document.getElementById("status-activity").className = "";
    });

    await listen("error", (event) => {
        const d = event.payload;
        const el = document.getElementById("status-activity");
        el.textContent = `Error: ${d.source}`;
        el.className = "status-error";
        setTimeout(() => { el.textContent = "Idle"; el.className = ""; }, 5000);
    });
}

// ═══════════════════════════════════════════════════════════════════════
// Auto-setup: detect monitor, validate, scan camera
// ═══════════════════════════════════════════════════════════════════════

async function autoSetup() {
    try {
        const monitors = await invoke("get_monitors");
        const stimIndex = monitors.length > 1 ? 1 : 0;
        const selected = await invoke("select_display", { monitorIndex: stimIndex });
        console.log(`[auto] Display: ${selected.name} ${selected.width_px}\u00d7${selected.height_px}`);
        stimulusAspectRatio = selected.width_px / selected.height_px;
        resizePreviewPanel();
        await updateStatusBar();

        invoke("validate_display").then(v => {
            console.log(`[auto] Validated: ${v.measured_refresh_hz.toFixed(2)} Hz`);
            updateStatusBar();
        }).catch(e => console.warn("[auto] Validation failed:", e));
    } catch (e) {
        console.warn("[auto] Display setup failed:", e);
    }

    try {
        const unlisten = await listen("camera:enumerated", async (event) => {
            unlisten();
            const devices = event.payload;
            if (devices.length > 0) {
                try {
                    await invoke("connect_camera", { cameraIndex: devices[0].index });
                    console.log(`[auto] Camera: ${devices[0].name}`);
                    document.getElementById("preview-panel").classList.remove("hidden");
                } catch (e) {
                    console.warn("[auto] Camera connect failed:", e);
                }
            }
        });
        await invoke("enumerate_cameras");
    } catch (e) {
        console.warn("[auto] Camera scan failed:", e);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Expose API for views
// ═══════════════════════════════════════════════════════════════════════

window.openISI = {
    showView,
    enableView,
    disableView,
    invoke,
    listen,
    showPreviewPanel,
    viz,
    setBaseMode,
    setMapName,
    setAnalysisFile,
    loadAnatomical,
    renderMapLayer,
    updateRingLayer,
    renderBordersLayer,
    drawGlobalRing,
    syncLayerBarUI,
    _resizePreview: resizePreviewPanel,
    _analysisFile: viz.analysisFile, // restored from localStorage
};

// ═══════════════════════════════════════════════════════════════════════
// Icon bar click handlers
// ═══════════════════════════════════════════════════════════════════════

document.querySelectorAll("#icon-bar .icon-btn").forEach(btn => {
    btn.addEventListener("click", () => {
        const view = btn.dataset.view;
        if (view) showView(view);
    });
});

// ═══════════════════════════════════════════════════════════════════════
// Init
// ═══════════════════════════════════════════════════════════════════════

document.addEventListener("DOMContentLoaded", async () => {
    setupCameraPreview();
    setupStimulusPreview();
    setupLayerBar();
    setupRingInteraction();
    updateRingLayer();
    await setupGlobalListeners();
    await updateStatusBar();
    setInterval(updateStatusBar, 2000);

    await showView("library");
    autoSetup();
});
