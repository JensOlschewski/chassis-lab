import { setupBrowse } from "./browse";
import { Section, renderSection } from "./sections";
import { describe, errorLine, run } from "./worker";

/** Parses the `--json` stdout: one `{"section": …}` line per section. */
export function parseSections(stdout: string): Section[] {
  return stdout
    .split("\n")
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line).section as Section | undefined)
    .filter((section): section is Section => section !== undefined);
}

export function setupAnalyze() {
  const form = document.querySelector<HTMLFormElement>("#analyze-form")!;
  const status = document.querySelector<HTMLElement>("#analyze-status")!;
  const sections = document.querySelector<HTMLElement>("#analyze-sections")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

  const show = (message: string, ok: boolean) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  setupBrowse(form);

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    sections.replaceChildren();
    show("Analyzing…", true);
    try {
      const data = new FormData(form);
      const text = (name: string) => String(data.get(name) ?? "").trim();
      const outcome = await run("run_analyze", {
        config: text("config"),
        input: text("input"),
        report: text("report") || null,
        force: data.has("force"),
      });
      const { ok, message } = describe(outcome);
      if (!ok) {
        show(errorLine(outcome.stderr) ?? message, false);
        return;
      }
      sections.replaceChildren(...parseSections(outcome.stdout).map(renderSection));
      show(message, true);
    } catch (error) {
      show(`Failed to run clab: ${error}`, false);
    } finally {
      submit.disabled = false;
    }
  });
}
