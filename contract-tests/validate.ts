/**
 * Shared contract validation utilities.
 * Used by the TypeScript runner and the orchestrator for result reporting.
 */

import { readFile } from "node:fs/promises";
import { join } from "node:path";

export interface FieldSpec {
  [key: string]: string;
}

export interface ExpectSpec {
  void?: boolean;
  fields?: FieldSpec;
  values?: Record<string, unknown>;
  type?: string;
  length?: number;
  min_length?: number;
  contains?: string;
  item_fields?: FieldSpec;
  events_length?: number;
  events_min_length?: number;
  event_fields?: FieldSpec;
  first_event_values?: Record<string, unknown>;
  first_event_fields?: FieldSpec;
}

export interface SaveSpec {
  [alias: string]: string;
}

export interface TestCase {
  id: string;
  operation: string;
  input?: Record<string, unknown>;
  expect: ExpectSpec;
  save?: SaveSpec;
}

export interface TestGroup {
  name: string;
  auth: string;
  cases: TestCase[];
}

export interface ContractSpec {
  version: number;
  description: string;
  groups: TestGroup[];
}

export async function loadSpec(): Promise<ContractSpec> {
  const specPath = process.env.HERALD_SPEC_PATH || join(import.meta.dirname, "spec", "spec.json");
  const raw = await readFile(specPath, "utf-8");
  return JSON.parse(raw) as ContractSpec;
}

function typeOf(val: unknown): string {
  if (val === null || val === undefined) return "null";
  if (Array.isArray(val)) return "array";
  return typeof val;
}

export function validateExpect(result: unknown, expect: ExpectSpec, saved: Record<string, unknown>): string[] {
  const errors: string[] = [];

  if (expect.void) return errors;

  if (expect.type === "array") {
    if (!Array.isArray(result)) {
      errors.push(`expected array, got ${typeOf(result)}`);
      return errors;
    }
    if (expect.length !== undefined && result.length !== expect.length) {
      errors.push(`expected array length ${expect.length}, got ${result.length}`);
    }
    if (expect.min_length !== undefined && result.length < expect.min_length) {
      errors.push(`expected array min length ${expect.min_length}, got ${result.length}`);
    }
    if (expect.contains !== undefined) {
      if (!result.includes(expect.contains)) {
        errors.push(`expected array to contain ${JSON.stringify(expect.contains)}`);
      }
    }
    if (expect.item_fields && result.length > 0) {
      errors.push(...validateFields(result[0] as Record<string, unknown>, expect.item_fields));
    }
    return errors;
  }

  const obj = result as Record<string, unknown>;

  if (expect.fields) {
    errors.push(...validateFields(obj, expect.fields));
  }

  if (expect.values) {
    for (const [key, expected] of Object.entries(expect.values)) {
      const resolved = resolveValue(expected, saved);
      if (obj[key] !== resolved) {
        errors.push(`${key}: expected ${JSON.stringify(resolved)}, got ${JSON.stringify(obj[key])}`);
      }
    }
  }

  // EventList-specific checks
  if (expect.events_length !== undefined) {
    const events = obj.events as unknown[];
    if (!Array.isArray(events)) {
      errors.push(`expected events array`);
    } else if (events.length !== expect.events_length) {
      errors.push(`expected events length ${expect.events_length}, got ${events.length}`);
    }
  }

  if (expect.events_min_length !== undefined) {
    const events = obj.events as unknown[];
    if (!Array.isArray(events)) {
      errors.push(`expected events array`);
    } else if (events.length < expect.events_min_length) {
      errors.push(`expected events min length ${expect.events_min_length}, got ${events.length}`);
    }
  }

  if (expect.event_fields) {
    const events = obj.events as Record<string, unknown>[];
    if (Array.isArray(events) && events.length > 0) {
      errors.push(...validateFields(events[0], expect.event_fields));
    }
  }

  if (expect.first_event_values) {
    const events = obj.events as Record<string, unknown>[];
    if (Array.isArray(events) && events.length > 0) {
      for (const [key, expected] of Object.entries(expect.first_event_values)) {
        const resolved = resolveValue(expected, saved);
        if (events[0][key] !== resolved) {
          errors.push(`first event ${key}: expected ${JSON.stringify(resolved)}, got ${JSON.stringify(events[0][key])}`);
        }
      }
    }
  }

  if (expect.first_event_fields) {
    const events = obj.events as Record<string, unknown>[];
    if (Array.isArray(events) && events.length > 0) {
      errors.push(...validateFields(events[0], expect.first_event_fields));
    }
  }

  return errors;
}

function validateFields(obj: Record<string, unknown>, fields: FieldSpec): string[] {
  const errors: string[] = [];
  for (const [key, expectedType] of Object.entries(fields)) {
    if (!(key in obj)) {
      errors.push(`missing field: ${key}`);
      continue;
    }
    const actualType = typeOf(obj[key]);
    if (expectedType === "any") continue;
    if (expectedType === "object" && (actualType === "object" || actualType === "null")) continue;
    if (actualType !== expectedType) {
      errors.push(`${key}: expected type ${expectedType}, got ${actualType}`);
    }
  }
  return errors;
}

function resolveValue(val: unknown, saved: Record<string, unknown>): unknown {
  if (typeof val === "string" && val.startsWith("$")) {
    return saved[val.slice(1)];
  }
  return val;
}

export function resolveInput(input: Record<string, unknown>, saved: Record<string, unknown>): Record<string, unknown> {
  const resolved: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(input)) {
    if (typeof val === "string" && val.startsWith("$")) {
      const ref = val.slice(1);
      if (ref === "event_seq_minus_1") {
        resolved[key] = (saved.event_seq as number) - 1;
      } else {
        resolved[key] = saved[ref];
      }
    } else {
      resolved[key] = val;
    }
  }
  return resolved;
}
