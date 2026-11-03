import { Channel, invoke } from "@tauri-apps/api/core";

/** Result of one clab worker run. */
export interface Outcome {
  exit_code: number | null;
  stdout: string;
  stderr: string;
}

export const run = <Request>(command: string, request: Request) =>
  invoke<Outcome>(command, { request });

/** Like `run`, for commands that print one result per stdout line: `onLine` gets each line as clab prints it, `stdout` stays empty. */
export const runStreaming = <Request>(command: string, request: Request, onLine: (line: string) => void) => {
  const channel = new Channel<string>();
  channel.onmessage = onLine;
  return invoke<Outcome>(command, { request, onLine: channel });
};

/** The `error: …` line of a failed run, else its last stderr line. */
export function errorLine(stderr: string): string | undefined {
  const lines = stderr.split("\n").map((line) => line.trim()).filter(Boolean);
  const error = lines.find((line) => line.startsWith("error: "));
  return error?.slice("error: ".length) ?? lines.at(-1);
}

export function describe(outcome: Outcome): { ok: boolean; message: string } {
  if (outcome.exit_code === 0) return { ok: true, message: "Done" };
  const code = outcome.exit_code ?? "terminated by signal";
  return { ok: false, message: `clab exited with ${code}` };
}
