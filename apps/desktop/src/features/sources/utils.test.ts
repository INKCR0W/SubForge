import { describe, expect, it } from "vitest";
import type { ConfigSchema, ConfigSchemaProperty } from "../../types/core";
import {
  getFormConfigValidationMessage,
  normalizeFormConfigForSubmit,
} from "./utils";

describe("normalizeFormConfigForSubmit", () => {
  it("secret 保留逻辑不会把 false 或 0 当成留空占位", () => {
    const result = normalizeFormConfigForSubmit(
      createSchema({
        flag_secret: { property_type: "string" },
        zero_secret: { property_type: "string" },
      }),
      ["flag_secret", "zero_secret"],
      {
        flag_secret: false,
        zero_secret: 0,
      },
      ["flag_secret", "zero_secret"],
    );

    expect(result).toEqual({
      flag_secret: false,
      zero_secret: 0,
    });
  });

  it("integer 小数输入会报错而不是静默截断", () => {
    const schema = createSchema({
      interval: { property_type: "integer" },
    });

    expect(getFormConfigValidationMessage(schema, { interval: 1.9 })).toBe(
      "interval 必须是整数",
    );
    expect(() =>
      normalizeFormConfigForSubmit(
        schema,
        [],
        { interval: 1.9 },
        [],
      ),
    ).toThrow("interval 必须是整数");
  });
});

function createSchema(properties: Record<string, ConfigSchemaProperty>): ConfigSchema {
  return {
    schema_type: "object",
    required: [],
    properties,
  };
}
