use std::{
    env,
    ffi::{CStr, CString},
    fs,
    mem::MaybeUninit,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use pyo3::{
    ffi::{self, c_str},
    prelude::*,
    types::PyModule,
};

const ANALYSIS: &std::ffi::CStr = c_str!(
    r#"
def analyze(values):
    import io
    import sys

    import matplotlib
    matplotlib.use("Agg")
    matplotlib.rcParams["svg.hashsalt"] = "chassis-lab"
    import matplotlib.pyplot as plt
    import numpy as np

    signal = np.asarray(values, dtype=np.float64)
    mean_square = float(np.mean(np.square(signal)))

    figure, axes = plt.subplots(figsize=(4, 3))
    axes.plot(signal)
    axes.set(xlabel="sample", ylabel="value", title="Embedded Python smoke test")
    figure.tight_layout()
    output = io.StringIO()
    figure.savefig(output, format="svg", metadata={"Date": None})
    plt.close(figure)
    return (
        mean_square,
        output.getvalue().encode("utf-8"),
        bool(sys.flags.isolated),
        np.__file__,
        matplotlib.__file__,
    )
"#
);

fn main() -> Result<()> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("embed-smoke.svg"));
    let values = vec![1.0_f64, 2.0, 3.0, 4.0];
    let runtime = initialize_python()?;

    let (mean_square, svg, isolated, numpy_path, matplotlib_path) =
        Python::attach(|py| -> PyResult<(f64, Vec<u8>, bool, String, String)> {
            let module = PyModule::from_code(
                py,
                ANALYSIS,
                c_str!("embedded_analysis.py"),
                c_str!("embedded_analysis"),
            )?;
            module.getattr("analyze")?.call1((values,))?.extract()
        })
        .context("embedded Python analysis failed")?;

    ensure!(
        (mean_square - 7.5).abs() < f64::EPSILON,
        "unexpected NumPy result: {mean_square}"
    );
    ensure!(
        svg.windows(4).any(|window| window == b"<svg"),
        "Matplotlib did not return SVG data"
    );
    if let Some(runtime) = &runtime {
        ensure!(isolated, "bundled Python did not start in isolated mode");
        for (name, module_path) in [
            ("NumPy", numpy_path.as_str()),
            ("Matplotlib", matplotlib_path.as_str()),
        ] {
            ensure!(
                Path::new(module_path).starts_with(runtime),
                "{name} loaded outside bundle: {module_path}"
            );
        }
    }
    fs::write(&output, svg).with_context(|| format!("failed to write '{}'", output.display()))?;

    println!("mean square: {mean_square}");
    println!(
        "python runtime: {}",
        runtime
            .as_deref()
            .map_or_else(|| "development".into(), |path| path.display().to_string())
    );
    println!("numpy: {numpy_path}");
    println!("matplotlib: {matplotlib_path}");
    println!("wrote: {}", output.display());
    Ok(())
}

fn initialize_python() -> Result<Option<PathBuf>> {
    let executable = env::current_exe().context("failed to locate smoke executable")?;
    let runtime = executable
        .parent()
        .and_then(Path::parent)
        .filter(|root| root.join("lib/python3.12").is_dir())
        .map(|root| {
            if cfg!(windows) {
                root.join("lib/python3.12")
            } else {
                root.to_path_buf()
            }
        });

    if let Some(runtime) = &runtime {
        initialize_isolated(runtime, &executable)?;
    } else {
        Python::initialize();
    }
    Ok(runtime)
}

fn initialize_isolated(runtime: &Path, executable: &Path) -> Result<()> {
    let runtime = path_c_string(runtime)?;
    let executable = path_c_string(executable)?;
    let mut config = MaybeUninit::<ffi::PyConfig>::uninit();

    unsafe {
        ffi::PyConfig_InitIsolatedConfig(config.as_mut_ptr());
        let config = config.assume_init_mut();
        config.site_import = 1;
        config.write_bytecode = 0;

        let result = (|| {
            let config_ptr = config as *mut ffi::PyConfig;
            check_status(
                ffi::PyConfig_SetBytesString(
                    config_ptr,
                    &raw mut (*config_ptr).home,
                    runtime.as_ptr(),
                ),
                "set Python home",
            )?;
            check_status(
                ffi::PyConfig_SetBytesString(
                    config_ptr,
                    &raw mut (*config_ptr).program_name,
                    executable.as_ptr(),
                ),
                "set Python program name",
            )?;
            check_status(
                ffi::PyConfig_SetBytesString(
                    config_ptr,
                    &raw mut (*config_ptr).executable,
                    executable.as_ptr(),
                ),
                "set Python executable",
            )?;
            check_status(
                ffi::Py_InitializeFromConfig(config_ptr),
                "initialize isolated Python",
            )
        })();
        ffi::PyConfig_Clear(config);
        result
    }
}

fn path_c_string(path: &Path) -> Result<CString> {
    CString::new(path.to_string_lossy().as_bytes())
        .with_context(|| format!("path contains a null byte: '{}'", path.display()))
}

fn check_status(status: ffi::PyStatus, operation: &str) -> Result<()> {
    if unsafe { ffi::PyStatus_Exception(status) } == 0 {
        return Ok(());
    }
    let detail = if status.err_msg.is_null() {
        format!("Python requested exit code {}", status.exitcode)
    } else {
        unsafe { CStr::from_ptr(status.err_msg) }
            .to_string_lossy()
            .into_owned()
    };
    bail!("{operation} failed: {detail}")
}
