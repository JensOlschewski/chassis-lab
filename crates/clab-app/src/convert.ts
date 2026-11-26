import { ask } from "@tauri-apps/plugin-dialog";
import { setupBrowse } from "./browse";
import { Help, helpMark, setupHelp } from "./help";
import { describe, run } from "./worker";

/** clab exit code: warnings need the user's confirmation. */
const CONFIRMATION_REQUIRED = 3;

/** Asks in a native dialog whether to proceed despite the warnings. */
function confirmWarnings(warnings: string[]): Promise<boolean> {
  return ask(warnings.join("\n\n"), { title: "Convert warnings", kind: "warning", okLabel: "Proceed", cancelLabel: "Cancel" });
}

interface ConvertRequest {
  input: string;
  test: string;
  subtest: string | null;
  setup: string | null;
  output: string | null;
  force: boolean;
  confirmed: boolean;
}

/** Setup checkboxes in code order: `b<brakes>e<engine>f<front ARB>r<rear ARB>`. */
const SETUP_FIELDS = [
  ["b", "brakes"],
  ["e", "engine"],
  ["f", "front_arb"],
  ["r", "rear_arb"],
] as const;

/** Help of the Setup checkboxes. */
const SETUP_HELP: { subtitle: string; help: Help } = {
  subtitle: "Defines the test conditions for knc test with setup code bX eX fX rX, 0=off, 1=on",
  help: [
    ["b", "Brakes applied."],
    ["e", "Steering engine on."],
    ["f", "Front anti-roll bar connected."],
    ["r", "Rear anti-roll bar connected."],
    ["Lommel data", "The rig stores the setup at the top of the .acd file, as <code>TestInfo:</code>"],
    ["Adams results", "The .res file does not store it. Tick the boxes to match the simulation you ran"],
  ],
};

/** Reads the form into a request; empty optional fields become null. */
export function readRequest(form: HTMLFormElement): ConvertRequest {
  const data = new FormData(form);
  const text = (name: string) => String(data.get(name) ?? "").trim();
  const test = text("test");
  return {
    input: text("input"),
    test,
    // Only knc has tests; clab rejects a subtest for svc.
    subtest: test === "knc" ? text("subtest") : null,
    setup: test === "knc" ? SETUP_FIELDS.map(([letter, name]) => `${letter}${data.has(name) ? 1 : 0}`).join("") : null,
    output: text("output") || null,
    force: data.has("force"),
    confirmed: false,
  };
}

export function setupConvert() {
  const form = document.querySelector<HTMLFormElement>("#convert-form")!;
  const testField = document.querySelector<HTMLElement>("#subtest-field")!;
  const setupField = document.querySelector<HTMLElement>("#setup-field")!;
  const status = document.querySelector<HTMLElement>("#convert-status")!;
  const output = document.querySelector<HTMLElement>("#convert-output")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

  const show = (message: string, ok: boolean, details = "") => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
    output.textContent = details;
  };

  const syncTest = () => {
    testField.hidden = setupField.hidden = readRequest(form).test !== "knc";
  };
  form.addEventListener("change", syncTest);

  const showHelp = setupHelp(document.querySelector<HTMLDialogElement>("#convert-help")!);
  setupField.querySelector("legend")!.append(
    helpMark("Setup", () => showHelp("Setup", SETUP_HELP.subtitle, SETUP_HELP.help)),
  );

  setupBrowse(form);

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    show("Converting…", true);
    try {
      const request = readRequest(form);
      if (!/\.(res|acd)$/i.test(request.input.replace(/["\s]+$/, ""))) {
        show("Input must be a .res or .acd file", false);
        return;
      }
      let outcome = await run("run_convert", request);
      if (outcome.exit_code === CONFIRMATION_REQUIRED) {
        const warnings = outcome.stderr
          .split("\n")
          .filter((line) => line.startsWith("warning: "))
          .map((line) => line.slice("warning: ".length));
        if (!(await confirmWarnings(warnings))) {
          show("Convert cancelled", false);
          return;
        }
        show("Converting…", true);
        outcome = await run("run_convert", { ...request, confirmed: true });
      }
      const { ok, message } = describe(outcome);
      show(message, ok, (outcome.stdout + outcome.stderr).trim());
    } catch (error) {
      show("Failed to run clab", false, String(error));
    } finally {
      submit.disabled = false;
    }
  });
}
