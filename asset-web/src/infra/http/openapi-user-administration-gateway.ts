import type { UserStatus } from "@/domain/auth";
import type { UserAdministrationGateway } from "@/shared/api/gateways";
import { expectData, expectSuccess, type OpenApiClient } from "./openapi-client";
import { mapManagedUser } from "./openapi-mappers";

export class OpenApiUserAdministrationGateway implements UserAdministrationGateway {
  constructor(private readonly client: OpenApiClient) {}

  async listUsers() {
    const result = await this.client.GET("/auth/users");
    return expectData(result).map(mapManagedUser);
  }

  async createUser(input: { username: string; password: string; isAdmin: boolean }): Promise<void> {
    expectSuccess(
      await this.client.POST("/auth/users", {
        body: {
          username: input.username,
          password: input.password,
          is_admin: input.isAdmin,
        },
      }),
    );
  }

  async updateUserStatus(id: string, status: UserStatus) {
    const result = await this.client.PATCH("/auth/users/{id}", {
      params: { path: { id } },
      body: { status },
    });
    return mapManagedUser(expectData(result));
  }
}
