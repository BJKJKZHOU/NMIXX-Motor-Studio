export type ActionMetadata = {
  id: number;
  symbol: string;
  name: string | null;
  description: string;
};

export type ActionHandle = {
  txn: number;
  actionId: number;
  symbol: string;
};

export type ActionCompletion = ActionHandle & {
  status: string;
  ok: boolean;
};
