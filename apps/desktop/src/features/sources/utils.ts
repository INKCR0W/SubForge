import type { ConfigSchema } from "../../types/core";
import { formatTimestamp as formatTimestampValue } from "../../lib/ui";

export const SECRET_PLACEHOLDER = "••••••";

export type SourceFormMode = "create" | "edit";

export function getFormConfigValidationMessage(
  schema: ConfigSchema,
  formConfig: Record<string, unknown>,
): string | null {
  for (const [fieldKey, property] of Object.entries(schema.properties)) {
    const rawValue = formConfig[fieldKey];
    const isEmpty = rawValue === undefined || rawValue === null || rawValue === "";
    if (isEmpty) {
      continue;
    }

    if (property.property_type === "integer" && !Number.isInteger(Number(rawValue))) {
      return `${fieldKey} 必须是整数`;
    }
  }

  return null;
}

export function normalizeFormConfigForSubmit(
  schema: ConfigSchema,
  secretFields: string[],
  formConfig: Record<string, unknown>,
  keptSecretFields: string[],
): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  const requiredSet = new Set(schema.required);
  const keptSecretSet = new Set(keptSecretFields);
  const secretFieldSet = new Set(secretFields);

  for (const [fieldKey, property] of Object.entries(schema.properties)) {
    const rawValue = formConfig[fieldKey];
    const isEmpty = rawValue === undefined || rawValue === null || rawValue === "";
    if (secretFieldSet.has(fieldKey) && keptSecretSet.has(fieldKey) && isEmpty) {
      result[fieldKey] = SECRET_PLACEHOLDER;
      continue;
    }

    if (isEmpty) {
      if (property.default !== undefined) {
        result[fieldKey] = property.default;
      } else if (requiredSet.has(fieldKey) && property.property_type === "boolean") {
        result[fieldKey] = false;
      } else if (requiredSet.has(fieldKey)) {
        result[fieldKey] = "";
      } else {
        continue;
      }
      continue;
    }

    if (property.property_type === "integer") {
      const integerValue = Number(rawValue);
      if (!Number.isInteger(integerValue)) {
        throw new Error(`${fieldKey} 必须是整数`);
      }
      result[fieldKey] = integerValue;
      continue;
    }

    if (property.property_type === "number") {
      result[fieldKey] = Number(rawValue);
      continue;
    }

    if (property.property_type === "boolean") {
      result[fieldKey] = Boolean(rawValue);
      continue;
    }

    result[fieldKey] = rawValue;
  }

  return result;
}

export function buildInitialFormConfig(
  schema: ConfigSchema,
  secretFields: string[],
  existingConfig?: Record<string, unknown>,
): {
  values: Record<string, unknown>;
  keptSecretFields: string[];
} {
  const values: Record<string, unknown> = {};
  const keptSecretFields: string[] = [];
  const secretFieldSet = new Set(secretFields);

  for (const [fieldKey, property] of Object.entries(schema.properties)) {
    const currentValue = existingConfig?.[fieldKey];
    if (secretFieldSet.has(fieldKey) && currentValue === SECRET_PLACEHOLDER) {
      values[fieldKey] = "";
      keptSecretFields.push(fieldKey);
      continue;
    }

    if (currentValue !== undefined) {
      values[fieldKey] = currentValue;
      continue;
    }

    if (property.default !== undefined) {
      values[fieldKey] = property.default;
      continue;
    }

    if (property.property_type === "boolean") {
      values[fieldKey] = false;
      continue;
    }

    values[fieldKey] = "";
  }

  return { values, keptSecretFields };
}

export function formatTimestamp(value: string): string {
  return formatTimestampValue(value, value);
}
