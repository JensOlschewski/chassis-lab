import { setupBrowse } from "./browse";
import { describe, run } from "./worker";

interface ConvertRequest {
  input: string;
  domain: string;
  test: string | null;
  output: string | null;
  force: boolean;
}

/** Reads the form into a request; empty optional fields become null. */
export function readRequest(form: HTMLFormElement): ConvertRequest {
  const data = new FormData(form);
  const text = (name: string) => String(data.get(name) ?? "").trim();
  const domain = text("domain");
  return {
    input: text("input"),
    domain,
    // Only knc has tests; clab rejects a test for svc.
    test: domain === "knc" ? text("test") : null,
    output: text("output") || null,
    force: data.has("force"),
  };
}

export function setupConvert(): void {
  const form = document.querySelector<HTMLFormElement>("#convert-form")!;
  const testField = document.querySelector<HTMLElement>("#test-field")!;
  const status = document.querySelector<HTMLElement>("#convert-status")!;
  const output = document.querySelector<HTMLElement>("#convert-output")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

  const show = (message: string, ok: boolean, details = "") => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
    output.textContent = details;
  };

  form.addEventListener("change", () => {
    testField.hidden = readRequest(form).domain !== "knc";
  });

  setupBrowse(form);

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    show("Converting…", true);
    try {
      const outcome = await run("run_convert", readRequest(form));
      const { ok, message } = describe(outcome);
      show(message, ok, (outcome.stdout + outcome.stderr).trim());
    } catch (error) {
      show("Failed to run clab", false, String(error));
    } finally {
      submit.disabled = false;
    }
  });
}
