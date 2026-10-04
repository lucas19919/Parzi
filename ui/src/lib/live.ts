import type { ApprovalCall } from "./api";

export interface Step {
  id: string;
  label: string;
  running: boolean;
  ok?: boolean;
  ms?: number;
  output?: string;
}

export interface LiveTool {
  id: string;
  name: string;
  label: string;
  running: boolean;
  ok: boolean;
  ms: number;
}

export interface Approval {
  key: string;
  session: string;
  call: ApprovalCall;
}
