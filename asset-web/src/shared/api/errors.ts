export class ConcurrentModificationError extends Error {
  constructor(message = "This item changed elsewhere. Review the latest version before retrying.") {
    super(message);
    this.name = "ConcurrentModificationError";
  }
}
