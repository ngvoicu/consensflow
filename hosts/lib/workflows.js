import { createPacket } from "./packets.js";
import { getAgent } from "./state.js";
import { runAgent } from "./runners.js";

export async function runNamedAgent(input) {
  const { cwd, agentRef, kind = "ask", task, signal, brief = "", extraContext, handoff, onEvent } = input;
  const agent = typeof agentRef === "object" ? agentRef : await getAgent(cwd, agentRef);
  if (!agent) throw new Error(`Unknown agent: ${agentRef}`);
  const packet = await createPacket({ cwd, agent, kind, task, brief, extraContext, handoff });
  return await runAgent({ cwd, agent, packet, kind, signal, onEvent });
}
