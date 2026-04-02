export class HeraldError extends Error {
  public readonly code: string;
  public readonly status: number;

  constructor(code: string, message: string, status: number) {
    super(`${code}: ${message}`);
    this.code = code;
    this.status = status;
    this.name = "HeraldError";
  }
}
