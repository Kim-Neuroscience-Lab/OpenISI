//! MPS complex-op diagnostic.
//!
//! Each candidate operation runs on a small tensor, then forces a GPU sync
//! by downloading one element to CPU. The `print` happens *before* each op
//! and the `OK (...)` after, so if the process hangs we know exactly which
//! op caused it (the last printed line without a matching `OK`).
//!
//! Run with:
//!
//! ```text
//! OPENISI_ANALYSIS_DEVICE=mps  cargo run --example mps_diagnostic
//! OPENISI_ANALYSIS_DEVICE=cpu  cargo run --example mps_diagnostic
//! ```

use std::io::Write;
use std::time::Instant;

use tch::{Device, Kind, Tensor};

/// Force the queued GPU kernels to actually run by pulling one element to CPU.
fn sync(t: &Tensor) {
    let cpu = t.to_device(Device::Cpu);
    // Read a single element to guarantee execution.
    if t.kind() == Kind::ComplexFloat || t.kind() == Kind::ComplexDouble {
        let real = cpu.real();
        let _: f64 = real.flatten(0, -1).double_value(&[0]);
    } else {
        let _: f64 = cpu.flatten(0, -1).double_value(&[0]);
    }
}

fn step<F: FnOnce() -> Tensor>(label: &str, f: F) -> Tensor {
    print!("  {:<40} ", label);
    std::io::stdout().flush().unwrap();
    let t0 = Instant::now();
    let result = f();
    sync(&result);
    println!("OK ({:?})", t0.elapsed());
    result
}

fn main() {
    let dev = isi_analysis::compute::device();
    println!("Device: {}", isi_analysis::compute::backend_info());
    println!();

    let n = 16i64;
    let v_re: Vec<f32> = (0..n * n).map(|i| (i as f32) * 0.1).collect();
    let v_im: Vec<f32> = (0..n * n).map(|i| (i as f32) * -0.05 + 0.3).collect();

    println!("--- Real tensor setup (control) ---");
    let re = step("Tensor::from_slice(re) → device", || {
        Tensor::from_slice(&v_re).reshape([n, n]).to(dev)
    });
    let im = step("Tensor::from_slice(im) → device", || {
        Tensor::from_slice(&v_im).reshape([n, n]).to(dev)
    });
    let _r1 = step("re + im (real arith)", || &re + &im);
    let _r2 = step("re * im (real arith)", || &re * &im);
    let _r3 = step("(re*re + im*im).sqrt() — old amp", || {
        (&re * &re + &im * &im).sqrt()
    });
    let _r4 = step("im.atan2(&re) — old phase", || im.atan2(&re));

    println!();
    println!("--- Native complex tensor construction ---");
    let z = step("Tensor::complex(&re, &im)", || Tensor::complex(&re, &im));
    println!("    z.kind() = {:?}", z.kind());
    println!("    z.size() = {:?}", z.size());

    println!();
    println!("--- Native complex extraction ---");
    let _ = step("z.real()", || z.real());
    let _ = step("z.imag()", || z.imag());
    let _ = step("z.conj()", || z.conj());

    println!();
    println!("--- Native complex pointwise ops ---");
    let _ = step("z.abs() — new amp", || z.abs());
    let _ = step("z.angle() — new phase", || z.angle());

    println!();
    println!("--- Native complex arithmetic ---");
    let _ = step("&z + &z", || &z + &z);
    let _ = step("&z * &z", || &z * &z);
    let _ = step("&z * &z.conj() — combine", || &z * &z.conj());
    let _ = step("(&z * &z.conj()).imag() — gradient", || {
        (&z * &z.conj()).imag()
    });

    println!();
    println!("--- In-place complex add (accumulator path) ---");
    let mut acc = step("acc = Tensor::complex(re, im) clone", || {
        Tensor::complex(&re, &im)
    });
    print!("  {:<40} ", "acc.f_add_(&z) — in-place complex add");
    std::io::stdout().flush().unwrap();
    let t0 = Instant::now();
    let _ = acc.f_add_(&z).expect("f_add_ on complex failed");
    sync(&acc);
    println!("OK ({:?})", t0.elapsed());

    println!();
    println!("--- Real matmul (DFT projection path) ---");
    let kr = step("kr [1, n] f32", || {
        Tensor::from_slice(&v_re[..n as usize])
            .reshape([1, n])
            .to(dev)
    });
    let dff = step("dff [n, H*W] f32 — fake DFF", || {
        let total = (n * n * n) as usize;
        let v: Vec<f32> = (0..total).map(|i| (i as f32) * 0.001).collect();
        Tensor::from_slice(&v).reshape([n, n * n]).to(dev)
    });
    let _ = step("kr.matmul(&dff)", || kr.matmul(&dff));

    println!();
    println!("--- Gaussian smooth path (conv2d) ---");
    let smooth_in = step("smooth_in [H, W] f32", || {
        let v: Vec<f32> = (0..(n * n) as usize).map(|i| (i as f32) * 0.01).collect();
        Tensor::from_slice(&v).reshape([n, n]).to(dev)
    });
    let _ = step("smooth_in.reshape([1,1,H,W])", || {
        smooth_in.reshape([1, 1, n, n])
    });
    let _ = step("reflection_pad2d", || {
        smooth_in
            .reshape([1, 1, n, n])
            .reflection_pad2d([2_i64, 2, 2, 2])
    });
    let kernel = step("gaussian kernel [1,1,1,5]", || {
        let v = vec![0.2_f32; 5];
        Tensor::from_slice(&v).reshape([1, 1, 1, 5]).to(dev)
    });
    let _ = step("conv2d (real)", || {
        let padded = smooth_in
            .reshape([1, 1, n, n])
            .reflection_pad2d([2_i64, 2, 0, 0]);
        padded.conv2d::<Tensor>(&kernel, None, [1, 1], [0, 0], [1, 1], 1)
    });

    println!();
    println!("--- Done ---");
}
