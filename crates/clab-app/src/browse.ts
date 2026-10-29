import { open } from "@tauri-apps/plugin-dialog";

/** Makes each `[data-browse]` button fill its `data-target` input from a native dialog. */
export function setupBrowse(form: HTMLFormElement): void {
  form.querySelectorAll<HTMLButtonElement>("[data-browse]").forEach((button) => {
    button.addEventListener("click", async () => {
      const selected = await open({ directory: button.dataset.browse === "directory" });
      if (!selected) return;
      const input = form.querySelector<HTMLInputElement>(`[name=${button.dataset.target}]`)!;
      input.value = selected;
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
  });
}
