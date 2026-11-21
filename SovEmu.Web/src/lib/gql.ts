// GraphQL documents typed from the server's schema with gql.tada: each `graphql(...)` call's
// result and variables are inferred by TypeScript from `src/graphql-env.d.ts`, which
// `npm run schema` writes from SovEmu.Server/schema.graphql (kept current by a server test).
// No generated code per operation: a query that asks for a field the schema lacks fails `tsc`.
import { initGraphQLTada } from "gql.tada";
import type { introspection, introspection_types } from "../graphql-env.d.ts";

export const graphql = initGraphQLTada<{
  introspection: introspection;
  scalars: { JSON: unknown };
  // A fragment's fields are part of the result that spreads it, as the pages read them.
  disableMasking: true;
}>();

export type { ResultOf, TadaDocumentNode, VariablesOf } from "gql.tada";

/** A GraphQL enum's values: `Enum<"SettingSource">` is `"CORE" | "SOVEMU" | …`. */
export type Enum<Name extends keyof introspection_types> = introspection_types[Name] extends {
  enumValues: infer V;
}
  ? V
  : never;
