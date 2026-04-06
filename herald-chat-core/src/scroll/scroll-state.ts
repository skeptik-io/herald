import { Notifier } from "../notifier.js";
import type { ScrollStateSnapshot } from "../types.js";

export class ScrollState {
  private _atLiveEdge = true;
  private _pendingCount = 0;
  private _isLoadingMore = false;
  private version = 0;

  constructor(
    private streamId: string,
    private notifier: Notifier,
  ) {}

  setAtLiveEdge(atEdge: boolean): void {
    if (this._atLiveEdge === atEdge) return;
    this._atLiveEdge = atEdge;
    if (atEdge) {
      this._pendingCount = 0;
    }
    this.bump();
  }

  incrementPending(): void {
    if (this._atLiveEdge) return;
    this._pendingCount++;
    this.bump();
  }

  resetPending(): void {
    if (this._pendingCount === 0) return;
    this._pendingCount = 0;
    this.bump();
  }

  setLoadingMore(loading: boolean): void {
    if (this._isLoadingMore === loading) return;
    this._isLoadingMore = loading;
    this.bump();
  }

  get atLiveEdge(): boolean {
    return this._atLiveEdge;
  }

  get pendingCount(): number {
    return this._pendingCount;
  }

  getSnapshot(): ScrollStateSnapshot {
    return {
      atLiveEdge: this._atLiveEdge,
      pendingCount: this._pendingCount,
      isLoadingMore: this._isLoadingMore,
    };
  }

  getVersion(): number {
    return this.version;
  }

  private bump(): void {
    this.version++;
    this.notifier.notify(`scroll:${this.streamId}`);
  }
}
