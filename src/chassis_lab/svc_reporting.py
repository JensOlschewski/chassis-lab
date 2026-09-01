"""Render deterministic SVC validation reports."""

import csv
import io
import json
from dataclasses import dataclass
from pathlib import Path

from chassis_lab import __version__
from chassis_lab.config import CaseConfig
from chassis_lab.runs import sha256_file
from chassis_lab.svc_reference import load_svc_reference
from chassis_lab.svc_validation import MetricComparison, SvcValidationResult

REPORT_SCHEMA_VERSION = 1
CSV_FIELDS = (
    "case",
    "variant",
    "loadcase",
    "metric",
    "unit",
    "actual",
    "reference",
    "difference",
    "absolute_tolerance",
    "relative_percent",
    "allowed_difference",
    "passed",
    "config_sha256",
    "simulation_sha256",
    "reference_sha256",
)
_UNIT_SUFFIXES = (
    ("_kg_mm2", "kg*mm^2"),
    ("_percent", "%"),
    ("_kg", "kg"),
    ("_mm", "mm"),
)


class SvcReportError(RuntimeError):
    """Raised when an SVC report cannot be built or written."""


@dataclass(frozen=True)
class FileProvenance:
    path: Path
    sha256: str

    def as_dict(self) -> dict[str, str]:
        return {"path": str(self.path), "sha256": self.sha256}


@dataclass(frozen=True)
class SvcReportMetric:
    comparison: MetricComparison
    unit: str

    def as_dict(self) -> dict[str, object]:
        return {"unit": self.unit, **self.comparison.as_dict()}


@dataclass(frozen=True)
class SvcReport:
    config: CaseConfig
    validation: SvcValidationResult
    sample_index: int
    config_input: FileProvenance
    simulation_input: FileProvenance
    reference_input: FileProvenance
    reference_sources: tuple[dict[str, str | None], ...]
    metrics: tuple[SvcReportMetric, ...]

    @property
    def passed(self) -> bool:
        return self.validation.passed

    @property
    def failed(self) -> tuple[SvcReportMetric, ...]:
        return tuple(
            metric for metric in self.metrics if not metric.comparison.passed
        )

    def as_dict(self) -> dict[str, object]:
        return {
            "schema_version": REPORT_SCHEMA_VERSION,
            "case": {
                "name": self.config.name,
                "variant": self.config.variant,
                "loadcase": self.config.loadcase,
            },
            "sample_index": self.sample_index,
            "passed": self.passed,
            "failed_count": len(self.failed),
            "inputs": {
                "config": self.config_input.as_dict(),
                "simulation_hdf5": self.simulation_input.as_dict(),
                "reference": self.reference_input.as_dict(),
            },
            "reference_sources": list(self.reference_sources),
            "software": {"chassis_lab": __version__},
            "metrics": [metric.as_dict() for metric in self.metrics],
        }


def build_svc_report(
    config: CaseConfig,
    validation: SvcValidationResult,
    *,
    sample_index: int = -1,
) -> SvcReport:
    """Build one report with input checksums and reference provenance."""
    if validation.case_name != config.name:
        raise SvcReportError(
            "validation result case does not match configuration: "
            f"{validation.case_name!r} != {config.name!r}"
        )
    if config.validation is None:
        raise SvcReportError(
            f"case '{config.name}' has no [validation] configuration"
        )
    reference = load_svc_reference(config.validation.reference)
    sources = tuple(
        {
            "file": source.file,
            "sha256": source.sha256,
            "test_date": source.test_date,
            "result": source.result,
        }
        for source in reference.sources
    )
    metrics = tuple(
        SvcReportMetric(comparison=metric, unit=metric_unit(metric.name))
        for metric in validation.metrics
    )
    return SvcReport(
        config=config,
        validation=validation,
        sample_index=sample_index,
        config_input=_provenance(config.path),
        simulation_input=_provenance(config.simulation_hdf5),
        reference_input=_provenance(reference.path),
        reference_sources=sources,
        metrics=metrics,
    )


