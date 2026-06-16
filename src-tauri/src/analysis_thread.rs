//! Analysis worker thread — owns the long-running analysis pipeline so
//! IPC commands return immediately and the WebView stays responsive.
//!
//! ## Why a dedicated thread
//!
//! `isi_analysis::analyze` is CPU-bound, runs for seconds to minutes per
//! invocation, and serialises through the config lock during snapshot
//! creation. Running it inside the `run_analysis` Tauri command (the
//! previous shape) tied each call to a Tauri runtime worker for its full
//! duration: rapid param edits in the UI piled up multiple synchronous
//! invocations behind each other, no per-call cancellation, and the
//! UI awaited results that wouldn't arrive for minutes.
//!
//! The fix mirrors the camera + stimulus threads already in this crate.
//! A single long-lived worker owns the analysis pipeline. Commands
//! arrive via a crossbeam channel; a new `Run` request preempts the
//! current one through an `Arc<AtomicBool>` cancel flag (the analysis
//! pipeline already polls it at stage boundaries), waits for the
//! preempted run to unwind, then starts the new one. Status lands at
//! the UI as `analysis-*` Tauri events, not as command return values.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};

use crate::messages::{AnalysisCmd, AnalysisEvt, AnalysisRequest};

pub fn run(cmd_rx: Receiver<AnalysisCmd>, evt_tx: Sender<AnalysisEvt>) {
    // The currently in-flight analysis, if any: cancel flag (we set it
    // when preempting) + the inner worker's join handle (we wait for it
    // to drop file handles before starting the next).
    let mut current: Option<(Arc<AtomicBool>, JoinHandle<()>)> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            AnalysisCmd::Run(req) => {
                // Preempt any in-flight analysis. The analyze() pipeline
                // checks `cancel` at stage boundaries and returns
                // `AnalysisError::Cancelled` promptly; the inner thread
                // emits `Cancelled` then exits.
                if let Some((cancel, handle)) = current.take() {
                    cancel.store(true, Ordering::Relaxed);
                    let _ = handle.join();
                }
                current = Some(spawn_inner(*req, evt_tx.clone()));
            }
            AnalysisCmd::Shutdown => {
                if let Some((cancel, handle)) = current.take() {
                    cancel.store(true, Ordering::Relaxed);
                    let _ = handle.join();
                }
                break;
            }
        }
    }
}

fn spawn_inner(
    req: AnalysisRequest,
    evt_tx: Sender<AnalysisEvt>,
) -> (Arc<AtomicBool>, JoinHandle<()>) {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = cancel.clone();
    let path = req.path.clone();
    let evt_tx_for_spawn = evt_tx.clone();

    let handle = std::thread::Builder::new()
        .name("analysis".into())
        .spawn(move || {
            let evt_tx = evt_tx_for_spawn;
            let _ = evt_tx.send(AnalysisEvt::Started {
                path: req.path.clone(),
            });

            let progress = isi_analysis::SilentProgress;

            let result = isi_analysis::analyze(&req.path, &req.params, &progress, &cancel_clone);

            match result {
                Ok(()) => {
                    // Stamp the config tree into /analysis_params for provenance.
                    if let Err(e) =
                        isi_analysis::io::write_analysis_params_attr(&req.path, &req.params_tree)
                    {
                        let _ = evt_tx.send(AnalysisEvt::Failed {
                            path: req.path,
                            error: format!("write /analysis_params: {e}"),
                        });
                        return;
                    }
                    let _ = evt_tx.send(AnalysisEvt::Complete {
                        path: req.path,
                        message: "Analysis complete".into(),
                    });
                }
                Err(isi_analysis::AnalysisError::Cancelled) => {
                    let _ = evt_tx.send(AnalysisEvt::Cancelled { path: req.path });
                }
                Err(e) => {
                    let _ = evt_tx.send(AnalysisEvt::Failed {
                        path: req.path,
                        error: e.to_string(),
                    });
                }
            }
        })
        .unwrap_or_else(|e| {
            // Spawn failed — synthesise a Failed event so the UI doesn't
            // wait forever for completion.
            let _ = evt_tx.send(AnalysisEvt::Failed {
                path,
                error: format!("failed to spawn analysis worker: {e}"),
            });
            // Return a placeholder handle that immediately joins. We can't
            // construct a JoinHandle without a thread, so panic — but
            // spawn failure is essentially "the OS refuses to give us a
            // thread", which is unrecoverable anyway.
            panic!("std::thread::Builder::spawn failed: {e}");
        });

    (cancel, handle)
}
