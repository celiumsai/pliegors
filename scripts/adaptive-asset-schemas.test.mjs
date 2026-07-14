import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import Ajv2020 from "ajv/dist/2020.js";

const schemaNames = [
  "pliego.adaptive-asset-recipe.schema.json",
  "pliego.adaptive-asset-plan.schema.json",
  "pliego.adaptive-asset-manifest.schema.json",
];

async function json(relativePath) {
  return JSON.parse(
    await readFile(new URL(`../${relativePath}`, import.meta.url), "utf8"),
  );
}

async function validator() {
  const schemas = await Promise.all(
    schemaNames.map((name) => json(`schemas/${name}`)),
  );
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  for (const schema of schemas) ajv.addSchema(schema);
  return { ajv, schemas };
}

test("adaptive asset schemas compile and accept the canonical recipe", async () => {
  const { ajv, schemas } = await validator();
  const recipe = await json("fixtures/adaptive-assets/recipe.json");
  const validate = ajv.getSchema(schemas[0].$id);
  assert.equal(validate(recipe), true, ajv.errorsText(validate.errors));
});

test("adaptive recipe schema rejects traversal and unknown fields", async () => {
  const { ajv, schemas } = await validator();
  const recipe = await json("fixtures/adaptive-assets/recipe.json");
  recipe.assets[0].input = "../outside.png";
  recipe.assets[0].surprise = true;
  const validate = ajv.getSchema(schemas[0].$id);
  assert.equal(validate(recipe), false);
  assert.ok(validate.errors.some((error) => error.keyword === "pattern"));
  assert.ok(
    validate.errors.some((error) => error.keyword === "additionalProperties"),
  );
});
