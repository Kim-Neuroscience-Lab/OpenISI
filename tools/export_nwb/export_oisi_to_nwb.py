#!/usr/bin/env python
"""Convert an OpenISI ``.oisi`` (HDF5) file to a reference-valid NWB file.

This is a pure *transformation*: it reads the existing ``.oisi`` (h5py) and writes
a new ``.nwb`` (pynwb) — the native ``.oisi`` format is unchanged, so the
bit-identical analysis-regression gate is untouched. Conformance is guaranteed by
the reference implementation (pynwb) plus ``nwbinspector``, not by hand-asserted
structure.

Mapping (core NWB where it fits, the ``ndx-openisi`` extension where it cannot):
  - session / subject / device / institution  -> NWBFile + Subject + Device + /general
  - raw camera frames (T,H,W)                  -> OnePhotonSeries (acquisition)
  - sweep schedule (start/stop + direction)    -> TimeIntervals "sweeps"
  - anatomical reference image                 -> GrayscaleImage in an Images container
  - visual-area segmentation (labels + signs)  -> PlaneSegmentation (image_mask + sign col)
  - retinotopy result maps + complex maps      -> ndx-openisi RetinotopyMaps (the
                                                  ImagingRetinotopy replacement)
  - multi-clock acquisition timing forensics   -> ndx-openisi TimingForensics

Usage:
    python export_oisi_to_nwb.py INPUT.oisi OUTPUT.nwb [--namespace path/to/ndx-openisi.namespace.yaml]

Assumptions are emitted as warnings (e.g. species defaults to Mus musculus, the
ISI subject).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import uuid
import warnings
from datetime import datetime, timezone

import h5py
import numpy as np
from pynwb import NWBFile, NWBHDF5IO, get_class, load_namespaces
from pynwb.base import Images
from pynwb.device import Device
from pynwb.image import GrayscaleImage
from pynwb.ophys import (
    ImageSegmentation,
    ImagingPlane,
    OnePhotonSeries,
    OpticalChannel,
    PlaneSegmentation,
)
from pynwb.epoch import TimeIntervals
from pynwb.file import Subject

# Result-map datasets that are NOT scalar render maps (handled by segmentation).
_SEG_ONLY = {"area_labels", "area_signs"}


def _warn(msg: str) -> None:
    print(f"[export_nwb] WARNING: {msg}", file=sys.stderr)


def _attr(obj, key, default=None):
    v = obj.attrs.get(key, default)
    if isinstance(v, bytes):
        return v.decode()
    if isinstance(v, np.generic):
        return v.item()
    return v


def _parse_created_at(raw) -> datetime:
    """`.oisi /created_at` is a unix-seconds string in current files (historically
    could be ISO-8601). Return a tz-aware UTC datetime; fall back to epoch with a
    warning so a missing/odd value never blocks a (clearly-flagged) export."""
    if raw is None:
        _warn("no /created_at attribute; session_start_time defaults to unix epoch")
        return datetime(1970, 1, 1, tzinfo=timezone.utc)
    s = raw.decode() if isinstance(raw, bytes) else str(raw)
    try:
        return datetime.fromtimestamp(int(s), tz=timezone.utc)
    except (ValueError, OSError):
        pass
    try:
        dt = datetime.fromisoformat(s.replace("Z", "+00:00"))
        return dt if dt.tzinfo else dt.replace(tzinfo=timezone.utc)
    except ValueError:
        _warn(f"unparseable /created_at {s!r}; session_start_time defaults to unix epoch")
        return datetime(1970, 1, 1, tzinfo=timezone.utc)


def _load_json_attr(f, key):
    raw = _attr(f, key)
    if raw is None:
        return None
    try:
        return json.loads(raw)
    except (json.JSONDecodeError, TypeError):
        return None


def _stimulus_geometry(experiment_params, analysis_params):
    """Stimulus sweep geometry (degrees of visual angle), from the typed
    experiment params if present, else the legacy flat analysis-params schema."""
    geom = {}
    if experiment_params and isinstance(experiment_params.get("stimulus_geometry"), dict):
        sg = experiment_params["stimulus_geometry"]
        geom = {
            "rotation_k": sg.get("rotation_k"),
            "azi_angular_range": sg.get("azi_angular_range"),
            "alt_angular_range": sg.get("alt_angular_range"),
            "offset_azi": sg.get("offset_azi"),
            "offset_alt": sg.get("offset_alt"),
        }
    elif analysis_params:  # legacy flat schema carried these at the root
        geom = {k: analysis_params.get(k) for k in
                ("rotation_k", "azi_angular_range", "alt_angular_range", "offset_azi", "offset_alt")}
    return {k: v for k, v in geom.items() if v is not None}


def _map_meta(ds):
    """OpenISI render metadata for a `/results/<map>` dataset, with safe defaults
    so an older file lacking some attrs still exports (flagged via defaults)."""
    return dict(
        palette=_attr(ds, "palette", "jet"),
        units=_attr(ds, "units", "unitless"),
        display_min=float(_attr(ds, "display_min", 0.0)),
        display_max=float(_attr(ds, "display_max", 0.0)),
        wrap_period=float(_attr(ds, "wrap_period", 0.0)),
        nan_means=_attr(ds, "nan_means", "") or "",
        zero_means=_attr(ds, "zero_means", "") or "",
    )


def convert(oisi_path: str, nwb_path: str, namespace_path: str, meta: dict | None = None) -> None:
    # `meta` is an optional sidecar of submission-required fields the `.oisi`
    # format does not capture (subject age/sex/species, experimenter, experiment
    # description, CCF imaging-plane location). The standard NWB-conversion pattern
    # — the source format stays minimal; DANDI-required metadata rides alongside.
    meta = meta or {}
    subject_meta = meta.get("subject", {})
    load_namespaces(namespace_path)
    RetinotopyMap = get_class("RetinotopyMap", "ndx-openisi")
    ComplexMap = get_class("ComplexMap", "ndx-openisi")
    RetinotopyMaps = get_class("RetinotopyMaps", "ndx-openisi")
    TimingForensics = get_class("TimingForensics", "ndx-openisi")

    with h5py.File(oisi_path, "r") as f:
        created_at = _attr(f, "created_at")
        animal_id = _attr(f, "animal_id")
        notes = _attr(f, "notes")
        source_type = _attr(f, "source_type", "unknown")
        oisi_version = _attr(f, "version", "unknown")
        software_version = _attr(f, "software_version")
        rig_params = _load_json_attr(f, "rig_params")
        experiment_params = _load_json_attr(f, "experiment_params")
        analysis_params_raw = _attr(f, "analysis_params")
        analysis_params = _load_json_attr(f, "analysis_params")

        session_desc = (
            f"OpenISI intrinsic-signal retinotopy ({source_type}). " + (notes or "")
        ).strip()

        nwb = NWBFile(
            session_description=session_desc or "OpenISI intrinsic-signal retinotopy.",
            identifier=str(uuid.uuid4()),
            session_start_time=_parse_created_at(created_at),
            lab=meta.get("lab", "Kim Neuroscience Lab"),
            institution=meta.get("institution", "University of California, Santa Barbara"),
            source_script=os.path.basename(__file__),
            source_script_file_name=os.path.basename(__file__),
            keywords=["intrinsic signal imaging", "retinotopy", "visual cortex", "mouse", "OpenISI"],
            **({"notes": notes} if notes else {}),
            **({"experimenter": meta["experimenter"]} if meta.get("experimenter") else {}),
            **({"experiment_description": meta["experiment_description"]}
               if meta.get("experiment_description") else {}),
        )

        # ── Subject (ISI is a mouse preparation; DANDI requires a Subject with
        #    species, sex, and age/date_of_birth). Fields the `.oisi` lacks come
        #    from the sidecar `--metadata`; absent ones are flagged, not faked. ──
        if not animal_id and "subject_id" not in subject_meta:
            _warn("no /animal_id and no metadata subject_id; defaults to 'unspecified'")
        if "age" not in subject_meta and "date_of_birth" not in subject_meta:
            _warn("subject age/date_of_birth absent (.oisi does not capture it) — "
                  "supply via --metadata for DANDI submission")
        subject_kwargs = dict(
            subject_id=str(subject_meta.get("subject_id", animal_id or "unspecified")),
            species=subject_meta.get("species", "Mus musculus"),
            sex=subject_meta.get("sex", "U"),
            description=subject_meta.get("description", notes or "OpenISI intrinsic-signal-imaging subject."),
        )
        for opt in ("age", "date_of_birth", "genotype", "strain"):
            if subject_meta.get(opt):
                subject_kwargs[opt] = subject_meta[opt]
        nwb.subject = Subject(**subject_kwargs)

        # ── Devices (camera + monitor), from the /hardware snapshot if present ──
        hw = f["hardware"] if "hardware" in f else None
        cam_model = _attr(hw, "camera_model", "OpenISI camera") if hw else "OpenISI camera"
        camera_dev = nwb.create_device(
            name="camera",
            description=str(cam_model),
            manufacturer=str(cam_model).split()[0] if cam_model else "unknown",
        )
        if hw is not None:
            mon = _attr(hw, "monitor_name", "stimulus monitor")
            nwb.create_device(name="stimulus_monitor", description=str(mon))

        # ── Imaging plane (one-photon widefield reflectance) ──
        optical_channel = OpticalChannel(
            name="reflectance",
            description="Widefield intrinsic-signal reflectance (no fluorophore).",
            emission_lambda=float("nan"),
        )
        um_per_pixel = None
        if rig_params and isinstance(rig_params.get("camera"), dict):
            um_per_pixel = rig_params["camera"].get("um_per_pixel")
        imaging_plane = nwb.create_imaging_plane(
            name="cortex_surface",
            optical_channel=optical_channel,
            description="Exposed mouse visual cortex surface, widefield ISI.",
            device=camera_dev,
            excitation_lambda=float("nan"),
            indicator="intrinsic_signal",
            # CCF ontology umbrella for the retinotopically-mapped region (V1 +
            # higher visual areas); override via metadata.imaging_plane_location.
            location=meta.get("imaging_plane_location", "VIS"),
            grid_spacing=[um_per_pixel, um_per_pixel] if um_per_pixel else None,
            grid_spacing_unit="micrometers" if um_per_pixel else "n.a.",
        )

        # ── Raw camera frames -> OnePhotonSeries (acquisition) ──
        if "acquisition/camera/frames" in f:
            frames = f["acquisition/camera/frames"]
            ts = f["acquisition/camera/timestamps_sec"][()] if "acquisition/camera/timestamps_sec" in f else None
            ops_kwargs = dict(
                name="raw_frames",
                imaging_plane=imaging_plane,
                data=frames[()],  # (T,H,W) uint16
                unit="n.a.",
                description="Raw widefield camera frames in acquisition order (no stimulus-state filtering).",
            )
            if ts is not None:
                ops_kwargs["timestamps"] = ts
            else:
                ops_kwargs["rate"] = 0.0
                ops_kwargs["starting_time"] = 0.0
            nwb.add_acquisition(OnePhotonSeries(**ops_kwargs))

        # ── Sweep schedule -> TimeIntervals "sweeps" ──
        # Guard on a NON-EMPTY schedule: a real acquisition can write an empty
        # schedule (no completed sweeps), and an empty TimeIntervals column has no
        # inferable dtype. Skip the table entirely when there are no sweeps.
        if "acquisition/schedule/sweep_start_sec" in f and len(f["acquisition/schedule/sweep_start_sec"]) > 0:
            starts = f["acquisition/schedule/sweep_start_sec"][()]
            stops = f["acquisition/schedule/sweep_end_sec"][()]
            seq_raw = _attr(f["acquisition/schedule"], "sweep_sequence")
            seq = json.loads(seq_raw) if seq_raw else [""] * len(starts)
            sweeps = TimeIntervals(name="sweeps", description="Realized stimulus sweep schedule (one row per sweep).")
            sweeps.add_column(name="direction", description="Sweep direction label (LR/RL/TB/BT/...).")
            for i in range(len(starts)):
                sweeps.add_row(
                    start_time=float(starts[i]),
                    stop_time=float(stops[i]),
                    direction=str(seq[i]) if i < len(seq) else "",
                )
            nwb.add_time_intervals(sweeps)

        # ── Anatomical reference image -> Images container ──
        images = []
        if "anatomical" in f and isinstance(f["anatomical"], h5py.Dataset):
            images.append(GrayscaleImage(name="anatomical", data=f["anatomical"][()],
                                         description="Anatomical reference image of the cortical surface."))
        if "anatomical/cortex_roi" in f:
            images.append(GrayscaleImage(name="cortex_roi", data=f["anatomical/cortex_roi"][()],
                                         description="User-drawn cortex ROI mask (0/1)."))
        if images:
            nwb.add_acquisition(Images(name="anatomical_images", images=images,
                                       description="Anatomical reference imagery."))

        ophys_mod = nwb.create_processing_module(
            name="ophys", description="OpenISI segmentation + retinotopy results."
        )

        # ── Visual-area segmentation -> PlaneSegmentation ──
        results = f["results"] if "results" in f else None
        if results is not None and "area_labels" in results:
            labels = results["area_labels"][()]
            signs = results["area_signs"][()] if "area_signs" in results else None
            img_seg = ImageSegmentation(name="image_segmentation")
            ps = PlaneSegmentation(
                name="visual_areas",
                description="Segmented retinotopic visual areas (one ROI per labelled area).",
                imaging_plane=imaging_plane,
            )
            uniq = [int(v) for v in np.unique(labels) if int(v) != 0]
            if signs is not None:
                ps.add_column(name="sign", description="Visual field sign of the area (+1 / -1).")
            for k in uniq:
                row = {"image_mask": (labels == k)}
                if signs is not None:
                    row["sign"] = int(signs[k - 1]) if 0 < k <= len(signs) else 0
                ps.add_roi(**row)
            img_seg.add_plane_segmentation(ps)
            ophys_mod.add(img_seg)

        # ── Retinotopy result maps + complex maps -> ndx RetinotopyMaps ──
        retino_maps = []
        if results is not None:
            for name in sorted(results.keys()):
                ds = results[name]
                if name in _SEG_ONLY or not isinstance(ds, h5py.Dataset):
                    continue
                if ds.ndim != 2:
                    continue
                meta = _map_meta(ds)
                desc = f"OpenISI retinotopy map '{name}' ({meta['units']})."
                retino_maps.append(RetinotopyMap(name=name, data=ds[()], description=desc, **meta))

        complex_maps = []
        if "complex_maps" in f:
            for name in sorted(f["complex_maps"].keys()):
                ds = f["complex_maps"][name]
                if isinstance(ds, h5py.Dataset) and ds.ndim == 3 and ds.shape[-1] == 2:
                    complex_maps.append(ComplexMap(name=name, data=ds[()]))

        if retino_maps or complex_maps:
            geom = _stimulus_geometry(experiment_params, analysis_params)
            rmaps = RetinotopyMaps(
                name="retinotopy",
                retinotopy_maps=retino_maps or None,
                complex_maps=complex_maps or None,
                analysis_params=analysis_params_raw if analysis_params_raw else None,
                software_version=str(software_version) if software_version else None,
                **{k: (int(v) if k == "rotation_k" else float(v)) for k, v in geom.items()},
            )
            ophys_mod.add(rmaps)

        # ── Multi-clock timing forensics -> ndx TimingForensics ──
        tf = _build_timing_forensics(f, TimingForensics)
        if tf is not None:
            nwb.add_acquisition(tf)

        # ── General provenance ──
        prov = {"oisi_version": oisi_version, "source_type": source_type}
        if software_version:
            prov["software_version"] = software_version
        if rig_params:
            prov["rig_params"] = rig_params
        if experiment_params:
            prov["experiment_params"] = experiment_params
        nwb.general_source_script = json.dumps(prov)

    with NWBHDF5IO(nwb_path, "w") as io:
        io.write(nwb)
    print(f"[export_nwb] wrote {nwb_path}")


def _ds(f, path):
    return f[path][()] if path in f else None


def _build_timing_forensics(f, TimingForensics):
    if "acquisition" not in f:
        return None
    kw = {}
    paths = {
        "camera_hardware_timestamps_us": "acquisition/camera/hardware_timestamps_us",
        "camera_system_timestamps_us": "acquisition/camera/system_timestamps_us",
        "camera_sequence_numbers": "acquisition/camera/sequence_numbers",
        "camera_frame_deltas_us": "acquisition/quality/camera_frame_deltas_us",
        "camera_sequence_gaps": "acquisition/quality/camera_sequence_gaps",
        "mean_frame_intensity": "acquisition/quality/mean_frame_intensity",
        "stimulus_timestamps_us": "acquisition/stimulus/timestamps_us",
        "stimulus_frame_deltas_us": "acquisition/stimulus/frame_deltas_us",
        "stimulus_dropped_frame_indices": "acquisition/stimulus/dropped_frame_indices",
    }
    for field, path in paths.items():
        d = _ds(f, path)
        if d is not None:
            kw[field] = d
    cs = f["acquisition/clock_sync"] if "acquisition/clock_sync" in f else None
    if cs is not None:
        for a in ("t0_system_us", "start_offset_us", "end_offset_us", "drift_us"):
            v = _attr(cs, a)
            if v is not None:
                kw[a] = float(v)
    q = f["acquisition/quality"] if "acquisition/quality" in f else None
    if q is not None:
        # dtypes must match the ndx spec (uint32 / uint8), not Python int.
        for a, cast in (("camera_drops_total", np.uint32), ("stimulus_drops_total", np.uint32),
                        ("stimulus_timing_validatable", np.uint8), ("acquisition_complete", np.uint8)):
            v = _attr(q, a)
            if v is not None:
                kw[a] = cast(v)
        ds = _attr(q, "display_scanout")
        if ds is not None:
            kw["display_scanout"] = str(ds)
    if not kw:
        return None
    return TimingForensics(name="timing_forensics", **kw)


def main():
    ap = argparse.ArgumentParser(description="Convert an OpenISI .oisi file to NWB.")
    ap.add_argument("input", help="Path to the .oisi (HDF5) file.")
    ap.add_argument("output", help="Path to write the .nwb file.")
    ap.add_argument(
        "--namespace",
        default=os.path.join(os.path.dirname(__file__), "..", "..", "ndx-openisi", "spec", "ndx-openisi.namespace.yaml"),
        help="Path to ndx-openisi.namespace.yaml.",
    )
    ap.add_argument(
        "--metadata",
        default=None,
        help="Optional JSON sidecar of submission-required fields the .oisi does not "
        "capture (subject.age/sex/species/date_of_birth, experimenter, "
        "experiment_description, institution, lab, imaging_plane_location).",
    )
    args = ap.parse_args()
    meta = None
    if args.metadata:
        with open(args.metadata, encoding="utf-8") as fh:
            meta = json.load(fh)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")  # silence pynwb's chatty cast warnings; nwbinspector is the gate
        convert(args.input, args.output, os.path.abspath(args.namespace), meta)


if __name__ == "__main__":
    main()
