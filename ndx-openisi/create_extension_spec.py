#!/usr/bin/env python
"""Author the `ndx-openisi` NWB extension spec.

OpenISI intrinsic-signal-imaging retinotopy data is *mostly* expressible in core
NWB (raw frames → ``OnePhotonSeries``, sweep schedule → ``TimeIntervals``,
segmentation → ``PlaneSegmentation``, anatomical → ``GrayscaleImage``, subject/
device/session → ``/general``). This extension covers only the parts core NWB
cannot hold faithfully:

- **RetinotopyMaps** — the per-pixel retinotopy result maps (phase/amplitude/VFS/
  eccentricity/magnification/SNR/reliability), the replacement for the deprecated
  ``ImagingRetinotopy`` neurodata type. Each map carries OpenISI render metadata
  (palette, display range, circular wrap period, NaN/zero sentinel semantics) so a
  reader reproduces the figures without re-deriving conventions. Also holds the
  complex Fourier maps (split real/imag) and the stimulus geometry + analysis
  provenance that pin the maps to degrees of visual angle.
- **TimingForensics** — the multi-clock acquisition timing record (hardware vs
  system timestamps, clock offset/drift, camera/stimulus inter-frame deltas,
  sequence/drop gaps, the timing characterization). Core NWB's single timeline
  cannot represent the two physical clocks and their reconciliation, which is the
  evidence an ISI recording's timing is sound.

Run this to (re)generate ``spec/ndx-openisi.namespace.yaml`` +
``spec/ndx-openisi.extensions.yaml``. Authored via the spec API (not hand-edited
YAML) so the namespace is always internally consistent.
"""

from pynwb.spec import (
    NWBNamespaceBuilder,
    NWBGroupSpec,
    NWBDatasetSpec,
    NWBAttributeSpec,
    export_spec,
)

NS = "ndx-openisi"


def map_meta_attrs():
    """OpenISI render-metadata attributes carried by every result map — the
    data-layer ↔ renderer contract, lifted verbatim from the `.oisi` map attrs."""
    return [
        NWBAttributeSpec("palette", "Colormap name (e.g. hsv_circular, jet, binary).", "text"),
        NWBAttributeSpec("units", "Physical/semantic units (rad, deg, unitless, bool, label).", "text"),
        NWBAttributeSpec("display_min", "Value mapped to the palette start.", "float64"),
        NWBAttributeSpec("display_max", "Value mapped to the palette end.", "float64"),
        NWBAttributeSpec(
            "wrap_period",
            "Period for circular palettes (2*pi for rad, angular_range for deg, 0 if non-circular).",
            "float64",
        ),
        NWBAttributeSpec(
            "nan_means",
            "Semantic label for NaN pixels (e.g. outside_cortex); empty if NaN is not a sentinel.",
            "text",
        ),
        NWBAttributeSpec(
            "zero_means",
            "Semantic label for the literal-0.0 sentinel (e.g. outside_patch, below_threshold); empty if 0 is a real value.",
            "text",
        ),
    ]


