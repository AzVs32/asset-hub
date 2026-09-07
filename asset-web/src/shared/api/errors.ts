export class ConcurrentModificationError extends Error {
  constructor(message = "This item changed elsewhere. The latest version has been loaded.") {
    super(message);
    this.name = "ConcurrentModificationError";
  }
}
