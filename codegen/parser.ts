// Parses an OpenAPI 3.1 spec into the SDK intermediate representation.

import { readFileSync } from "node:fs";
import { parse as parseYaml } from "yaml";
import type { Method, Namespace, Param, ReturnType, RootMethod, SdkDefinition } from "./ir.ts";
import {
  CHAT_NAMESPACES,
  EXCLUDED_PATH_PREFIXES,
  NAMESPACE_CLASSES,
  OPERATION_MAP,
} from "./config.ts";

// ---- OpenAPI types (minimal) ----

interface OpenApiSpec {
  paths: Record<string, Record<string, OpenApiOperation>>;
  components?: { schemas?: Record<string, OpenApiSchema> };
}

interface OpenApiOperation {
  operationId?: string;
  tags?: string[];
  parameters?: OpenApiParameter[];
  requestBody?: { content?: Record<string, { schema?: OpenApiSchema }> };
  responses?: Record<string, { description?: string; content?: Record<string, { schema?: OpenApiSchema }> }>;
  security?: Array<Record<string, string[]>>;
}

interface OpenApiParameter {
  name: string;
  in: "path" | "query" | "header" | "cookie";
  required?: boolean;
  schema?: OpenApiSchema;
}

interface OpenApiSchema {
  $ref?: string;
  type?: string | string[];
  format?: string;
  properties?: Record<string, OpenApiSchema>;
  required?: string[];
  items?: OpenApiSchema;
  enum?: string[];
  minimum?: number;
}

// ---- Helpers ----

function resolveRef(spec: OpenApiSpec, ref: string): OpenApiSchema {
  // "#/components/schemas/Foo" → spec.components.schemas.Foo
  const parts = ref.replace("#/", "").split("/");
  let obj: unknown = spec;
  for (const part of parts) {
    obj = (obj as Record<string, unknown>)[part];
  }
  return obj as OpenApiSchema;
}

function resolveSchema(spec: OpenApiSpec, schema: OpenApiSchema | undefined): OpenApiSchema {
  if (!schema) return {};
  if (schema.$ref) return resolveRef(spec, schema.$ref);
  return schema;
}

/** Convert snake_case to camelCase. */
function camelCase(s: string): string {
  return s.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}

/** Map an OpenAPI schema type to language types. */
function schemaToTypes(schema: OpenApiSchema): { ts: string; go: string; py: string } {
  const rawType = Array.isArray(schema.type)
    ? schema.type.find((t) => t !== "null") ?? "string"
    : schema.type ?? "string";

  switch (rawType) {
    case "integer":
      return { ts: "number", go: "int64", py: "int" };
    case "number":
      return { ts: "number", go: "float64", py: "float" };
    case "boolean":
      return { ts: "boolean", go: "bool", py: "bool" };
    case "string":
      return { ts: "string", go: "string", py: "str" };
    case "array":
      return { ts: "unknown[]", go: "[]any", py: "list" };
    case "object":
      return { ts: "unknown", go: "any", py: "Any" };
    default:
      return { ts: "unknown", go: "any", py: "Any" };
  }
}

function isNullable(schema: OpenApiSchema): boolean {
  if (Array.isArray(schema.type)) return schema.type.includes("null");
  return false;
}

/** Determine the return type for a method from its OpenAPI responses. */
function buildReturnType(
  spec: OpenApiSpec,
  responses: Record<string, { content?: Record<string, { schema?: OpenApiSchema }> }> | undefined,
  unwrapKey: string | undefined,
  unwrapElementType: string | undefined,
): ReturnType {
  if (!responses) {
    return { isVoid: true, tsType: "void", goType: "", pyType: "None", rbType: "nil", unwrapKey: "" };
  }

  // Check for 204 No Content (void return)
  const successCodes = Object.keys(responses).filter((c) => c.startsWith("2"));
  const hasContent = successCodes.some((c) => {
    const r = responses[c];
    return r.content && Object.keys(r.content).some((ct) => ct.includes("json"));
  });

  if (!hasContent) {
    return { isVoid: true, tsType: "void", goType: "", pyType: "None", rbType: "nil", unwrapKey: "" };
  }

  // Find the success response with JSON content
  const successCode = successCodes.find((c) => {
    const r = responses[c];
    return r.content?.["application/json"]?.schema;
  });
  if (!successCode) {
    return { isVoid: true, tsType: "void", goType: "", pyType: "None", rbType: "nil", unwrapKey: "" };
  }

  const schema = resolveSchema(spec, responses[successCode].content?.["application/json"]?.schema);
  const refName = responses[successCode].content?.["application/json"]?.schema?.$ref?.split("/").pop();

  if (unwrapKey) {
    // Response is unwrapped — the method returns the inner array/value
    if (unwrapElementType) {
      return {
        isVoid: false,
        tsType: `${unwrapElementType}[]`,
        goType: `[]${unwrapElementType}`,
        pyType: `list[${unwrapElementType}]`,
        rbType: `Array<${unwrapElementType}>`,
        unwrapKey,
        unwrapElementType,
      };
    }
    // blocks.list unwraps to string[]
    return {
      isVoid: false,
      tsType: "string[]",
      goType: "[]string",
      pyType: "list[str]",
      rbType: "Array<String>",
      unwrapKey,
    };
  }

  // Direct return — map the response schema ref to a type name
  const typeName = refName ?? "unknown";
  return {
    isVoid: false,
    tsType: typeName,
    goType: `*${typeName}`,
    pyType: typeName,
    rbType: typeName,
    unwrapKey: "",
  };
}

