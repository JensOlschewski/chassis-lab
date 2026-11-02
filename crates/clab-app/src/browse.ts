import { open, save } from "@tauri-apps/plugin-dialog";

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
      const filters = extensions && [{ name: extensions.join(", "), extensions }];
      let selected: string | null;
      if (button.dataset.browse === "save") {
        const source = value(button.dataset.suggestFrom ?? "").trim().replace(/^"|"$/g, "");
        const defaultPath = source
          ? source.replace(/\.[^./\\]*$/, `.${extensions?.[0] ?? ""}`)
          : undefined;
        selected = await save({ filters, defaultPath });
      } else {
        selected = (await open({ directory: button.dataset.browse === "directory", filters })) as string | null;
      }
      if (!selected) return;
      const input = form.querySelector<HTMLInputElement>(`[name=${button.dataset.target}]`)!;
      input.value = selected;
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
  });
}
