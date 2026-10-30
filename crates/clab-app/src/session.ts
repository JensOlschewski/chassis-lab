import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, message, open, save } from "@tauri-apps/plugin-dialog";
import type { ConvertState } from "./convert";
import type { InspectState } from "./inspect";

interface Pane<T> {
  get(): T;
  set(state: T): void | Promise<void>;
}

/** Mirrors `Session` in `src-tauri/src/session.rs`. */
interface SessionFile {
  version: 1;
  embed: boolean;
  convert: ConvertState;
  inspect: InspectState;
}

const FILTERS = [{ name: "ChassisLab session", extensions: ["clab"] }];

const BLANK: SessionFile = {
  version: 1,
  embed: false,
  convert: { input: null, domain: "svc", test: null, output: null, force: false },
  inspect: { file: null, curves: [], plotted: false },
};

const baseName = (path: string) => path.split(/[\\/]/).pop()!;

/**
 * Owns the current session file and its dirty state. A session is dirty when
 * the panes no longer serialize to what was last saved or opened.
 */
export function setupSession(convert: Pane<ConvertState>, inspect: Pane<InspectState>): void {
  const name = document.querySelector<HTMLElement>("#session-name")!;
  const embed = document.querySelector<HTMLInputElement>("#session-embed")!;

  let path: string | null = null;
  let saved = "";
  /** Input paths that point into the opened session's extracted copies. */
  let embeddedPaths = new Set<string>();

  const collect = (): SessionFile => ({
    version: 1,
    embed: embed.checked,
    convert: convert.get(),
    inspect: inspect.get(),
  });
  const isDirty = () => JSON.stringify(collect()) !== saved;

  /** Input paths by form field name. */
  const inputPaths = () => ({ input: convert.get().input, file: inspect.get().file });
  const isEmbedded = (input: string | null) => input !== null && embeddedPaths.has(input);

  const refresh = () => {
    name.textContent = `${path ? baseName(path) : "Untitled"}${isDirty() ? " •" : ""}`;
    name.title = path ?? "";

    const paths = inputPaths();
    document.querySelectorAll<HTMLElement>("[data-embedded-for]").forEach((note) => {
      const input = paths[note.dataset.embeddedFor as keyof typeof paths];
      note.hidden = !isEmbedded(input);
      if (input) note.textContent = `Embedded copy of ${baseName(input)} from this session`;
    });
  };

  const markSaved = () => {
    saved = JSON.stringify(collect());
    refresh();
  };

  async function apply(session: SessionFile): Promise<void> {
    embed.checked = session.embed;
    convert.set(session.convert);
    await inspect.set(session.inspect);
  }

  const fail = (error: unknown) => message(String(error), { title: "ChassisLab", kind: "error" });

  const confirmDiscard = async () =>
    !isDirty() ||
    ask("This session has unsaved changes. Discard them?", {
      title: "ChassisLab",
      kind: "warning",
      okLabel: "Discard",
      cancelLabel: "Cancel",
    });

  async function newSession(): Promise<void> {
    if (!(await confirmDiscard())) return;
    path = null;
    embeddedPaths = new Set();
    await apply(BLANK);
    markSaved();
  }

  async function openSession(): Promise<void> {
    if (!(await confirmDiscard())) return;
    const selected = await open({ filters: FILTERS });
    if (!selected) return;
    try {
      const session = await invoke<SessionFile>("load_session", { path: selected });
      path = selected;
      embeddedPaths = new Set(
        session.embed ? [session.convert.input, session.inspect.file].filter((p): p is string => p !== null) : [],
      );
      await apply(session);
      markSaved();
    } catch (error) {
      await fail(error);
    }
  }

  async function saveSession(asNew: boolean): Promise<void> {
    if (!embed.checked && Object.values(inputPaths()).some(isEmbedded)) {
      await fail("Embedded files have no original location. Keep Embed on, or choose the files again.");
      return;
    }
    const target = !asNew && path ? path : await save({ filters: FILTERS, defaultPath: path ?? "session.clab" });
    if (!target) return;
    try {
      await invoke("save_session", { path: target, session: collect() });
      path = target;
      markSaved();
    } catch (error) {
      await fail(error);
    }
  }

  const actions: Record<string, () => Promise<void>> = {
    new: newSession,
    open: openSession,
    save: () => saveSession(false),
    saveAs: () => saveSession(true),
  };

  document.querySelectorAll<HTMLButtonElement>("[data-session]").forEach((button) =>
    button.addEventListener("click", () => actions[button.dataset.session!]()),
  );

  document.addEventListener("keydown", (event) => {
    if (!(event.ctrlKey || event.metaKey)) return;
    const key = event.key.toLowerCase();
    const action = key === "n" ? "new" : key === "o" ? "open" : key === "s" ? (event.shiftKey ? "saveAs" : "save") : null;
    if (!action) return;
    event.preventDefault();
    void actions[action]();
  });

  document.addEventListener("input", refresh);
  document.addEventListener("change", refresh);

  void getCurrentWindow().onCloseRequested(async (event) => {
    if (!(await confirmDiscard())) event.preventDefault();
  });

  markSaved();
}
