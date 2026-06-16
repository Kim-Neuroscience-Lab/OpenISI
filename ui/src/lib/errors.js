// Error-payload helpers for Tauri-invoke catch blocks.
//
// The Rust backend serializes errors via `AppErrorWire` — a structured
// object with `category`, `code`, `message`, optional `file_path` /
// `stage`, and a free-form `details` JSON. Older code paths may still
// surface bare strings (rejected promises whose reason is a string),
// so these helpers handle both shapes transparently.
//
// Use `errorToString(e)` in every `catch (e) { … }` block that
// interpolates `e` into a user-facing message. The previous
// `alert("Error: " + e)` pattern produced `"Error: [object Object]"`
// with the structured payload — that regression is closed here.
//
// The stable `code`/`category` vocabularies are GENERATED from the Rust error
// enums (`error-codes.generated.js`, the single source of truth). Branch on
// `ERROR_CODES.E_…` / `ERROR_CATEGORIES.…`, never on a bare string literal, so
// the frontend cannot drift from the backend.

import { ERROR_CODES, ERROR_CATEGORIES } from './error-codes.generated.js';

export { ERROR_CODES, ERROR_CATEGORIES };

/**
 * Render a Tauri/JS error as a human-readable string.
 *
 * Accepts any of:
 *  - a bare string (legacy / non-Tauri rejection)
 *  - an `AppErrorWire` object `{ category, code, message, … }`
 *  - any other thrown value (falls back to `String(e)`)
 *
 * @param {unknown} e
 * @returns {string}
 */
export function errorToString(e) {
    if (typeof e === 'string') return e;
    if (e && typeof e === 'object' && typeof e.message === 'string') return e.message;
    return String(e);
}

/**
 * Extract the stable machine-readable error code from a Tauri error
 * payload, or `null` if the error isn't a structured `AppErrorWire`.
 * Useful for branching on known error classes
 * (e.g. `if (errorCode(e) === ERROR_CODES.E_INVALID_PACKAGE) { … }`).
 *
 * @param {unknown} e
 * @returns {string | null}
 */
export function errorCode(e) {
    if (e && typeof e === 'object' && typeof e.code === 'string') return e.code;
    return null;
}

/**
 * Extract the category ("Analysis" | "Acquisition" | "Config" | …) from
 * a Tauri error payload, or `null` if it's a bare-string error.
 *
 * @param {unknown} e
 * @returns {string | null}
 */
export function errorCategory(e) {
    if (e && typeof e === 'object' && typeof e.category === 'string') return e.category;
    return null;
}
