export type ProblemSeverity = "INFO" | "WARNING" | "ERROR" | "FAULT";
export type ProblemDomain = "PROTECTION";

export type ProblemRecord = {
  id: string;
  severity: ProblemSeverity;
  domain: ProblemDomain;
  summary: string;
  description: string;
  code: number;
  active: boolean;
  firstSeenMs: number;
  lastSeenMs: number;
  occurrenceCount: number;
};

export type ProblemSnapshot = {
  active: ProblemRecord[];
  history: ProblemRecord[];
};
