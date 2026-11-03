import { open, save } from "@tauri-apps/plugin-dialog";

const filtersFor = (extensions?: string[]) => extensions && [{ name: extensions.join(", "), extensions }];

/** Picks an existing file, or a folder; `null` when the user cancels. */
export async function pick(directory: boolean, extensions?: string[]): Promise<string | null> {
  return (await open({ directory, filters: filtersFor(extensions) })) as string | null;
}

/**
 * Makes each `[data-browse]` button fill its `data-target` input from a native dialog:
 * `file` opens an existing file, `directory` a folder, `save` a file name that may not exist yet.
 * A `save` button starts at the path of its `data-suggest-from` input, with the first extension.
 */
export function setupBrowse(form: HTMLFormElement): void {
  const value = (name: string) => form.querySelector<HTMLInputElement>(`[name=${name}]`)!.value;
  form.querySelectorAll<HTMLButtonElement>("[data-browse]").forEach((button) => {
    button.addEventListener("click", async () => {
      const extensions = button.dataset.extensions?.split(",");
      let selected: string | null;
      if (button.dataset.browse === "save") {
        const source = value(button.dataset.suggestFrom ?? "").trim().replace(/^"|"$/g, "");
        const defaultPath = source
          ? source.replace(/\.[^./\\]*$/, `.${extensions?.[0] ?? ""}`)
          : undefined;
        selected = await save({ filters: filtersFor(extensions), defaultPath });
      } else {
        selected = await pick(button.dataset.browse === "directory", extensions);
      }
      if (!selected) return;
      const input = form.querySelector<HTMLInputElement>(`[name=${button.dataset.target}]`)!;
      input.value = selected;
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
  });
}
