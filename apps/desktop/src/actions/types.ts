export type ActionHandle = {
  txn: number;
  actionId: number;
};

export type ActionCompletion = ActionHandle & {
  operation: string;
  status: string;
  ok: boolean;
};
