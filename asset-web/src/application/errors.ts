export class AuthenticationRequiredError extends Error {
  constructor() {
    super("Authentication is required");
    this.name = "AuthenticationRequiredError";
  }
}

export class ConcurrentModificationError extends Error {
  constructor(message = "This item changed elsewhere. The latest version has been loaded.") {
    super(message);
    this.name = "ConcurrentModificationError";
  }
}
