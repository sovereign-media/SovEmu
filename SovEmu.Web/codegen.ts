// Typed GraphQL documents for the web client, generated from the server's checked-in schema
// (SovEmu.Backend/SovEmu.Server/schema.graphql, kept current by a server test). The output in
// src/graphql/ is committed, so builds need neither the server nor its source.
//
//   npm run codegen          # after changing a query or the schema
//   npm run codegen:check    # CI: fails if src/graphql/ is out of date
import type { CodegenConfig } from "@graphql-codegen/cli";

const config: CodegenConfig = {
  schema: "../SovEmu.Backend/SovEmu.Server/schema.graphql",
  documents: ["src/**/*.{ts,tsx}", "!src/graphql/**"],
  ignoreNoDocuments: true,
  generates: {
    "./src/graphql/": {
      preset: "client",
      presetConfig: { fragmentMasking: false },
      config: {
        documentMode: "string",
        useTypeImports: true,
        enumsAsTypes: true,
        scalars: { JSON: "unknown" },
      },
    },
  },
};
export default config;
