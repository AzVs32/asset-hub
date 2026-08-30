import { AuthenticationRequiredError } from "@/shared/api/errors";
import type { AuthGateway } from "@/shared/api/gateways";
import { HttpError } from "./http-error";
import { expectData, expectSuccess, type OpenApiClient } from "./openapi-client";
import { mapCurrentUser } from "./openapi-mappers";

export class OpenApiAuthGateway implements AuthGateway {
  constructor(private readonly client: OpenApiClient) {}

  async currentUser() {
    try {
      const result = await this.client.GET("/auth/me");
      return mapCurrentUser(expectData(result).user);
    } catch (error) {
      if (error instanceof HttpError && error.status === 401) {
        throw new AuthenticationRequiredError();
      }
      throw error;
    }
  }

  async login(username: string, password: string) {
    const result = await this.client.POST("/auth/login", {
      body: { username, password },
    });
    return mapCurrentUser(expectData(result).user);
  }

  async logout(): Promise<void> {
    expectSuccess(await this.client.POST("/auth/logout"));
  }
}