def metric_unit(name: str) -> str:
    """Return explicit display unit encoded by a canonical metric name."""
    for suffix, unit in _UNIT_SUFFIXES:
        if name.endswith(suffix):
            return unit
    raise SvcReportError(f"no report unit defined for SVC metric: {name}")


def render_svc_console(report: SvcReport) -> str:
    """Render concise human output, listing details only for failures."""
    passed_count = len(report.metrics) - len(report.failed)
    lines = [
        f"SVC validation: {'PASS' if report.passed else 'FAIL'}",
        (
            f"Case: {report.config.name} "
            f"({report.config.variant}, {report.config.loadcase})"
        ),
        f"Simulation: {report.simulation_input.path}",
        f"Reference: {report.reference_input.path}",
        (
            f"Metrics: {passed_count} passed, {len(report.failed)} failed, "
            f"{len(report.metrics)} total"
        ),
    ]
    if report.failed:
        lines.extend(
            [
                "",
                "Failed metrics:",
                "METRIC                               ACTUAL      REFERENCE  "
                "DIFFERENCE     ALLOWED  UNIT",
            ]
        )
        for report_metric in report.failed:
            metric = report_metric.comparison
            lines.append(
                f"{metric.name:<34} "
                f"{_number(metric.actual):>11} "
                f"{_number(metric.reference):>11} "
                f"{_number(metric.difference):>11} "
                f"{_number(metric.allowed_difference):>11}  "
                f"{report_metric.unit}"
            )
    return "\n".join(lines) + "\n"


def render_svc_json(report: SvcReport) -> str:
    """Render stable, newline-terminated JSON."""
    return json.dumps(report.as_dict(), indent=2, sort_keys=True) + "\n"


def render_svc_csv(report: SvcReport) -> str:
    """Render stable CSV with standalone provenance on every metric row."""
    stream = io.StringIO(newline="")
    writer = csv.DictWriter(stream, fieldnames=CSV_FIELDS, lineterminator="\n")
    writer.writeheader()
    for report_metric in report.metrics:
        metric = report_metric.comparison
        writer.writerow(
            {
                "case": report.config.name,
                "variant": report.config.variant,
                "loadcase": report.config.loadcase,
                "metric": metric.name,
                "unit": report_metric.unit,
                "actual": metric.actual,
                "reference": metric.reference,
                "difference": metric.difference,
                "absolute_tolerance": metric.absolute_tolerance,
                "relative_percent": metric.relative_percent,
                "allowed_difference": metric.allowed_difference,
                "passed": str(metric.passed).lower(),
                "config_sha256": report.config_input.sha256,
                "simulation_sha256": report.simulation_input.sha256,
                "reference_sha256": report.reference_input.sha256,
            }
        )
    return stream.getvalue()


def write_svc_outputs(
    report: SvcReport,
    *,
    json_path: Path | str | None = None,
    csv_path: Path | str | None = None,
    force: bool = False,
) -> None:
    """Write requested report formats without silently replacing outputs."""
    outputs: list[tuple[Path, str]] = []
    if json_path is not None:
        outputs.append((Path(json_path).expanduser(), render_svc_json(report)))
    if csv_path is not None:
        outputs.append((Path(csv_path).expanduser(), render_svc_csv(report)))
    resolved_outputs = [(path.resolve(), content) for path, content in outputs]
    paths = [path for path, _ in resolved_outputs]
    if len(set(paths)) != len(paths):
        raise SvcReportError("JSON and CSV output paths must be different")
    existing = [path for path in paths if path.exists()]
    if existing and not force:
        raise SvcReportError(f"report output already exists: {existing[0]}")
    for path, content in resolved_outputs:
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_name(f".{path.name}.tmp")
        try:
            temporary.write_text(content, encoding="utf-8", newline="")
            temporary.replace(path)
        except OSError:
            temporary.unlink(missing_ok=True)
            raise


def _provenance(path: Path) -> FileProvenance:
    resolved = path.resolve()
    return FileProvenance(path=resolved, sha256=sha256_file(resolved))


def _number(value: float) -> str:
    return f"{value:.8g}"
