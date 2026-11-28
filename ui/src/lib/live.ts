import type { ApprovalCall } from "./api";

export interface Approval {
  key: string;
  session: string;
  call: ApprovalCall;
}
