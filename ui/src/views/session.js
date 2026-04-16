// Session view — workflow rail with collapsible sections.
// Setup → Focus → Protocol → Acquire

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
import { buildParamInput, buildParamGroup, wireParamListeners, applyParamChanges, fetchGroupDescriptors } from '../param-form.js';

// ═══════════════════════════════════════════════════════════════════════
// Section definitions
// ═══════════════════════════════════════════════════════════════════════

const sections = [
    { id: "setup",    title: "Setup",    status: "Not started" },
    { id: "focus",    title: "Focus",    status: "—" },
    { id: "protocol", title: "Protocol", status: "—" },
    { id: "acquire",  title: "Acquire",  status: "Ready" },
];

let expandedSection = null;
let unlisteners = [];

// ═══════════════════════════════════════════════════════════════════════
// Render
// ═══════════════════════════════════════════════════════════════════════

export async function render(container) {
    container.innerHTML = sections.map(s => `
        <div class="section-header" data-section="${s.id}" id="header-${s.id}">
            <span class="chevron">▶</span>
            <span class="title">${s.title}</span>
            <span class="status" id="status-${s.id}">${s.status}</span>
        </div>
        <div class="section-body" id="body-${s.id}"></div>
    `).join("");

    // Click handlers for section headers.
    document.querySelectorAll(".section-header").forEach(header => {
        header.addEventListener("click", () => toggleSection(header.dataset.section));
    });

    // Expand Setup by default.
    await toggleSection("setup");

    // Update statuses from current state.
    await refreshStatuses();

    return cleanup;
}

function cleanup() {
    for (const fn of unlisteners) fn();
    unlisteners = [];
}

// ═══════════════════════════════════════════════════════════════════════
// Section toggle (accordion)
// ═══════════════════════════════════════════════════════════════════════

async function toggleSection(id) {
    const wasExpanded = expandedSection === id;

    // Collapse current.
    if (expandedSection) {
        const header = document.getElementById(`header-${expandedSection}`);
        const body = document.getElementById(`body-${expandedSection}`);
        if (header) header.classList.remove("expanded");
        if (body) body.classList.remove("expanded");
    }

    // Exit focus mode if leaving Focus section.
    if (expandedSection === "focus") {
        exitFocusMode();
    }

    expandedSection = null;

    // Expand new (unless we're toggling off).
    if (!wasExpanded) {
        const header = document.getElementById(`header-${id}`);
        const body = document.getElementById(`body-${id}`);
        if (header) header.classList.add("expanded");
        if (body) {
            body.classList.add("expanded");
            await renderSectionContent(id, body);
        }
        expandedSection = id;

        // Enter focus mode — camera preview fills full panel height.
        if (id === "focus") {
            enterFocusMode();
        }
    }
}

function enterFocusMode() {
    const panel = document.getElementById("preview-panel");
    if (!panel) return;
    const cards = panel.querySelectorAll(".preview-card");

    // Hide stimulus preview.
    if (cards.length >= 2) {
        cards[1].style.display = "none";
    }

    panel.classList.add("focus-mode");

    // Compute: camera is square, fill available panel height.
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
    // Panel width = square size + padding.
    const padH = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    panel.style.width = `${size + padH}px`;
}

