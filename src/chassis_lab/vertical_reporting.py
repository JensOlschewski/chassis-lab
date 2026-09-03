"""Render deterministic vertical trajectory reports and plots."""

import csv
import io
import json
from dataclasses import dataclass
from pathlib import Path

from chassis_lab import __version__
from chassis_lab.config import CaseConfig
from chassis_lab.runs import sha256_file
from chassis_lab.vertical import VerticalResult

REPORT_SCHEMA_VERSION = 1
CSV_FIELDS = (
    "case",
    "variant",
    "loadcase",
    "setup_code",
    "corner",
    "axis",
    "measurement_baseline_mm",
    "simulation_baseline_mm",
    "baseline_error_mm",
    "rmse_mm",
    "maximum_error_mm",
    "config_sha256",
    "signal_map_sha256",
    "measurement_sha256",
    "simulation_sha256",
)


class VerticalReportError(RuntimeError):
    """Raised when a vertical report cannot be built or written."""


@dataclass(frozen=True)
class VerticalReport:
    config: CaseConfig
    result: VerticalResult
    config_sha256: str
    signal_map_sha256: str | None
    measurement_sha256: str
    simulation_sha256: str

    def as_dict(self) -> dict[str, object]:
        vertical = self.config.vertical
        assert vertical is not None
        inputs = {
            "config": {
                "path": str(self.config.path),
                "sha256": self.config_sha256,
            },
            "measurement_acd": {
                "path": str(vertical.measurement_acd),
                "sha256": self.measurement_sha256,
            },
            "simulation_hdf5": {
                "path": str(vertical.simulation_hdf5),
                "sha256": self.simulation_sha256,
            },
        }
        if vertical.signal_map is not None:
            inputs["signal_map"] = {
                "path": str(vertical.signal_map),
                "sha256": self.signal_map_sha256,
            }
        return {
            "schema_version": REPORT_SCHEMA_VERSION,
            **self.result.as_dict(),
            "inputs": inputs,
            "software": {"chassis_lab": __version__},
        }


def build_vertical_report(
    config: CaseConfig, result: VerticalResult
) -> VerticalReport:
    """Build report with provenance for both trajectory sources."""
    if config.vertical is None:
        raise VerticalReportError(
            f"case '{config.name}' has no [vertical] configuration"
        )
    if result.case_name != config.name:
        raise VerticalReportError(
            "vertical result does not match configuration"
        )
    return VerticalReport(
        config=config,
        result=result,
        config_sha256=sha256_file(config.path),
        signal_map_sha256=(
            sha256_file(config.vertical.signal_map)
            if config.vertical.signal_map is not None
            else None
        ),
        measurement_sha256=sha256_file(config.vertical.measurement_acd),
        simulation_sha256=sha256_file(config.vertical.simulation_hdf5),
    )


def render_vertical_console(report: VerticalReport) -> str:
    """Render stable human-readable trajectory metrics."""
    lines = [
        "Vertical trajectory comparison",
        f"case: {report.result.case_name} "
        f"({report.result.variant}, {report.result.loadcase}, "
        f"{report.result.setup_code})",
        "",
        f"reporting {len(report.result.comparisons)} trajectories",
    ]
    current_corner = None
    for comparison in report.result.comparisons:
        if comparison.corner != current_corner:
            current_corner = comparison.corner
            lines.extend(("", f"vertical::{current_corner.value}"))
        lines.append(
            f"  {comparison.axis} ... "
            f"baseline diff {_number(comparison.baseline_error_mm)} mm; "
            f"RMSE {_number(comparison.rmse_mm)} mm; "
            f"max {_number(comparison.maximum_error_mm)} mm"
        )
    lines.extend(
        ("", "validation result: informational (no tolerances configured)")
    )
    return "\n".join(lines) + "\n"


def render_vertical_json(report: VerticalReport) -> str:
    return json.dumps(report.as_dict(), indent=2, sort_keys=True) + "\n"


