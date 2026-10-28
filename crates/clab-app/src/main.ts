import { invoke } from "@tauri-apps/api/core";

interface AnalyzeOutcome {
  exit_code: number | null;
  stdout: string;
  stderr: string;
}

const form = document.querySelector<HTMLFormElement>("#analyze-form")!;
const statusElement = document.querySelector<HTMLParagraphElement>("#status")!;
const outputElement = document.querySelector<HTMLPreElement>("#output")!;
const field = (id: string) => document.querySelector<HTMLInputElement>(`#${id}`)!.value;

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  statusElement.textContent = "Running…";
  outputElement.textContent = "";
  try {
    const outcome = await invoke<AnalyzeOutcome>("run_analyze", {
      config: field("config"),
      input: field("input"),
      report: field("report"),
    });
    statusElement.textContent = `clab exit code: ${outcome.exit_code ?? "terminated by signal"}`;
    outputElement.textContent = outcome.stdout + outcome.stderr;
  } catch (error) {
    statusElement.textContent = "Failed to run analysis.";
    outputElement.textContent = String(error);
  }
});