function exitFocusMode() {
    const panel = document.getElementById("preview-panel");
    if (!panel) return;
    panel.classList.remove("focus-mode");
    const cards = panel.querySelectorAll(".preview-card");
    if (cards.length >= 2) {
        cards[1].style.display = "";
    }
    // Reset inline styles and trigger normal resize.
    const camContainer = cards[0]?.querySelector(".preview-container");
    if (camContainer) {
        camContainer.style.width = "";
        camContainer.style.height = "";
    }
    panel.style.width = "";
    if (typeof window.openISI._resizePreview === "function") {
        requestAnimationFrame(window.openISI._resizePreview);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Section content rendering
// ═══════════════════════════════════════════════════════════════════════

async function renderSectionContent(id, body) {
    switch (id) {
        case "setup":    return renderSetup(body);
        case "focus":    return renderFocus(body);
        case "protocol": return renderProtocol(body);
        case "acquire":  return renderAcquire(body);
        // Analysis is in its own dedicated view, not a session section.
    }
}

async function renderSetup(body) {
    const monitors = await invoke("get_monitors");
    const session = await invoke("get_session");

    // Load rig geometry.
    let geometry = { viewing_distance_cm: 0, horizontal_offset_deg: 0, vertical_offset_deg: 0, projection: "cartesian" };
    try {
        // Read from rig config via a session property or direct invoke.
        // For now, use the session data which includes geometry from rig.
    } catch (e) {}

    const disp = session.selected_display;

    body.innerHTML = `
        <div style="display:flex; gap:12px;">
            <div class="card" style="flex:1;">
                <h3>Display</h3>
                <div class="form-row">
                    <label>Monitor</label>
                    <select id="monitor-select" style="flex:1">
                        ${monitors.map((m, i) => `
                            <option value="${i}" ${disp?.index === i ? "selected" : ""}>
                                ${m.name} \u2014 ${m.width_px}\u00d7${m.height_px} @${m.refresh_hz}Hz
                            </option>
                        `).join("")}
                    </select>
                    <button id="btn-select-display">Select</button>
                </div>
                <div class="form-row">
                    <label>Size (cm)</label>
                    <input type="number" id="disp-width-cm" step="0.1" min="1" max="300" value="${disp?.width_cm?.toFixed(1) ?? ""}" style="width:65px">
                    <span>\u00d7</span>
                    <input type="number" id="disp-height-cm" step="0.1" min="1" max="200" value="${disp?.height_cm?.toFixed(1) ?? ""}" style="width:65px">
                    <span class="mono-value" style="font-size:10px">${disp?.physical_source ?? ""}</span>
                </div>
                <div class="form-row">
                    <label>Viewing dist.</label>
                    <input type="number" id="geom-vdist" step="0.5" min="1" max="200" style="width:65px"> cm
                    <label style="min-width:auto; margin-left:8px">Mount rotation</label>
                    <select id="monitor-rotation" style="width:60px">
                        <option value="0" ${session.monitor_rotation_deg === 0 ? "selected" : ""}>0\u00b0</option>
                        <option value="90" ${session.monitor_rotation_deg === 90 ? "selected" : ""}>90\u00b0</option>
                        <option value="180" ${session.monitor_rotation_deg === 180 ? "selected" : ""}>180\u00b0</option>
                        <option value="270" ${session.monitor_rotation_deg === 270 ? "selected" : ""}>270\u00b0</option>
                    </select>
                </div>
                <div class="form-row">
                    <button id="btn-validate">Validate</button>
                    <span id="validation-result" class="mono-value" style="font-size:11px">
                        ${session.display_validation
                            ? `${session.display_validation.matches_reported ? "\u2713" : "\u26a0"} ${session.display_validation.measured_refresh_hz.toFixed(2)} Hz, ${session.display_validation.jitter_us.toFixed(1)} \u00b5s jitter`
                            : "Not validated"}
                    </span>
                </div>
            </div>
            <div class="card" style="flex:1;">
                <h3>Camera</h3>
                <div class="form-row">
                    <button id="btn-scan-cameras">Scan</button>
                    <span id="camera-status" class="mono-value">
                        ${session.camera_connected
                            ? `Connected: ${session.camera?.model} ${session.camera?.width_px}\u00d7${session.camera?.height_px}`
                            : "Not connected"}
                    </span>
                </div>
            </div>
        </div>
    `;

    document.getElementById("btn-select-display").addEventListener("click", async () => {
        const idx = parseInt(document.getElementById("monitor-select").value);
        try {
            const result = await invoke("select_display", { monitorIndex: idx });
            document.getElementById("display-dims").textContent =
                `${result.width_cm.toFixed(1)} × ${result.height_cm.toFixed(1)} cm`;
            await refreshStatuses();
        } catch (e) {
            console.error("select_display:", e);
        }
    });

    document.getElementById("btn-validate").addEventListener("click", async () => {
        const el = document.getElementById("validation-result");
        el.textContent = "Validating...";
        try {
            const v = await invoke("validate_display");
            el.textContent = `${v.measured_refresh_hz.toFixed(2)} Hz, ${v.jitter_us.toFixed(1)} µs jitter`;
            await refreshStatuses();
        } catch (e) {
            el.textContent = `Error: ${e}`;
        }
    });

    document.getElementById("btn-scan-cameras").addEventListener("click", async () => {
        const statusEl = document.getElementById("camera-status");

        // If already connected, disconnect first before rescanning.
        const session = await invoke("get_session");
        if (session.camera_connected) {
            statusEl.textContent = "Disconnecting...";
            try { await invoke("disconnect_camera"); } catch (_) {}
            await new Promise(r => setTimeout(r, 500));
        }

        statusEl.textContent = "Scanning...";

        const unlisten = await listen("camera:enumerated", async (event) => {
            unlisten();
            const devices = event.payload;
            if (devices.length > 0) {
                statusEl.textContent = `Found ${devices.length}, connecting...`;
                try {
                    await invoke("connect_camera", { cameraIndex: devices[0].index });
                    statusEl.textContent = `Connected: ${devices[0].name}`;
                    document.getElementById("preview-panel").classList.remove("hidden");
                    await refreshStatuses();
                } catch (e) {
                    statusEl.textContent = `Connect failed: ${e}`;
                }
            } else {
                statusEl.textContent = "No cameras found";
            }
        });
        unlisteners.push(unlisten);

        try {
            await invoke("enumerate_cameras");
        } catch (e) {
            statusEl.textContent = `Scan failed: ${e}`;
        }
    });

    // Listen for camera connect/disconnect events (e.g., from auto-setup).
    const unlistenCam = await listen("camera:status", (event) => {
        const statusEl = document.getElementById("camera-status");
        if (!statusEl) return;
        if (event.payload.connected) {
            statusEl.textContent = `Connected: ${event.payload.model} ${event.payload.width_px}\u00d7${event.payload.height_px}`;
        } else {
            statusEl.textContent = "Disconnected";
        }
    });
    unlisteners.push(unlistenCam);

    // Rotation.
    document.getElementById("monitor-rotation").addEventListener("change", (e) => {
        invoke("set_monitor_rotation", { rotationDeg: parseFloat(e.target.value) });
    });

    // Physical dimension override.
    function saveDimensions() {
        const w = parseFloat(document.getElementById("disp-width-cm").value);
        const h = parseFloat(document.getElementById("disp-height-cm").value);
        if (w > 0 && h > 0) {
            invoke("set_display_dimensions", { widthCm: w, heightCm: h });
            document.getElementById("disp-source").textContent = "user_override";
        }
    }
    document.getElementById("disp-width-cm").addEventListener("change", saveDimensions);
    document.getElementById("disp-height-cm").addEventListener("change", saveDimensions);

    // Viewing distance — read from rig config and persist on change.
    try {
        const geom = await invoke("get_rig_geometry");
        document.getElementById("geom-vdist").value = geom.viewing_distance_cm;
    } catch (e) {}

    document.getElementById("geom-vdist").addEventListener("change", (e) => {
        const val = parseFloat(e.target.value);
        if (val > 0) {
            invoke("set_viewing_distance", { distanceCm: val });
        }
    });
}

async function renderFocus(body) {
    // Load ring overlay config from rig.
    const ring = await invoke("get_ring_overlay");

    body.innerHTML = `
        <div class="card">
            <h3>Exposure</h3>
            <div class="form-row">
                <label>Exposure (µs)</label>
                <button id="exp-minus" style="width:32px">−</button>
                <input type="range" id="exposure-slider" min="1000" max="200000" step="1000" style="flex:1">
                <button id="exp-plus" style="width:32px">+</button>
                <input type="number" id="exposure-value" style="width:80px">
            </div>
        </div>
        <div class="card">
            <h3>Ring Overlay</h3>
            <div class="form-row">
                <label>Show ring</label>
                <input type="checkbox" id="ring-enabled" ${ring.enabled ? "checked" : ""}>
            </div>
            <div class="form-row">
                <label>Radius (px)</label>
                <input type="number" id="ring-radius" min="10" max="500" step="5" value="${ring.radius_px}" style="width:80px">
            </div>
            <div class="form-row">
                <label>Center X</label>
                <input type="number" id="ring-cx" value="${ring.center_x_px}" style="width:80px">
                <label style="min-width:auto">Y</label>
                <input type="number" id="ring-cy" value="${ring.center_y_px}" style="width:80px">
            </div>
            <div class="form-row">
                <label>Ring diameter (mm)</label>
                <input type="number" id="ring-diameter-mm" min="0.1" max="50" step="0.1" value="5.0" style="width:80px">
                <span id="ring-pix-per-mm" class="mono-value" style="font-size:11px"></span>
            </div>
        </div>
        <div class="card">
            <h3>Anatomical</h3>
            <button id="btn-capture-anat" class="primary">Capture Anatomical</button>
            <span id="anat-status" style="margin-left: 8px; font-size: 12px; color: var(--text-secondary);">Not captured</span>
        </div>
    `;

    // Hydrate exposure from config.
    try {
        const session = await invoke("get_session");
        if (session.exposure_us) {
            document.getElementById("exposure-slider").value = session.exposure_us;
            document.getElementById("exposure-value").value = session.exposure_us;
        }
    } catch (e) {}

    const slider = document.getElementById("exposure-slider");
    const valueInput = document.getElementById("exposure-value");
    const step = 1000;

    function setExposure(us) {
        us = Math.max(1000, Math.min(200000, us));
        slider.value = us;
        valueInput.value = us;
        invoke("set_exposure", { exposureUs: us });
    }

    slider.addEventListener("input", () => setExposure(parseInt(slider.value)));
    valueInput.addEventListener("change", () => setExposure(parseInt(valueInput.value)));
    document.getElementById("exp-minus").addEventListener("click", () => setExposure(parseInt(slider.value) - step));
    document.getElementById("exp-plus").addEventListener("click", () => setExposure(parseInt(slider.value) + step));

    // Anatomical capture — save 16-bit PNG via file dialog.
    document.getElementById("btn-capture-anat").addEventListener("click", async () => {
        const statusEl = document.getElementById("anat-status");
        statusEl.textContent = "Saving...";
        try {
            const dataDir = await invoke("get_data_directory");
            const ts = Math.floor(Date.now() / 1000);
            const path = (dataDir || ".") + "\\anatomical_" + ts + ".png";
            await invoke("capture_anatomical", { path });
            statusEl.textContent = "Captured";
            statusEl.style.color = "var(--success)";
        } catch (e) {
            statusEl.textContent = `Error: ${e}`;
            statusEl.style.color = "var(--error)";
        }
    });

    // Ring overlay drawing.
    const ringCanvas = document.getElementById("layer-ring");
    const camCanvas = document.getElementById("layer-base");

    // Ensure ring layer visibility matches the checkbox.
    if (ring.enabled) {
        window.openISI.viz.ringVisible = true;
        window.openISI.updateRingLayer();
    }

    function drawRing() {
        if (!ringCanvas || !camCanvas) return;
        const enabled = document.getElementById("ring-enabled")?.checked;
        const ctx = ringCanvas.getContext("2d");

        // Match overlay canvas size to camera canvas display size.
        const container = ringCanvas.parentElement;
        ringCanvas.width = container.clientWidth;
        ringCanvas.height = container.clientHeight;
        ctx.clearRect(0, 0, ringCanvas.width, ringCanvas.height);

        if (!enabled) return;

        const radiusEl = document.getElementById("ring-radius");
        const cxEl = document.getElementById("ring-cx");
        const cyEl = document.getElementById("ring-cy");
        if (!radiusEl || !cxEl || !cyEl) return;

        const radius = parseInt(radiusEl.value);
        const cx = parseInt(cxEl.value);
        const cy = parseInt(cyEl.value);

        // Scale from camera pixel coords to display coords.
        if (camCanvas.width === 0 || camCanvas.height === 0) return;
        const scaleX = ringCanvas.width / camCanvas.width;
        const scaleY = ringCanvas.height / camCanvas.height;

        ctx.beginPath();
        ctx.arc(cx * scaleX, cy * scaleY, radius * scaleX, 0, Math.PI * 2);
        ctx.strokeStyle = "rgba(0, 255, 0, 0.7)";
        ctx.lineWidth = 2;
        ctx.stroke();

        // Crosshair at center.
        const cxd = cx * scaleX;
        const cyd = cy * scaleY;
        ctx.beginPath();
        ctx.moveTo(cxd - 8, cyd);
        ctx.lineTo(cxd + 8, cyd);
        ctx.moveTo(cxd, cyd - 8);
        ctx.lineTo(cxd, cyd + 8);
        ctx.strokeStyle = "rgba(0, 255, 0, 0.5)";
        ctx.lineWidth = 1;
        ctx.stroke();
    }

    // Redraw ring and persist when controls change.
    function saveRingConfig() {
        const overlay = {
            enabled: document.getElementById("ring-enabled").checked,
            radius_px: parseInt(document.getElementById("ring-radius").value),
            center_x_px: parseInt(document.getElementById("ring-cx").value),
            center_y_px: parseInt(document.getElementById("ring-cy").value),
        };
        invoke("set_ring_overlay", { overlay });
    }

    ["ring-enabled", "ring-radius", "ring-cx", "ring-cy"].forEach(id => {
        const el = document.getElementById(id);
        if (el) {
            el.addEventListener("input", () => {
                // Sync ring visibility with the layer system.
                if (id === "ring-enabled") {
                    window.openISI.viz.ringVisible = el.checked;
                    window.openISI.updateRingLayer();
                }
                drawRing();
                saveRingConfig();
            });
            el.addEventListener("change", () => {
                if (id === "ring-enabled") {
                    window.openISI.viz.ringVisible = el.checked;
                    window.openISI.updateRingLayer();
                }
                drawRing();
                saveRingConfig();
            });
        }
    });

    // Update pix/mm calculation.
    function updatePixPerMm() {
        const radiusPx = parseInt(document.getElementById("ring-radius")?.value) || 0;
        const diameterMm = parseFloat(document.getElementById("ring-diameter-mm")?.value) || 0;
        const el = document.getElementById("ring-pix-per-mm");
        if (el && radiusPx > 0 && diameterMm > 0) {
            const pixPerMm = (radiusPx * 2) / diameterMm;
            el.textContent = `= ${pixPerMm.toFixed(1)} px/mm`;
        }
    }
    document.getElementById("ring-radius")?.addEventListener("input", updatePixPerMm);
    document.getElementById("ring-diameter-mm")?.addEventListener("input", updatePixPerMm);
    updatePixPerMm();

    // Click-drag to move ring center.
    ringCanvas.style.pointerEvents = "auto";
    let dragging = false;
    ringCanvas.addEventListener("mousedown", (e) => {
        if (!document.getElementById("ring-enabled").checked) return;
        dragging = true;
        updateRingCenter(e);
    });
    ringCanvas.addEventListener("mousemove", (e) => {
        if (!dragging) return;
        updateRingCenter(e);
    });
    window.addEventListener("mouseup", () => { dragging = false; });

    function updateRingCenter(e) {
        const rect = ringCanvas.getBoundingClientRect();
        const displayX = e.clientX - rect.left;
        const displayY = e.clientY - rect.top;
        if (camCanvas.width === 0 || camCanvas.height === 0) return;
        const scaleX = camCanvas.width / ringCanvas.width;
        const scaleY = camCanvas.height / ringCanvas.height;
        const cxEl = document.getElementById("ring-cx");
        const cyEl = document.getElementById("ring-cy");
        cxEl.value = Math.round(displayX * scaleX);
        cyEl.value = Math.round(displayY * scaleY);
        drawRing();
        saveRingConfig();
    }

    // Scroll to change ring radius.
    ringCanvas.addEventListener("wheel", (e) => {
        if (!document.getElementById("ring-enabled").checked) return;
        e.preventDefault();
        const radiusEl = document.getElementById("ring-radius");
        const delta = e.deltaY > 0 ? -5 : 5;
        const newVal = Math.max(10, Math.min(500, parseInt(radiusEl.value) + delta));
        radiusEl.value = newVal;
        drawRing();
        saveRingConfig();
    }, { passive: false });

    // Redraw periodically while focus is active (camera frames change canvas size).
    const ringInterval = setInterval(drawRing, 200);
    unlisteners.push(() => clearInterval(ringInterval));
}

async function renderProtocol(body) {
    let exp;
    try {
        exp = await invoke("get_experiment");
    } catch (e) {
        body.innerHTML = `<p style="color: var(--text-muted);">Failed to load experiment: ${e}</p>`;
        return;
    }

    const p = exp.stimulus.params;

    // Load available saved experiments.
    let savedExperiments = [];
    try { savedExperiments = await invoke("list_experiments"); } catch (_) {}

    // Fetch descriptor groups for stimulus, geometry, and timing.
    const stimulusDescs = await fetchGroupDescriptors(invoke, "stimulus");
    const geometryDescs = await fetchGroupDescriptors(invoke, "geometry");
    const timingDescs = await fetchGroupDescriptors(invoke, "timing");

    // Build descriptor-driven HTML for each group.
    const stimulusHTML = stimulusDescs.map(buildParamInput).filter(Boolean).join('\n');
    const geometryHTML = geometryDescs.map(buildParamInput).filter(Boolean).join('\n');
    const timingHTML = timingDescs.map(buildParamInput).filter(Boolean).join('\n');

    body.innerHTML = `
        <div class="card">
            <h3>Experiment</h3>
            <div class="form-row">
                <label>Load saved</label>
                <select id="experiment-picker" style="flex:1">
                    <option value="">— Current —</option>
                    ${savedExperiments.map(e => `<option value="${e.path}">${e.name || e.path.split(/[\\/]/).pop()} (${e.envelope}, ${e.conditions.join("/")})</option>`).join("")}
                </select>
            </div>
            <div class="form-row">
                <label>Save as</label>
                <input type="text" id="experiment-save-name" placeholder="Name..." style="flex:1">
                <button id="btn-save-experiment" style="margin-left:6px">Save</button>
            </div>
        </div>
        <div class="card">
            <h3>Stimulus</h3>
            <input type="hidden" id="proto-rotation" value="${p.rotation_deg}">
            <div class="form-grid-3">
                ${stimulusHTML}
            </div>
        </div>
        <div class="card">
            <h3>Geometry</h3>
            ${geometryHTML}
        </div>
        <div style="display:flex; gap:12px;">
            <div class="card" style="flex:1;">
                <h3>Presentation</h3>
                <div class="form-row" style="align-items:flex-start">
                    <label>Conditions</label>
                    <div id="conditions-list" style="flex:1"></div>
                </div>
                <div class="form-row">
                    <label>Repetitions</label>
                    <input type="number" id="proto-reps" min="1" max="100" value="${exp.presentation.repetitions}" style="width:60px">
                </div>
                <div class="form-row">
                    <label>Order</label>
                    <select id="proto-order">
                        <option value="sequential" ${exp.presentation.order === "sequential" ? "selected" : ""}>Sequential</option>
                        <option value="interleaved" ${exp.presentation.order === "interleaved" ? "selected" : ""}>Interleaved</option>
                        <option value="randomized" ${exp.presentation.order === "randomized" ? "selected" : ""}>Randomized</option>
                    </select>
                </div>
            </div>
            <div class="card" style="flex:1;">
                <h3>Timing</h3>
                ${timingHTML}
            </div>
        </div>
        <div class="card">
            <h3>Summary</h3>
            <div id="duration-summary" class="form-row">
                <span class="mono-value" style="color: var(--text-muted)">Calculating...</span>
            </div>
        </div>
        <div style="display: flex; gap: 8px; margin-top: 8px;">
            <button id="btn-preview">Preview</button>
            <button id="btn-stop-preview">Stop Preview</button>
        </div>
    `;

    // Load duration summary.
    try {
        const dur = await invoke("get_duration_summary");
        const el = document.getElementById("duration-summary");
        if (el) {
            el.innerHTML = `
                <span class="mono-value">${dur.sweep_count} sweeps \u00d7 ${dur.sweep_duration_sec.toFixed(1)}s = ${dur.formatted}</span>
            `;
        }
    } catch (_) {
        const el = document.getElementById("duration-summary");
        if (el) el.innerHTML = `<span class="mono-value" style="color: var(--text-muted)">Select a display to compute duration</span>`;
    }

    // Wire descriptor-driven param inputs: set_params on change, refresh duration after.
    wireParamListeners(body, invoke, async () => {
        try {
            const dur = await invoke("get_duration_summary");
            const el = document.getElementById("duration-summary");
            if (el) el.innerHTML = `<span class="mono-value">${dur.sweep_count} sweeps \u00d7 ${dur.sweep_duration_sec.toFixed(1)}s = ${dur.formatted}</span>`;
        } catch (_) {}

        // If preview is running, restart it with new params.
        if (previewStatus && previewStatus.textContent === "Running") {
            await invoke("stop_preview");
            await invoke("start_preview");
        }
    });

    // Auto-save for custom fields (conditions, order, reps) that aren't in the registry.
    let saveTimeout;
    let previewStatus = null;
    function scheduleAutoSave() {
        clearTimeout(saveTimeout);
        saveTimeout = setTimeout(autoSaveCustomFields, 500);
    }

    // Experiment picker — load a saved experiment.
    document.getElementById("experiment-picker").addEventListener("change", async (e) => {
        const path = e.target.value;
        if (!path) return;
        try {
            await invoke("load_experiment", { path });
            renderProtocol(body); // Re-render with loaded experiment.
        } catch (err) {
            alert("Failed to load experiment: " + err);
        }
    });

    // Save current experiment as a named file.
    document.getElementById("btn-save-experiment").addEventListener("click", async () => {
        const name = document.getElementById("experiment-save-name").value.trim();
        if (!name) return;
        try {
            await invoke("save_experiment_as", { name });
            document.getElementById("experiment-save-name").value = "";
            renderProtocol(body); // Re-render to update picker.
        } catch (err) {
            alert("Failed to save experiment: " + err);
        }
    });

    // Envelope → conditions mapping.
    const envelopeConditions = {
        bar: ["LR", "RL", "TB", "BT"],
        wedge: ["CW", "CCW"],
        ring: ["Expand", "Contract"],
        fullfield: ["On"],
    };

    // Conditions list — orderable, toggleable.
    // `activeConditions` is the ordered list of enabled conditions (the SSoT).
    let activeConditions = [...exp.presentation.conditions];

    function buildConditionsList(envelope) {
        const container = document.getElementById("conditions-list");
        if (!container) return;
        const pool = envelopeConditions[envelope] || [];

        // Show enabled ones in activeConditions order first, then disabled ones.
        const ordered = [...activeConditions.filter(c => pool.includes(c)), ...pool.filter(c => !activeConditions.includes(c))];

        container.innerHTML = ordered.map(cond => {
            const enabled = activeConditions.includes(cond);
            return `
                <div class="cond-item ${enabled ? "" : "cond-disabled"}" data-cond="${cond}">
                    <input type="checkbox" class="cond-check" data-cond="${cond}" ${enabled ? "checked" : ""} style="display:none">
                    <span class="cond-label">${cond}</span>
                    <span class="cond-drag">\u2630</span>
                </div>`;
        }).join("");

        // Pointer-based: click to toggle, drag to reorder.
        let dragEl = null;
        let startY = 0;
        let didDrag = false;
        let placeholder = null;
        const DRAG_THRESHOLD = 5;

        container.querySelectorAll(".cond-item").forEach(item => {
            item.addEventListener("pointerdown", (e) => {
                e.preventDefault();
                dragEl = item;
                startY = e.clientY;
                didDrag = false;
                placeholder = null;
                item.setPointerCapture(e.pointerId);
            });
        });

        container.addEventListener("pointermove", (e) => {
            if (!dragEl) return;
            const dy = Math.abs(e.clientY - startY);

            // Start drag after threshold.
            if (!didDrag && dy > DRAG_THRESHOLD) {
                didDrag = true;
                dragEl.classList.add("dragging");
                placeholder = document.createElement("div");
                placeholder.className = "cond-placeholder";
                placeholder.style.height = dragEl.offsetHeight + "px";
                container.insertBefore(placeholder, dragEl);
                const rect = dragEl.getBoundingClientRect();
                dragEl.style.position = "fixed";
                dragEl.style.zIndex = "1000";
                dragEl.style.width = rect.width + "px";
                dragEl.style.left = rect.left + "px";
                dragEl.style.top = rect.top + "px";
            }

            if (didDrag && placeholder) {
                dragEl.style.top = (e.clientY - 12) + "px";
                // Move placeholder to correct position.
                const items = [...container.querySelectorAll(".cond-item:not(.dragging)")];
                for (const item of items) {
                    const rect = item.getBoundingClientRect();
                    if (e.clientY < rect.top + rect.height / 2) {
                        container.insertBefore(placeholder, item);
                        return;
                    }
                }
                container.appendChild(placeholder);
            }
        });

        container.addEventListener("pointerup", () => {
            if (!dragEl) return;

            if (didDrag && placeholder) {
                // End drag — drop at placeholder position.
                dragEl.style.position = "";
                dragEl.style.zIndex = "";
                dragEl.style.width = "";
                dragEl.style.left = "";
                dragEl.style.top = "";
                dragEl.classList.remove("dragging");
                container.insertBefore(dragEl, placeholder);
                placeholder.remove();

                // Rebuild activeConditions from DOM order.
                const newOrder = [];
                container.querySelectorAll(".cond-item").forEach(el => {
                    const cb = el.querySelector(".cond-check");
                    if (cb && cb.checked) newOrder.push(el.dataset.cond);
                });
                activeConditions = newOrder;
                scheduleAutoSave();
            } else {
                // Click — toggle enabled/disabled.
                const cond = dragEl.dataset.cond;
                const cb = dragEl.querySelector(".cond-check");
                if (cb) {
                    cb.checked = !cb.checked;
                    if (cb.checked) {
                        if (!activeConditions.includes(cond)) activeConditions.push(cond);
                        dragEl.classList.remove("cond-disabled");
                    } else {
                        activeConditions = activeConditions.filter(c => c !== cond);
                        dragEl.classList.add("cond-disabled");
                    }
                    scheduleAutoSave();
                }
            }

            dragEl = null;
            placeholder = null;
            didDrag = false;
        });
    }

    buildConditionsList(exp.stimulus.envelope);

    // When envelope changes via descriptor (params:changed will show/hide speed fields),
    // also update the conditions pool.
    const envelopeEl = body.querySelector('[data-param-id="stimulus.envelope"]');
    if (envelopeEl) {
        envelopeEl.addEventListener("change", () => {
            const newEnvelope = envelopeEl.value;
            const pool = envelopeConditions[newEnvelope] || [];
            activeConditions = [...pool];
            buildConditionsList(newEnvelope);
            scheduleAutoSave();
        });
    }

    // Save custom fields (conditions, order, reps) that aren't in the descriptor registry.
    async function autoSaveCustomFields() {
        try {
            const updated = await invoke("get_experiment");
            updated.presentation.conditions = [...activeConditions];
            updated.presentation.repetitions = parseInt(document.getElementById("proto-reps").value);
            updated.presentation.order = document.getElementById("proto-order").value;
            await invoke("update_experiment", { config: updated });

            // Refresh duration summary.
            try {
                const dur = await invoke("get_duration_summary");
                const el = document.getElementById("duration-summary");
                if (el) el.innerHTML = `<span class="mono-value">${dur.sweep_count} sweeps \u00d7 ${dur.sweep_duration_sec.toFixed(1)}s = ${dur.formatted}</span>`;
            } catch (_) {}
        } catch (e) {
            console.error("auto-save custom fields:", e);
        }
    }

    // Wire custom fields (non-descriptor) to auto-save.
    ["proto-reps", "proto-order"].forEach(id => {
        const el = document.getElementById(id);
        if (el) {
            el.addEventListener("change", scheduleAutoSave);
            el.addEventListener("input", scheduleAutoSave);
        }
    });

    previewStatus = document.createElement("span");
    previewStatus.className = "mono-value";
    previewStatus.style.marginLeft = "8px";
    document.getElementById("btn-preview").parentElement.appendChild(previewStatus);

    document.getElementById("btn-preview").addEventListener("click", async () => {
        try {
            await invoke("start_preview");
            previewStatus.textContent = "Running";
            previewStatus.style.color = "var(--success)";
        } catch (e) {
            previewStatus.textContent = `Error: ${e}`;
            previewStatus.style.color = "var(--error)";
        }
    });
    document.getElementById("btn-stop-preview").addEventListener("click", async () => {
        try {
            await invoke("stop_preview");
            previewStatus.textContent = "Stopped";
            previewStatus.style.color = "var(--text-secondary)";
        } catch (e) {
            previewStatus.textContent = `Error: ${e}`;
            previewStatus.style.color = "var(--error)";
        }
    });
}

async function renderAcquire(body) {
    const session = await invoke("get_session");

    // Check prerequisites.
    const tc = session.timing_characterization;
    const timingOk = !!tc;
    const timingWarn = tc && tc.regime === "SYSTEMATIC";
    const checks = [
        { label: "Display selected", ok: !!session.selected_display, fix: "setup" },
        { label: "Display validated", ok: !!session.display_validation, fix: "setup" },
        { label: "Camera connected", ok: session.camera_connected, fix: "setup" },
        { label: "Timing validated", ok: timingOk, warn: timingWarn, fix: "acquire" },
    ];
    const allReady = checks.every(c => c.ok);

    const disp = session.selected_display;
    const val = session.display_validation;
    const cam = session.camera;

    body.innerHTML = `
        <div style="display:flex; gap:12px;">
            <div class="card" style="flex:1;">
                <h3>Readiness</h3>
                ${checks.map(c => {
                    const color = c.ok ? (c.warn ? "var(--warning, orange)" : "var(--success)") : "var(--error)";
                    const icon = c.ok ? (c.warn ? "!" : "\u2713") : "\u2717";
                    return `
                    <div class="form-row">
                        <span style="color: ${color}; width: 16px; font-weight: bold;">${icon}</span>
                        <span>${c.label}${c.warn ? " (systematic)" : ""}</span>
                    </div>`;
                }).join("")}
                ${!timingOk && session.display_validation && session.camera_connected ? `
                    <div class="form-row" style="margin-top:8px;">
                        <button id="btn-validate-timing" class="primary" style="flex:1">Validate Timing (~3s)</button>
                    </div>
                ` : ""}
            </div>
            <div class="card" style="flex:1;">
                <h3>Hardware</h3>
            <div class="form-row">
                <label>Stimulus display</label>
                <span class="mono-value">${disp ? `${disp.name} ${disp.width_px}\u00d7${disp.height_px} ${disp.width_cm.toFixed(1)}\u00d7${disp.height_cm.toFixed(1)}cm` : "\u2014"}</span>
            </div>
            <div class="form-row">
                <label>Refresh rate</label>
                <span class="mono-value">${val ? `${val.measured_refresh_hz.toFixed(2)} Hz (jitter ${val.jitter_us.toFixed(1)} \u00b5s, ${val.sample_count} samples)` : "\u2014"}</span>
            </div>
            <div class="form-row">
                <label>Camera</label>
                <span class="mono-value">${cam ? `${cam.model} ${cam.width_px}\u00d7${cam.height_px} ${cam.exposure_us}\u00b5s` : "\u2014"}</span>
            </div>
            ${tc ? `
            <div class="form-row">
                <label>Camera rate</label>
                <span class="mono-value">${tc.f_cam_hz.toFixed(3)} Hz (jitter ${(tc.cam_jitter_sec * 1e6).toFixed(1)} \u00b5s)</span>
            </div>
            <div class="form-row">
                <label>Stimulus rate</label>
                <span class="mono-value">${tc.f_stim_hz.toFixed(3)} Hz (jitter ${(tc.stim_jitter_sec * 1e6).toFixed(1)} \u00b5s)</span>
            </div>
            <div class="form-row">
                <label>Beat period</label>
                <span class="mono-value">${tc.beat_period_sec.toFixed(3)}s</span>
            </div>
            <div class="form-row">
                <label>Regime</label>
                <span class="mono-value" style="color: ${tc.regime === "SYSTEMATIC" ? "var(--error)" : tc.regime === "partial" ? "var(--warning, orange)" : "var(--success)"}">${tc.regime}</span>
            </div>
            <div class="form-row">
                <label>Phase coverage</label>
                <span class="mono-value">${(tc.phase_coverage * 100).toFixed(1)}%</span>
            </div>
            <div class="form-row">
                <label>Onset uncertainty</label>
                <span class="mono-value">\u00b1${(tc.onset_uncertainty_sec * 1e6).toFixed(1)} \u00b5s (${(tc.onset_uncertainty_fraction * 100).toFixed(1)}% of frame)</span>
            </div>
            ${tc.warnings && tc.warnings.length > 0 ? tc.warnings.map(w => `
                <div class="form-row" style="color: var(--error); font-size: 11px; line-height: 1.3;">
                    ${w}
                </div>
            `).join("") : ""}
            ` : ""}
            </div>
        </div>

        <div class="card">
            <h3>Session</h3>
            <div class="form-row">
                <label>Animal ID</label>
                <input type="text" id="animal-id" placeholder="e.g. M001" style="flex:1">
            </div>
            <div class="form-row">
                <label>Notes</label>
                <input type="text" id="acq-notes" placeholder="Free text notes..." style="flex:1">
            </div>
        </div>

        <div class="card">
            <h3>Acquisition</h3>
            <div class="form-row">
                <button class="primary" id="btn-start-acq" ${allReady ? "" : "disabled"}>Start Acquisition</button>
                <button id="btn-stop-acq">Stop</button>
            </div>
            <div id="acq-dashboard" style="margin-top: 12px;">
                <div class="form-row">
                    <label>State</label>
                    <span id="acq-state" class="mono-value">—</span>
                </div>
                <div class="form-row">
                    <label>Condition</label>
                    <span id="acq-condition" class="mono-value">—</span>
                </div>
                <div class="form-row">
                    <label>Sweep</label>
                    <span id="acq-sweep" class="mono-value">—</span>
                </div>
                <div class="form-row">
                    <label>Progress</label>
                    <span id="acq-progress-val" class="mono-value">—</span>
                </div>
                <div class="form-row">
                    <label>Time</label>
                    <span id="acq-time" class="mono-value">—</span>
                </div>
                <div class="form-row">
                    <label>Stimulus delta</label>
                    <span id="acq-delta" class="mono-value">—</span>
                </div>
                <div class="form-row">
                    <label>Stimulus FPS</label>
                    <span id="acq-fps" class="mono-value">—</span>
                </div>
            </div>
        </div>
    `;

    // Fix buttons navigate to the right section.

    // Timing validation button.
    const btnTiming = document.getElementById("btn-validate-timing");
    if (btnTiming) {
        btnTiming.addEventListener("click", async () => {
            btnTiming.disabled = true;
            btnTiming.textContent = "Measuring...";
            try {
                await invoke("validate_timing");
                // Re-render to show results.
                renderAcquire(body);
            } catch (e) {
                btnTiming.disabled = false;
                btnTiming.textContent = `Error: ${e}`;
                setTimeout(() => { btnTiming.textContent = "Validate Timing (~3s)"; btnTiming.disabled = false; }, 3000);
            }
        });
    }

    // Live acquisition progress.
    const unlisten = await listen("stimulus:frame", (event) => {
        const f = event.payload;
        const set = (id, val) => { const el = document.getElementById(id); if (el) el.textContent = val; };

        set("acq-state", f.state);
        set("acq-condition", f.condition);
        set("acq-sweep", `${f.sweep_index + 1} / ${f.total_sweeps}`);
        set("acq-progress-val", `${(f.state_progress * 100).toFixed(1)}%`);

        const elapsed = f.elapsed_sec.toFixed(1);
        const total = (f.elapsed_sec + f.remaining_sec).toFixed(1);
        const remaining = f.remaining_sec.toFixed(1);
        set("acq-time", `${elapsed}s / ${total}s (${remaining}s remaining)`);

        const delta_ms = (f.frame_delta_us / 1000).toFixed(2);
        set("acq-delta", `${delta_ms} ms`);

        if (f.frame_delta_us > 0) {
            const fps = (1000000 / f.frame_delta_us).toFixed(1);
            set("acq-fps", `${fps} Hz`);
        }
    });
    unlisteners.push(unlisten);

    document.getElementById("btn-start-acq").addEventListener("click", async () => {
        // Save session metadata before starting.
        const animalId = document.getElementById("animal-id").value;
        const notes = document.getElementById("acq-notes").value;
        await invoke("set_session_metadata", { animalId, notes });
        try {
            await invoke("start_acquisition");
            document.getElementById("acq-state").textContent = "Starting...";
        } catch (e) {
            document.getElementById("acq-state").textContent = `Error: ${e}`;
        }
    });

    document.getElementById("btn-stop-acq").addEventListener("click", async () => {
        try { await invoke("stop_acquisition"); } catch (e) { console.error("stop:", e); }
    });

    // Save prompt when acquisition completes.
    const unlistenComplete = await listen("stimulus:complete", (event) => {
        const s = event.payload.summary;
        const dashboard = document.getElementById("acq-dashboard");
        if (!dashboard) return;
        dashboard.innerHTML = `
            <div class="card" style="border-color: var(--accent); margin-top: 8px;">
                <h3>Acquisition Complete</h3>
                <div class="form-row">
                    <label>Frames</label>
                    <span class="mono-value">${s.total_frames}</span>
                </div>
                <div class="form-row">
                    <label>Sweeps</label>
                    <span class="mono-value">${s.total_sweeps}</span>
                </div>
                <div class="form-row">
                    <label>Duration</label>
                    <span class="mono-value">${s.duration_sec.toFixed(1)}s</span>
                </div>
                <div class="form-row">
                    <label>Dropped</label>
                    <span class="mono-value">${s.dropped_frames}</span>
                </div>
                <div style="display: flex; gap: 8px; margin-top: 12px;">
                    <button class="primary" id="btn-save-acq">Save</button>
                    <button id="btn-discard-acq">Discard</button>
                </div>
                <div id="save-status" class="mono-value" style="margin-top: 4px;"></div>
            </div>
        `;

        document.getElementById("btn-save-acq").addEventListener("click", async () => {
            const statusEl = document.getElementById("save-status");
            statusEl.textContent = "Saving...";
            try {
                const filePath = await invoke("save_acquisition", { path: null });
                statusEl.innerHTML = `Saved: ${filePath}<br><button id="btn-go-analysis" class="primary" style="margin-top:8px">Analyze \u2192</button>`;
                statusEl.style.color = "var(--success)";
                // Enable analysis view and wire up the button.
                window.openISI.enableView("analysis");
                document.getElementById("btn-go-analysis").addEventListener("click", async () => {
                    const btn = document.getElementById("btn-go-analysis");
                    btn.textContent = "Analyzing...";
                    btn.disabled = true;
                    await invoke("run_analysis", { path: filePath });
                    window.openISI._analysisFile = filePath;
                    window.openISI.viz.analysisFile = filePath;
                    window.openISI.showView("analysis");
                });
            } catch (e) {
                statusEl.textContent = `Error: ${e}`;
                statusEl.style.color = "var(--error)";
            }
        });

        document.getElementById("btn-discard-acq").addEventListener("click", async () => {
            await invoke("discard_acquisition");
            const statusEl = document.getElementById("save-status");
            statusEl.textContent = "Discarded";
            statusEl.style.color = "var(--text-muted)";
        });
    });
    unlisteners.push(unlistenComplete);
}

// ═══════════════════════════════════════════════════════════════════════
// Status refresh
// ═══════════════════════════════════════════════════════════════════════

async function refreshStatuses() {
    try {
        const session = await invoke("get_session");
        const exp = await invoke("get_experiment");
        const set = (id, text) => { const el = document.getElementById(id); if (el) el.textContent = text; };

        // Setup.
        const setupParts = [];
        if (session.selected_display) {
            setupParts.push(`Display: ${session.selected_display.name}`);
        }
        if (session.display_validation) {
            setupParts.push(`${session.display_validation.measured_refresh_hz.toFixed(1)} Hz`);
        }
        if (session.camera_connected && session.camera) {
            setupParts.push(`${session.camera.model} ${session.camera.width_px}×${session.camera.height_px}`);
        }
        set("status-setup", setupParts.length > 0 ? setupParts.join("  ·  ") : "Not started");

        // Focus.
        set("status-focus", `Exposure: ${session.exposure_us}µs`);

        // Protocol.
        set("status-protocol", `${exp.stimulus.envelope} · ${exp.presentation.conditions.length} cond × ${exp.presentation.repetitions} reps`);

        // Acquire.
        if (session.is_acquiring) {
            set("status-acquire", "Acquiring...");
        } else if (session.last_acquisition) {
            set("status-acquire", `✓ ${session.last_acquisition.total_frames} frames saved`);
        } else {
            set("status-acquire", "Ready");
        }

    } catch (e) {}
}