def build():
    ns_builder = NWBNamespaceBuilder(
        name=NS,
        version="0.1.0",
        doc="OpenISI intrinsic-signal retinotopy extension: per-pixel retinotopy "
        "result maps (the ImagingRetinotopy replacement), complex Fourier maps, "
        "stimulus geometry, analysis provenance, and multi-clock acquisition "
        "timing forensics.",
        author=["Kim Neuroscience Lab"],
        contact=["a.murray0413@gmail.com"],
    )
    # Core types we extend / reference.
    for t in ("NWBDataInterface", "Image", "VectorData"):
        ns_builder.include_type(t, namespace="core")
    ns_builder.include_type("Data", namespace="hdmf-common")

    # ── A single retinotopy result map: an Image carrying OpenISI render meta ──
    retino_map = NWBDatasetSpec(
        neurodata_type_def="RetinotopyMap",
        neurodata_type_inc="Image",
        doc="A single 2-D (height x width) retinotopy result map with OpenISI "
        "render metadata. The data is the map; the attributes are the contract a "
        "renderer needs to reproduce the OpenISI figure.",
        attributes=map_meta_attrs(),
    )

    # ── A complex Fourier map: (H, W, 2) real/imag split ──
    complex_map = NWBDatasetSpec(
        neurodata_type_def="ComplexMap",
        neurodata_type_inc="Data",
        doc="A per-direction complex Fourier response map stored as a "
        "(height, width, 2) float64 array: [:, :, 0] = real, [:, :, 1] = imag. "
        "HDF5 has no portable native-complex convention, so OpenISI uses this "
        "documented real/imag split.",
        dtype="float64",
        dims=["height", "width", "real_imag"],
        shape=[None, None, 2],
        attributes=[
            NWBAttributeSpec(
                "convention",
                "Storage convention for the trailing axis.",
                "text",
                value="last_axis_real_imag",
            )
        ],
    )

    # ── The retinotopy result container ──
    retinotopy = NWBGroupSpec(
        neurodata_type_def="RetinotopyMaps",
        neurodata_type_inc="NWBDataInterface",
        doc="OpenISI retinotopy analysis result: the per-pixel maps (phase, "
        "amplitude, VFS, eccentricity, cortical magnification, SNR, reliability), "
        "the complex Fourier maps they derive from, the stimulus geometry that "
        "pins them to degrees of visual angle, and the analysis provenance. "
        "Replaces the deprecated core ImagingRetinotopy.",
        datasets=[
            NWBDatasetSpec(
                neurodata_type_inc="RetinotopyMap",
                doc="The retinotopy result maps (one RetinotopyMap per quantity, "
                "each named by its OpenISI map name e.g. azi_phase_degrees, vfs, "
                "eccentricity).",
                quantity="*",
            ),
            NWBDatasetSpec(
                neurodata_type_inc="ComplexMap",
                doc="The per-direction complex Fourier maps "
                "(azi_fwd, azi_rev, alt_fwd, alt_rev), each named by direction.",
                quantity="*",
            ),
        ],
        attributes=[
            NWBAttributeSpec(
                "rotation_k", "Camera-frame rotation applied to the maps (quarter turns).", "int32",
                required=False,
            ),
            NWBAttributeSpec(
                "azi_angular_range", "Azimuth sweep extent in degrees of visual angle.", "float64",
                required=False,
            ),
            NWBAttributeSpec(
                "alt_angular_range", "Altitude sweep extent in degrees of visual angle.", "float64",
                required=False,
            ),
            NWBAttributeSpec("offset_azi", "Azimuth center offset in degrees.", "float64", required=False),
            NWBAttributeSpec("offset_alt", "Altitude center offset in degrees.", "float64", required=False),
            NWBAttributeSpec(
                "analysis_params",
                "JSON of the tagged AnalysisConfig that produced these maps (per-stage "
                "method + active tunables) — the reproducibility provenance.",
                "text",
                required=False,
            ),
            NWBAttributeSpec(
                "software_version", "OpenISI version that produced the analysis.", "text", required=False
            ),
        ],
    )

    # ── Multi-clock acquisition timing forensics ──
    timing = NWBGroupSpec(
        neurodata_type_def="TimingForensics",
        neurodata_type_inc="NWBDataInterface",
        doc="Multi-clock acquisition timing record: the two physical clocks "
        "(camera hardware clock and the system QPC clock, the latter shared with "
        "the stimulus vsync), their reconciliation (offset + drift), and the "
        "per-frame interval / dropped-frame / sequence-gap evidence that an ISI "
        "recording's timing is sound. Core NWB's single timeline cannot represent "
        "two clocks and their offset.",
        datasets=[
            NWBDatasetSpec(
                name="camera_hardware_timestamps_us", doc="Camera internal-clock timestamps (microseconds).",
                dtype="int64", dims=["frame"], shape=[None], quantity="?",
            ),
            NWBDatasetSpec(
                name="camera_system_timestamps_us",
                doc="System QPC timestamps at camera frame read (microseconds; same clock as stimulus vsync).",
                dtype="int64", dims=["frame"], shape=[None], quantity="?",
            ),
            NWBDatasetSpec(
                name="camera_sequence_numbers", doc="Camera hardware sequence counters (gap detection).",
                dtype="int64", dims=["frame"], shape=[None], quantity="?",
            ),
            NWBDatasetSpec(
                name="camera_frame_deltas_us", doc="Inter-frame intervals from camera hardware timestamps (microseconds).",
                dtype="int64", dims=["frame_minus_1"], shape=[None], quantity="?",
            ),
            NWBDatasetSpec(
                name="camera_sequence_gaps", doc="Frame indices where the camera sequence number jumped (dropped frames).",
                dtype="uint32", dims=["gap"], shape=[None], quantity="?",
            ),
            NWBDatasetSpec(
                name="mean_frame_intensity", doc="Per-frame mean pixel intensity (illumination-drift diagnostic).",
                dtype="float32", dims=["frame"], shape=[None], quantity="?",
            ),
            NWBDatasetSpec(
                name="stimulus_timestamps_us", doc="Stimulus vsync timestamps (microseconds, QPC clock).",
                dtype="int64", dims=["stim_frame"], shape=[None], quantity="?",
            ),
            NWBDatasetSpec(
                name="stimulus_frame_deltas_us", doc="Stimulus inter-frame intervals (microseconds).",
                dtype="int64", dims=["stim_frame_minus_1"], shape=[None], quantity="?",
            ),
            NWBDatasetSpec(
                name="stimulus_dropped_frame_indices", doc="Stimulus frame indices whose interval exceeded the drop threshold.",
                dtype="int64", dims=["drop"], shape=[None], quantity="?",
            ),
        ],
        attributes=[
            NWBAttributeSpec("t0_system_us", "Epoch (system-clock microseconds) defining t=0 for the unified timeline.", "float64", required=False),
            NWBAttributeSpec("start_offset_us", "system - hardware clock offset at acquisition start (microseconds).", "float64", required=False),
            NWBAttributeSpec("end_offset_us", "system - hardware clock offset at acquisition end (microseconds).", "float64", required=False),
            NWBAttributeSpec("drift_us", "Cumulative clock drift over the session = end_offset - start_offset (microseconds).", "float64", required=False),
            NWBAttributeSpec("camera_drops_total", "Total camera sequence-gap count.", "uint32", required=False),
            NWBAttributeSpec("stimulus_drops_total", "Total stimulus drop count.", "uint32", required=False),
            NWBAttributeSpec("stimulus_timing_validatable", "1 if stimulus was on physical hardware scanout (real vsync), else 0.", "uint8", required=False),
            NWBAttributeSpec("display_scanout", "physical or remote_virtual.", "text", required=False),
            NWBAttributeSpec("acquisition_complete", "1 if acquisition finished cleanly, 0 if interrupted.", "uint8", required=False),
        ],
    )

    new_data_types = [retino_map, complex_map, retinotopy, timing]
    import os
    output_dir = os.path.join(os.path.dirname(__file__), "spec")
    os.makedirs(output_dir, exist_ok=True)
    export_spec(ns_builder, new_data_types, output_dir)
    print(f"wrote {NS} spec to {output_dir}")


if __name__ == "__main__":
    build()
