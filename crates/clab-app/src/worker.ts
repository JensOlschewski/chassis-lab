import { invoke } from "@tauri-apps/api/core";

/** Result of one clab worker run. */
export interface Outcome {
  exit_code: number | null;
  stdout: string;
  stderr: string;
}

export const run = <Request>(command: string, request: Request) =>
  invoke<Outcome>(command, { request });

export function describe(outcome: Outcome): { ok: boolean; message: string } {
  if (outcome.exit_code === 0) return { ok: true, message: "Done" };
  const code = outcome.exit_code ?? "terminated by signal";
  return { ok: false, message: `clab exited with ${code}` };
}
