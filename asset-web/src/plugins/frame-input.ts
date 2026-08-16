import type { JsonObject, JsonValue } from "@/domain/plugin";

const maximumActionIdLength = 128;
const maximumJsonDepth = 32;
const maximumJsonValues = 10_000;

export function parseActionId(value: unknown): string {
  if (typeof value !== "string" || !value || value.length > maximumActionIdLength) {
    throw new TypeError(
      `Action ID must be a non-empty string of at most ${maximumActionIdLength} characters.`,
    );
  }
  return value;
}

/** Validates the complete value graph accepted at the untrusted Plugin Frame boundary. */
export function parseActionInput(value: unknown): JsonObject {
  if (value === undefined) return {};
  if (!isPlainObject(value)) {
    throw new TypeError("Action input must be a JSON object.");
  }

  const state = { values: 0, ancestors: new Set<object>() };
  validateJsonValue(value, 0, state);
  return value as JsonObject;
}

function validateJsonValue(
  value: unknown,
  depth: number,
  state: { values: number; ancestors: Set<object> },
): asserts value is JsonValue {
  state.values += 1;
  if (state.values > maximumJsonValues || depth > maximumJsonDepth) {
    throw invalidJsonInput();
  }
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "boolean" ||
    (typeof value === "number" && Number.isFinite(value))
  ) {
    return;
  }
  if (typeof value !== "object") throw invalidJsonInput();
  if (state.ancestors.has(value)) throw invalidJsonInput();

  state.ancestors.add(value);
  try {
    if (Array.isArray(value)) {
      const descriptors = Object.getOwnPropertyDescriptors(value);
      const names = Object.keys(descriptors);
      if (Object.getOwnPropertySymbols(value).length > 0 || names.length !== value.length + 1) {
        throw invalidJsonInput();
      }
      for (let index = 0; index < value.length; index += 1) {
        const descriptor = descriptors[String(index)];
        if (!descriptor?.enumerable || !("value" in descriptor)) throw invalidJsonInput();
        validateJsonValue(descriptor.value, depth + 1, state);
      }
      return;
    }
    if (!isPlainObject(value) || Object.getOwnPropertySymbols(value).length > 0) {
      throw invalidJsonInput();
    }
    for (const descriptor of Object.values(Object.getOwnPropertyDescriptors(value))) {
      if (!descriptor.enumerable || !("value" in descriptor)) throw invalidJsonInput();
      validateJsonValue(descriptor.value, depth + 1, state);
    }
  } finally {
    state.ancestors.delete(value);
  }
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function invalidJsonInput(): TypeError {
  return new TypeError(
    `Action input must contain only JSON-compatible values with at most ${maximumJsonDepth} levels and ${maximumJsonValues} values.`,
  );
}
