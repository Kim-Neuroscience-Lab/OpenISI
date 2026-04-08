// Library view — file explorer for .oisi acquisitions.

const { invoke } = window.__TAURI__.core;

let sortField = "modified_epoch";
let sortAsc = false; // newest first by default
let selectedPaths = new Set();

export async function render(container) {
    const dataDir = await invoke("get_data_directory");
    let files = [];
    try { files = await invoke("list_oisi_files"); } catch (_) {}
    const hasDataDir = dataDir.length > 0;

    // Sort.
    files.sort((a, b) => {
        let va = a[sortField], vb = b[sortField];
        if (typeof va === "string") va = va.toLowerCase();
        if (typeof vb === "string") vb = vb.toLowerCase();
        if (va < vb) return sortAsc ? -1 : 1;
        if (va > vb) return sortAsc ? 1 : -1;
        return 0;
    });

    // Clear selection of files that no longer exist.
    const existingPaths = new Set(files.map(f => f.path));
    for (const p of selectedPaths) {
        if (!existingPaths.has(p)) selectedPaths.delete(p);
    }

    const selCount = selectedPaths.size;

    container.innerHTML = `
        <div class="library-header">
            <div class="library-actions">
                <button class="primary" id="btn-new-session">New Session</button>
                <button id="btn-import-snlc">Import SNLC Data</button>
                <button id="btn-download-sample">Download Sample Data</button>
                <button id="btn-set-data-dir">Set Data Directory</button>
                <span class="mono-value" style="align-self:center; font-size: 11px;">${hasDataDir ? dataDir : "No directory set"}</span>
            </div>
            <div class="library-toolbar" id="toolbar" style="display: ${selCount > 0 ? "flex" : "none"}; align-items: center; gap: 8px; margin-top: 8px;">
                <span class="mono-value" style="font-size: 11px;">${selCount} selected</span>
                <button id="btn-select-all" style="font-size: 11px;">Select All</button>
                <button id="btn-select-none" style="font-size: 11px;">Clear</button>
                <span style="flex:1"></span>
                <button id="btn-delete-selected" style="font-size: 11px; color: var(--error);">Delete Selected</button>
            </div>
        </div>

        ${files.length > 0 ? `
            <table class="file-table">
                <thead>
                    <tr>
                        <th style="width: 30px;"><input type="checkbox" id="select-all-cb" ${selCount === files.length && files.length > 0 ? "checked" : ""}></th>
                        <th class="sortable" data-sort="filename">Name ${sortIcon("filename")}</th>
                        <th class="sortable" data-sort="modified_epoch">Date ${sortIcon("modified_epoch")}</th>
                        <th class="sortable" data-sort="size_bytes">Size ${sortIcon("size_bytes")}</th>
                        <th style="width: 80px;"></th>
                    </tr>
                </thead>
                <tbody>
                    ${files.map(f => {
                        const sel = selectedPaths.has(f.path);
                        return `
                        <tr data-path="${f.path}" class="${sel ? "selected" : ""}">
                            <td><input type="checkbox" class="file-cb" data-path="${f.path}" ${sel ? "checked" : ""}></td>
                            <td class="filename-cell">${f.filename}</td>
                            <td class="mono-value">${f.modified}</td>
                            <td class="mono-value">${formatSize(f.size_bytes)}</td>
                            <td><button class="btn-open-file" data-path="${f.path}">Analyze</button></td>
                        </tr>`;
                    }).join("")}
                </tbody>
            </table>
            <div class="library-footer mono-value" style="margin-top: 8px; font-size: 11px; color: var(--text-muted);">
                ${files.length} file${files.length !== 1 ? "s" : ""}, ${formatSize(files.reduce((s, f) => s + f.size_bytes, 0))} total
            </div>
        ` : `
            <div class="empty-state">
                <h2>OpenISI</h2>
                ${hasDataDir
                    ? `<p>No .oisi files found in the data directory.</p>`
                    : `<p>Set a data directory to browse acquisitions, or start a new session.</p>`
                }
            </div>
        `}
    `;

    // ── Event handlers ───────────────────────────────────────────

    // New Session.
    document.getElementById("btn-new-session").addEventListener("click", () => {
        window.openISI.enableView("session");
        window.openISI.showView("session");
    });

    // Set data directory.
    document.getElementById("btn-set-data-dir").addEventListener("click", async () => {
        try {
            const dialog = window.__TAURI__?.dialog;
            if (!dialog) return;
            const selected = await dialog.open({ directory: true, title: "Select Data Directory" });
            if (selected) {
                await invoke("set_data_directory", { path: selected });
                selectedPaths.clear();
                await render(container);
            }
        } catch (e) {
            console.error("set data dir:", e);
        }
    });

    // Import SNLC .mat data.
    document.getElementById("btn-import-snlc").addEventListener("click", async () => {
        try {
            const dialog = window.__TAURI__?.dialog;
            if (!dialog) return;
            const selected = await dialog.open({ directory: true, title: "Select SNLC .mat Directory" });
            if (!selected) return;
            const btn = document.getElementById("btn-import-snlc");
            btn.disabled = true;
            btn.textContent = "Importing...";
            const oisiPath = await invoke("import_snlc", { dirPath: selected });
            btn.textContent = "Import SNLC Data";
            btn.disabled = false;
            // Refresh file list and open the imported file.
            await render(container);
            window.openISI._analysisFile = oisiPath;
            window.openISI.enableView("analysis");
            window.openISI.showView("analysis");
        } catch (e) {
            const btn = document.getElementById("btn-import-snlc");
            if (btn) { btn.textContent = "Import SNLC Data"; btn.disabled = false; }
            alert(`Import failed: ${e}`);
        }
    });

    // Download SNLC sample data.
    document.getElementById("btn-download-sample").addEventListener("click", async () => {
        const btn = document.getElementById("btn-download-sample");
        try {
            btn.disabled = true;
            btn.textContent = "Downloading...";
            const paths = await invoke("import_snlc_sample_data");
            btn.textContent = "Download Sample Data";
            btn.disabled = false;
            await render(container);
            if (paths.length > 0) {
                window.openISI._analysisFile = paths[0];
                window.openISI.enableView("analysis");
                window.openISI.showView("analysis");
            }
        } catch (e) {
            btn.textContent = "Download Sample Data";
            btn.disabled = false;
            alert(`Sample data download failed: ${e}`);
        }
    });

    // Sort column headers.
    container.querySelectorAll(".sortable").forEach(th => {
        th.addEventListener("click", () => {
            const field = th.dataset.sort;
            if (sortField === field) {
                sortAsc = !sortAsc;
            } else {
                sortField = field;
                sortAsc = field === "filename"; // name ascending, date/size descending
            }
            render(container);
        });
    });

    // Lightweight selection update — no full re-render.
    function updateSelection() {
        const selCount = selectedPaths.size;
        const toolbar = document.getElementById("toolbar");
        if (toolbar) toolbar.style.display = selCount > 0 ? "flex" : "none";
        const selLabel = toolbar?.querySelector(".mono-value");
        if (selLabel) selLabel.textContent = `${selCount} selected`;
        container.querySelectorAll(".file-table tbody tr").forEach(tr => {
            const path = tr.dataset.path;
            const sel = selectedPaths.has(path);
            tr.classList.toggle("selected", sel);
            const cb = tr.querySelector(".file-cb");
            if (cb) cb.checked = sel;
        });
        const selectAllCb = document.getElementById("select-all-cb");
        if (selectAllCb) selectAllCb.checked = selCount === files.length && files.length > 0;
    }

    // Select-all checkbox in header.
    const selectAllCb = document.getElementById("select-all-cb");
    if (selectAllCb) {
        selectAllCb.addEventListener("change", () => {
            if (selectAllCb.checked) { files.forEach(f => selectedPaths.add(f.path)); }
            else { selectedPaths.clear(); }
            updateSelection();
        });
    }

    // Individual file checkboxes.
    container.querySelectorAll(".file-cb").forEach(cb => {
        cb.addEventListener("change", () => {
            if (cb.checked) { selectedPaths.add(cb.dataset.path); }
            else { selectedPaths.delete(cb.dataset.path); }
            updateSelection();
        });
    });

    // Select All / Clear buttons.
    document.getElementById("btn-select-all")?.addEventListener("click", () => {
        files.forEach(f => selectedPaths.add(f.path));
        updateSelection();
    });
    document.getElementById("btn-select-none")?.addEventListener("click", () => {
        selectedPaths.clear();
        updateSelection();
    });

    // Delete selected.
    document.getElementById("btn-delete-selected")?.addEventListener("click", async () => {
        const count = selectedPaths.size;
        if (count === 0) return;
        const msg = count === 1
            ? `Delete this file?\n\n${[...selectedPaths][0].split(/[\\/]/).pop()}`
            : `Delete ${count} files?\n\nThis cannot be undone.`;
        if (!confirm(msg)) return;
        try {
            await invoke("delete_oisi_files", { paths: [...selectedPaths] });
            selectedPaths.clear();
            await render(container);
        } catch (e) {
            alert(`Delete failed: ${e}`);
        }
    });

    // Open file for analysis.
    container.querySelectorAll(".btn-open-file").forEach(btn => {
        btn.addEventListener("click", async () => {
            window.openISI._analysisFile = btn.dataset.path;
            window.openISI.viz.analysisFile = btn.dataset.path;
            window.openISI.enableView("analysis");
            await window.openISI.showView("analysis");
        });
    });

    // Click row to toggle selection (but not on buttons/checkboxes).
    container.querySelectorAll(".file-table tbody tr").forEach(tr => {
        tr.addEventListener("click", (e) => {
            if (e.target.tagName === "INPUT" || e.target.tagName === "BUTTON") return;
            const path = tr.dataset.path;
            if (selectedPaths.has(path)) { selectedPaths.delete(path); }
            else { selectedPaths.add(path); }
            updateSelection();
        });
    });
}

function sortIcon(field) {
    if (sortField !== field) return "";
    return sortAsc ? "\u25b2" : "\u25bc";
}

function formatSize(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