// ---- Main parser ----

export function parseSpec(specPath: string): SdkDefinition {
  const raw = readFileSync(specPath, "utf-8");
  const spec: OpenApiSpec = parseYaml(raw);

  const namespaceMap = new Map<string, Method[]>();
  const rootMethods: RootMethod[] = [];

  for (const [path, pathItem] of Object.entries(spec.paths)) {
    // Skip excluded paths
    if (EXCLUDED_PATH_PREFIXES.some((prefix) => path.startsWith(prefix))) continue;

    for (const [httpMethodLower, operation] of Object.entries(pathItem)) {
      const httpMethod = httpMethodLower.toUpperCase();
      if (!["GET", "POST", "PUT", "PATCH", "DELETE"].includes(httpMethod)) continue;

      const opKey = `${httpMethod} ${path}`;
      const mapping = OPERATION_MAP[opKey];
      if (!mapping) continue;

      // Build params
      const pathParams: Param[] = [];
      const queryParams: Param[] = [];
      const bodyParams: Param[] = [];
      const headerParams: Param[] = [];

      // Path and query params from OpenAPI parameters
      if (operation.parameters) {
        for (const p of operation.parameters) {
          const types = schemaToTypes(p.schema ?? {});
          const param: Param = {
            name: camelCase(p.name),
            wireName: p.name,
            tsType: types.ts,
            goType: types.go,
            pyType: types.py,
            required: p.required ?? false,
            location: p.in as "path" | "query",
          };
          if (p.in === "path") pathParams.push(param);
          else if (p.in === "query") queryParams.push(param);
        }
      }

      // Body params from requestBody
      if (operation.requestBody?.content?.["application/json"]?.schema) {
        const bodySchema = resolveSchema(
          spec,
          operation.requestBody.content["application/json"].schema,
        );
        if (bodySchema.properties) {
          const requiredFields = new Set(bodySchema.required ?? []);
          for (const [fieldName, fieldSchema] of Object.entries(bodySchema.properties)) {
            const resolved = resolveSchema(spec, fieldSchema);
            const types = schemaToTypes(resolved);
            // Untyped fields (meta, data, config) → unknown/any
            if (!resolved.type && !resolved.$ref && !resolved.enum) {
              types.ts = "unknown";
              types.go = "any";
              types.py = "Any";
            }
            bodyParams.push({
              name: camelCase(fieldName),
              wireName: fieldName,
              tsType: isNullable(resolved) || !requiredFields.has(fieldName) ? types.ts : types.ts,
              goType: types.go,
              pyType: types.py,
              required: requiredFields.has(fieldName),
              location: "body",
            });
          }
        }
      }

      const returnType = buildReturnType(
        spec,
        operation.responses,
        mapping.unwrapKey,
        mapping.unwrapElementType,
      );

      const method: Method = {
        name: mapping.method,
        httpMethod,
        pathTemplate: path,
        pathParams,
        queryParams,
        bodyParams,
        headerParams,
        returnType,
        operationId: operation.operationId ?? opKey,
      };

      if (mapping.namespace === "_root") {
        const isAdmin = path.startsWith("/admin/");
        rootMethods.push({ ...method, adminOnly: isAdmin });
      } else {
        if (!namespaceMap.has(mapping.namespace)) {
          namespaceMap.set(mapping.namespace, []);
        }
        namespaceMap.get(mapping.namespace)!.push(method);
      }
    }
  }

  // Build namespaces
  const namespaces: Namespace[] = [];
  for (const [nsId, methods] of namespaceMap) {
    namespaces.push({
      id: nsId,
      className: NAMESPACE_CLASSES[nsId] ?? `${nsId.charAt(0).toUpperCase()}${nsId.slice(1)}Namespace`,
      isChat: CHAT_NAMESPACES.has(nsId),
      methods,
    });
  }

  // Sort namespaces by a stable order
  const nsOrder = ["streams", "members", "events", "tenants", "audit", "presence", "blocks"];
  namespaces.sort((a, b) => {
    const ai = nsOrder.indexOf(a.id);
    const bi = nsOrder.indexOf(b.id);
    return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
  });

  return { namespaces, rootMethods };
}