def render_vertical_csv(report: VerticalReport) -> str:
    stream = io.StringIO(newline="")
    writer = csv.DictWriter(stream, fieldnames=CSV_FIELDS, lineterminator="\n")
    writer.writeheader()
    for comparison in report.result.comparisons:
        writer.writerow(
            {
                "case": report.result.case_name,
                "variant": report.result.variant,
                "loadcase": report.result.loadcase,
                "setup_code": report.result.setup_code,
                **comparison.as_dict(),
                "config_sha256": report.config_sha256,
                "signal_map_sha256": report.signal_map_sha256 or "",
                "measurement_sha256": report.measurement_sha256,
                "simulation_sha256": report.simulation_sha256,
            }
        )
    return stream.getvalue()


def write_vertical_outputs(
    report: VerticalReport,
    *,
    json_path: Path | str | None = None,
    csv_path: Path | str | None = None,
    plot_path: Path | str | None = None,
    force: bool = False,
) -> None:
    """Write requested outputs atomically without silent replacement."""
    text_outputs: list[tuple[Path, str]] = []
    if json_path is not None:
        text_outputs.append((Path(json_path), render_vertical_json(report)))
    if csv_path is not None:
        text_outputs.append((Path(csv_path), render_vertical_csv(report)))
    plot = Path(plot_path).expanduser().resolve() if plot_path else None
    if plot is not None and plot.suffix.casefold() != ".svg":
        raise VerticalReportError(
            "vertical plot output must use .svg extension"
        )
    resolved_text = [
        (path.expanduser().resolve(), content) for path, content in text_outputs
    ]
    paths = [path for path, _ in resolved_text]
    if plot is not None:
        paths.append(plot)
    if len(paths) != len(set(paths)):
        raise VerticalReportError("vertical output paths must be different")
    existing = [path for path in paths if path.exists()]
    if existing and not force:
        raise VerticalReportError(
            f"report output already exists: {existing[0]}"
        )
    staged: list[tuple[Path, Path]] = []
    try:
        for path, content in resolved_text:
            path.parent.mkdir(parents=True, exist_ok=True)
            temporary = path.with_name(f".{path.name}.tmp")
            temporary.write_text(content, encoding="utf-8", newline="")
            staged.append((temporary, path))
        if plot is not None:
            plot.parent.mkdir(parents=True, exist_ok=True)
            temporary = plot.with_name(f".{plot.name}.tmp")
            _render_plot(report, temporary)
            staged.append((temporary, plot))
        for temporary, path in staged:
            temporary.replace(path)
    except Exception:
        for temporary, _ in staged:
            temporary.unlink(missing_ok=True)
        raise


def _render_plot(report: VerticalReport, path: Path) -> None:
    import matplotlib

    matplotlib.use("Agg")
    from matplotlib import pyplot as plt

    settings = {
        "font.family": "DejaVu Sans",
        "svg.hashsalt": "chassis-lab-vertical",
    }
    try:
        with plt.rc_context(settings):
            figure, axes = plt.subplots(4, 3, figsize=(11, 12), squeeze=False)
            for index, comparison in enumerate(report.result.comparisons):
                row, column = divmod(index, 3)
                axis = axes[row][column]
                axis.plot(
                    comparison.travel_mm,
                    comparison.measurement_mm,
                    color="#167d91",
                    linewidth=1.8,
                    label="Lommel",
                )
                axis.plot(
                    comparison.travel_mm,
                    comparison.simulation_mm,
                    color="#d2603a",
                    linewidth=1.5,
                    linestyle="--",
                    label="Adams",
                )
                axis.axhline(0.0, color="#777777", linewidth=0.6)
                axis.axvline(0.0, color="#777777", linewidth=0.6)
                axis.grid(color="#dddddd", linewidth=0.5)
                axis.set_title(
                    f"{comparison.corner.value.replace('_', ' ')} "
                    f"Δ{comparison.axis.upper()}"
                )
                axis.set_xlabel("test travel [mm]")
                axis.set_ylabel(f"Δ{comparison.axis.upper()} [mm]")
                if index == 0:
                    axis.legend(frameon=False)
            figure.suptitle(
                f"{report.result.variant} {report.result.loadcase} vertical "
                f"wheel-center trajectories ({report.result.setup_code})"
            )
            figure.tight_layout()
            figure.savefig(
                path,
                format="svg",
                metadata={"Date": None, "Creator": "ChassisLab"},
            )
            plt.close(figure)
    except Exception:
        path.unlink(missing_ok=True)
        raise


def _number(value: float) -> str:
    return f"{value:.8g}"
