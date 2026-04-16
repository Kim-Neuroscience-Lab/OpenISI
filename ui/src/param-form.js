// param-form.js — Descriptor-driven form builder for the reactive parameter registry.
// Builds HTML form elements from parameter descriptors returned by get_param_descriptors.

/**
 * Build an HTML input element from a parameter descriptor.
 * @param {object} desc — {id, label, unit, param_type, value, constraint, active, group}
 * @returns {string} HTML string
 */
export function buildParamInput(desc) {
    if (!desc.active) return '';

    if (desc.param_type === 'bool') {
        return `<div class="form-row" data-param-row="${desc.id}">
            <label>${desc.label}</label>
            <input type="checkbox" data-param-id="${desc.id}" ${desc.value ? 'checked' : ''}>
        </div>`;
    }

    if (desc.param_type === 'enum') {
        const options = (desc.constraint?.values || []).map(v =>
            `<option value="${v}" ${v === desc.value ? 'selected' : ''}>${v}</option>`
        ).join('');
        return `<div class="form-row" data-param-row="${desc.id}">
            <label>${desc.label}</label>
            <select data-param-id="${desc.id}">${options}</select>
        </div>`;
    }

    if (desc.param_type === 'string') {
        return `<div class="form-row" data-param-row="${desc.id}">
            <label>${desc.label}</label>
            <input type="text" data-param-id="${desc.id}" value="${desc.value ?? ''}" style="flex:1">
        </div>`;
    }

    // Numeric types: u16, u32, i32, usize, f64
    const c = desc.constraint || {};
    const attrs = [];
    if (c.min !== undefined) attrs.push(`min="${c.min}"`);
    if (c.max !== undefined) attrs.push(`max="${c.max}"`);
    if (desc.param_type === 'f64') attrs.push('step="any"');
    else attrs.push('step="1"');

    return `<div class="form-row" data-param-row="${desc.id}">
        <label>${desc.label}</label>
        <input type="number" data-param-id="${desc.id}" value="${desc.value}" ${attrs.join(' ')} style="width:70px">
        ${desc.unit ? `<span class="unit">${desc.unit}</span>` : ''}
    </div>`;
}

/**
 * Build a form section from a group of descriptors.
 * @param {Array} descriptors — array of parameter descriptors
 * @param {string} title — card heading
 * @returns {string} HTML string
 */
export function buildParamGroup(descriptors, title) {
    console.log(`[param-form] buildParamGroup("${title}"): ${descriptors.length} descriptors, active: ${descriptors.filter(d => d.active).length}`);
    const inputs = descriptors.map(d => {
        const html = buildParamInput(d);
        if (!html) console.log(`[param-form]   SKIPPED: ${d.id} (active=${d.active}, type=${d.param_type})`);
        return html;
    }).filter(Boolean).join('\n');
    if (!inputs) return '';
    return `<div class="card"><h3>${title}</h3>${inputs}</div>`;
}

/**
 * Wire change listeners on a container. On change, collect the value
 * and call set_params with the single update. Returns a list of teardown functions.
 * @param {HTMLElement} container
 * @param {Function} invoke — Tauri invoke function
 * @param {Function} [onAfterSet] — optional callback after successful set_params(id, value)
 */
export function wireParamListeners(container, invoke, onAfterSet) {
    container.querySelectorAll('[data-param-id]').forEach(el => {
        const handler = async () => {
            const id = el.dataset.paramId;
            let value;
            if (el.type === 'checkbox') {
                value = el.checked;
            } else if (el.tagName === 'SELECT') {
                value = el.value;
            } else if (el.type === 'number') {
                value = parseFloat(el.value);
                if (isNaN(value)) return;
                const min = parseFloat(el.min);
                const max = parseFloat(el.max);
                if (!isNaN(min) && value < min) { el.classList.add('invalid'); return; }
                if (!isNaN(max) && value > max) { el.classList.add('invalid'); return; }
                el.classList.remove('invalid');
            } else {
                value = el.value;
            }

            try {
                await invoke('set_params', { updates: { [id]: value } });
                el.classList.remove('invalid');
                if (onAfterSet) onAfterSet(id, value);
            } catch (e) {
                console.error(`set_params failed for ${id}:`, e);
                el.classList.add('invalid');
            }
        };
        el.addEventListener('change', handler);
    });
}

/**
 * Apply params:changed event to update form elements in a container.
 * @param {HTMLElement} container
 * @param {Array} changes — [{id, value, constraint?, active?}, ...]
 */
export function applyParamChanges(container, changes) {
    for (const change of changes) {
        const el = container.querySelector(`[data-param-id="${change.id}"]`);
        if (!el) continue;

        // Update value.
        if (change.value !== undefined) {
            if (el.type === 'checkbox') {
                el.checked = change.value;
            } else if (el.tagName === 'SELECT') {
                // If constraint changed with new enum values, rebuild options.
                if (change.constraint?.values) {
                    el.innerHTML = change.constraint.values.map(v =>
                        `<option value="${v}" ${v === change.value ? 'selected' : ''}>${v}</option>`
                    ).join('');
                } else {
                    el.value = change.value;
                }
            } else {
                el.value = change.value;
            }
        }

        // Update constraint attributes for numeric inputs.
        if (change.constraint) {
            if (change.constraint.min !== undefined) el.min = change.constraint.min;
            if (change.constraint.max !== undefined) el.max = change.constraint.max;
        }

        // Handle active/inactive visibility.
        if (change.active !== undefined) {
            const row = el.closest('.form-row');
            if (row) row.style.display = change.active ? '' : 'none';
        }
    }
}

/**
 * Fetch descriptors for a group and return them as an array.
 * @param {Function} invoke — Tauri invoke function
 * @param {string} group — group name to filter by
 * @returns {Promise<Array>} array of parameter descriptors
 */
export async function fetchGroupDescriptors(invoke, group) {
    try {
        const result = await invoke('get_param_descriptors', { group });
        console.log(`[param-form] get_param_descriptors("${group}") returned ${result.length} descriptors:`, result.map(d => d.id));
        return result;
    } catch (e) {
        console.error(`get_param_descriptors(${group}) failed:`, e);
        return [];
    }
}
