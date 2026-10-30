/** Uncommitted text only. The caller supplies the current committed display text. */
export class ParameterDrafts {
  private epoch = -1;
  private readonly edited = new Map<string, string>();

  resetForConnection(epoch: number): void {
    if (this.epoch === epoch) return;
    this.epoch = epoch;
    this.edited.clear();
  }

  edit(symbol: string, text: string): void { this.edited.set(symbol, text); }
  discard(symbol: string): void { this.edited.delete(symbol); }
  text(symbol: string, committed: string): string { return this.edited.get(symbol) ?? committed; }
  dirty(symbol: string, committed: string): boolean {
    return this.edited.has(symbol) && this.edited.get(symbol) !== committed;
  }
}
