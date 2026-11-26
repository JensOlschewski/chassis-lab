import { openUrl } from "@tauri-apps/plugin-opener";
import katex from "katex";
import "katex/dist/katex.min.css";
import { element } from "./sections";

/** A help line: a label, then its text. The text may hold `$...$` math and `<a href="https://…">` links. */
export type Help = [label: string, text: string][];

/** Typesets the `$...$` math of a help text with KaTeX. The text is a constant from our code, not user input. */
const richText = (text: string) =>
  text.replace(/\$([^$]+)\$/g, (_, tex: string) => katex.renderToString(tex, { throwOnError: false }));

/** `https://` links open in the default browser, not in the app window. */
document.addEventListener("click", (event) => {
  const link = (event.target as Element).closest<HTMLAnchorElement>('a[href^="https://"]');
  if (!link) return;
  event.preventDefault();
  void openUrl(link.href);
});

/** Wires a `<dialog>` (h2, p, dl, close button) and returns a function that shows a title, subtitle and rows in it. */
export function setupHelp(dialog: HTMLDialogElement) {
  dialog.querySelector("button")!.addEventListener("click", () => dialog.close());
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) dialog.close();
  });
  // Focus lands on the dialog, not on the first link: a link would show a focus ring when the help opens.
  dialog.tabIndex = -1;
  const subtitleLine = dialog.querySelector<HTMLElement>("p")!;
  return (title: string, subtitle: string | undefined, rows: Help) => {
    dialog.querySelector("h2")!.textContent = title;
    subtitleLine.textContent = subtitle ?? "";
    subtitleLine.hidden = !subtitle;
    dialog.querySelector("dl")!.replaceChildren(
      ...rows.flatMap(([label, text]) => {
        const value = element("dd");
        value.innerHTML = richText(text);
        return [element("dt", label), value];
      }),
    );
    dialog.showModal();
    dialog.focus();
  };
}

/** The `?` of a section header. The click must not fold the section. */
export function helpMark(title: string, show: () => void) {
  const mark = element("button", "?", "help-mark");
  Object.assign(mark, { type: "button", title: `What ${title.split(" ")[0]} is for` });
  mark.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
    show();
  });
  return mark;
}
